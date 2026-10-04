//! Microphone capture, plus helpers to turn it into 16 kHz mono WAV for Whisper.
//!
//! The mic is only open while a take is being recorded. Keeping it open would hold Bluetooth
//! headsets in their low-quality hands-free mode (game audio too) the whole time.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const WHISPER_RATE: u32 = 16_000;
const MAX_SECONDS: usize = 35;

#[derive(Default)]
struct Shared {
    /// 0 until the stream is open.
    rate: usize,
    buf: Vec<f32>,
    device_name: String,
    error: Option<String>,
}

impl Shared {
    fn push(&mut self, mono: impl Iterator<Item = f32>) {
        let max = self.rate * MAX_SECONDS;
        for s in mono {
            if self.buf.len() < max {
                self.buf.push(s);
            }
        }
    }
}

/// One take: the mic is opened on a background thread when this is created and closed when it's
/// finished or dropped.
pub struct Recorder {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
}

/// What a finished take recorded.
pub struct Take {
    pub samples: Vec<f32>,
    pub rate: u32,
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

fn open_and_run(name_contains: &str, shared: &Arc<Mutex<Shared>>, stop: &AtomicBool) -> Result<(), String> {
    let device = pick_device(name_contains).ok_or("No microphone found")?;
    let device_name = device
        .description()
        .map(|n| n.name().to_string())
        .unwrap_or_else(|_| "Unknown mic".into());
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    {
        let mut s = shared.lock().unwrap();
        s.rate = config.sample_rate as usize;
        s.device_name = device_name;
    }
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, shared.clone()),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, shared.clone()),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, shared.clone()),
        SampleFormat::I32 => build_stream::<i32>(&device, &config, shared.clone()),
        other => return Err(format!("Unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

impl Recorder {
    /// Opens the mic whose name contains `name_contains` (or the Windows default) and starts
    /// recording as soon as audio arrives. Returns at once; errors show up in `finish`.
    pub fn start(name_contains: &str) -> Recorder {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let name = name_contains.to_string();
        let (thread_shared, thread_stop) = (shared.clone(), stop.clone());
        // cpal streams aren't Send on every backend, so the stream lives and dies on this thread.
        thread::spawn(move || {
            if let Err(e) = open_and_run(&name, &thread_shared, &thread_stop) {
                thread_shared.lock().unwrap().error = Some(e);
            }
        });
        Recorder { shared, stop }
    }

    /// True once the mic is delivering audio (Bluetooth headsets take a moment to switch modes).
    pub fn is_live(&self) -> bool {
        !self.shared.lock().unwrap().buf.is_empty()
    }

    /// Closes the mic and returns what it recorded.
    pub fn finish(self) -> Result<Take, String> {
        self.stop.store(true, Ordering::Relaxed);
        let mut s = self.shared.lock().unwrap();
        if let Some(e) = s.error.take() {
            return Err(e);
        }
        Ok(Take { samples: std::mem::take(&mut s.buf), rate: s.rate as u32, device_name: s.device_name.clone() })
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
