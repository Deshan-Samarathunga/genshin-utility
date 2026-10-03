//! Microphone capture with a short pre-roll, plus helpers to turn it into 16 kHz mono WAV for Whisper.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const WHISPER_RATE: u32 = 16_000;
const PREROLL_MS: usize = 300;
const MAX_SECONDS: usize = 35;

struct Shared {
    rate: usize,
    preroll: VecDeque<f32>,
    recording: bool,
    buf: Vec<f32>,
}

impl Shared {
    fn push(&mut self, mono: impl Iterator<Item = f32>) {
        let preroll_cap = self.rate * PREROLL_MS / 1000;
        let max = self.rate * MAX_SECONDS;
        for s in mono {
            if self.recording {
                if self.buf.len() < max {
                    self.buf.push(s);
                }
            } else {
                if self.preroll.len() >= preroll_cap {
                    self.preroll.pop_front();
                }
                self.preroll.push_back(s);
            }
        }
    }
}

/// Keeps an input stream open on its own thread for as long as it lives.
pub struct Recorder {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    pub device_name: String,
}

pub fn input_device_names() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devs| devs.filter_map(|d| d.description().ok().map(|n| n.name().to_string())).collect())
        .unwrap_or_default()
}

fn pick_device(name_contains: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let wanted = name_contains.trim().to_lowercase();
    if !wanted.is_empty() {
        if let Ok(devs) = host.input_devices() {
            for d in devs {
                let matches = d
                    .description()
                    .map(|n| n.name().to_lowercase().contains(&wanted))
                    .unwrap_or(false);
                if matches {
                    return Some(d);
                }
            }
        }
    }
    host.default_input_device()
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Mutex<Shared>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    device.build_input_stream(
        *config,
        move |data: &[T], _| {
            if let Ok(mut s) = shared.lock() {
                s.push(
                    data.chunks(channels)
                        .map(|frame| frame.iter().map(|v| f32::from_sample(*v)).sum::<f32>() / channels as f32),
                );
            }
        },
        |e| eprintln!("voice: audio stream error: {e}"),
        None,
    )
}

impl Recorder {
    pub fn start(name_contains: &str) -> Result<Recorder, String> {
        let device = pick_device(name_contains).ok_or("No microphone found")?;
        let device_name = device
            .description()
            .map(|n| n.name().to_string())
            .unwrap_or_else(|_| "Unknown mic".into());
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config = supported.config();

        let shared = Arc::new(Mutex::new(Shared {
            rate: config.sample_rate as usize,
            preroll: VecDeque::new(),
            recording: false,
            buf: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));

        // cpal streams aren't Send on every backend, so the stream lives and dies on this thread.
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        {
            let shared = shared.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                let stream = match format {
                    SampleFormat::F32 => build_stream::<f32>(&device, &config, shared),
                    SampleFormat::I16 => build_stream::<i16>(&device, &config, shared),
                    SampleFormat::U16 => build_stream::<u16>(&device, &config, shared),
                    SampleFormat::I32 => build_stream::<i32>(&device, &config, shared),
                    other => {
                        let _ = ready_tx.send(Err(format!("Unsupported sample format {other:?}")));
                        return;
                    }
                };
                let stream = match stream.map_err(|e| e.to_string()).and_then(|s| {
                    s.play().map_err(|e| e.to_string())?;
                    Ok(s)
                }) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
                drop(stream);
            });
        }

        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Timed out opening microphone".to_string())??;

        Ok(Recorder { shared, stop, device_name })
    }

    pub fn begin(&self) {
        let mut s = self.shared.lock().unwrap();
        let preroll: Vec<f32> = s.preroll.drain(..).collect();
        s.buf = preroll;
        s.recording = true;
    }

    /// Stops recording and returns (mono samples, sample rate).
    pub fn end(&self) -> (Vec<f32>, u32) {
        let mut s = self.shared.lock().unwrap();
        s.recording = false;
        (std::mem::take(&mut s.buf), s.rate as u32)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Resamples mono audio to 16 kHz. Integer ratios (48k, 32k) average blocks; others interpolate linearly.
pub fn resample_to_16k(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == WHISPER_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    if rate.is_multiple_of(WHISPER_RATE) {
        let factor = (rate / WHISPER_RATE) as usize;
        return samples
            .chunks(factor)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect();
    }
    let ratio = rate as f64 / WHISPER_RATE as f64;
    let out_len = (samples.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = samples[idx];
            let b = *samples.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Scales quiet recordings up (controller mics are soft) without clipping.
pub fn normalize(samples: &mut [f32]) {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 && peak < 0.5 {
        let gain = (0.9 / peak).min(8.0);
        samples.iter_mut().for_each(|s| *s *= gain);
    }
}

pub fn wav_bytes(samples_16k: &[f32]) -> Result<Vec<u8>, String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|e| e.to_string())?;
        for s in samples_16k {
            writer
                .write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_48k_by_averaging() {
        let input: Vec<f32> = (0..48).map(|i| i as f32).collect();
        let out = resample_to_16k(&input, 48_000);
        assert_eq!(out.len(), 16);
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn resamples_44k1_to_expected_length() {
        let input = vec![0.5f32; 44_100];
        let out = resample_to_16k(&input, 44_100);
        assert_eq!(out.len(), 16_000);
        assert!((out[100] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn wav_has_header() {
        let wav = wav_bytes(&vec![0.0; 1600]).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 3200);
    }
}
