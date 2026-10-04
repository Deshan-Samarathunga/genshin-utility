//! Reads the DualSense mic-mute button straight from HID (shared access, Genshin keeps working).

use hidapi::{HidApi, HidDevice};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

const SONY_VID: u16 = 0x054C;
const DUALSENSE_PIDS: [u16; 2] = [0x0CE6, 0x0DF2]; // DualSense, DualSense Edge
const MIC_BUTTON_MASK: u8 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadEvent {
    Connected(bool),
    MicDown,
    MicUp,
}

/// USB output report that switches the controller's built-in mic back on and its mute LED off.
/// The mic button also toggles the firmware's mic mute, which silenced every other recording.
fn unmute_report() -> [u8; 48] {
    let mut report = [0u8; 48];
    report[0] = 0x02; // output report id (USB)
    report[2] = 0x01 | 0x02; // valid_flag1: mic-mute LED + power-save (mic mute) control
    report[9] = 0x00; // mute button LED off
    report[10] = 0x00; // power_save_control: clear MIC_MUTE (bit 4)
    report
}

/// Mic button state from an input report, or None for reports we don't understand.
fn mic_pressed(report: &[u8]) -> Option<bool> {
    match report.first()? {
        // USB full report: buttons at [8..=10], mic in byte 10.
        0x01 if report.len() >= 11 => Some(report[10] & MIC_BUTTON_MASK != 0),
        // Bluetooth extended report has one extra header byte.
        0x31 if report.len() >= 12 => Some(report[11] & MIC_BUTTON_MASK != 0),
        _ => None,
    }
}

fn open_pad(api: &mut HidApi) -> Option<HidDevice> {
    let _ = api.refresh_devices();
    api.device_list()
        .filter(|d| d.vendor_id() == SONY_VID && DUALSENSE_PIDS.contains(&d.product_id()))
        // Gamepad top-level collection (Generic Desktop / Game Pad).
        .filter(|d| d.usage_page() == 0x01 && d.usage() == 0x05)
        .find_map(|d| d.open_device(api).ok())
}

/// Spawns the reader thread. It only holds the device open while `enabled` is set.
/// Setting `unmute` asks it to switch the controller's mic back on (checked every ~50 ms).
pub fn spawn(enabled: Arc<AtomicBool>, unmute: Arc<AtomicBool>, tx: UnboundedSender<PadEvent>) {
    thread::spawn(move || {
        let mut api = match HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                eprintln!("voice: hidapi init failed: {e}");
                return;
            }
        };
        let mut buf = [0u8; 128];

        loop {
            if !enabled.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(250));
                continue;
            }

            let Some(device) = open_pad(&mut api) else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            let _ = tx.send(PadEvent::Connected(true));
            let _ = device.write(&unmute_report());
            let mut pressed = false;

            while enabled.load(Ordering::Relaxed) {
                if unmute.swap(false, Ordering::Relaxed) {
                    let _ = device.write(&unmute_report());
                }
                match device.read_timeout(&mut buf, 50) {
                    Ok(0) => {}
                    Ok(n) => {
                        if let Some(now_pressed) = mic_pressed(&buf[..n]) {
                            if now_pressed != pressed {
                                pressed = now_pressed;
                                let _ = tx.send(if pressed { PadEvent::MicDown } else { PadEvent::MicUp });
                            }
                        }
                    }
                    Err(_) => break, // unplugged
                }
            }

            if pressed {
                let _ = tx.send(PadEvent::MicUp);
            }
            let _ = tx.send(PadEvent::Connected(false));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmute_report_layout() {
        let r = unmute_report();
        assert_eq!((r[0], r[1], r[2], r[10]), (0x02, 0x00, 0x03, 0x00));
    }

    #[test]
    fn parses_usb_and_bt_reports() {
        let mut usb = [0u8; 64];
        usb[0] = 0x01;
        assert_eq!(mic_pressed(&usb), Some(false));
        usb[10] = 0x04;
        assert_eq!(mic_pressed(&usb), Some(true));

        let mut bt = [0u8; 78];
        bt[0] = 0x31;
        bt[11] = 0x04;
        assert_eq!(mic_pressed(&bt), Some(true));

        assert_eq!(mic_pressed(&[0x01, 0, 0]), None);
    }
}
