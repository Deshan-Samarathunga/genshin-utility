//! Reads the DualSense PS button (the talk button) straight from HID, over USB or Bluetooth.
//! Shared access and read-only, so Genshin keeps working. On Bluetooth nothing is ever sent to the
//! controller: switching it out of its basic report mode stops Genshin from seeing any input.

use hidapi::{BusType, HidApi, HidDevice};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

const SONY_VID: u16 = 0x054C;
const DUALSENSE_PIDS: [u16; 2] = [0x0CE6, 0x0DF2]; // DualSense, DualSense Edge
const PS_BUTTON_MASK: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadEvent {
    Connected { connected: bool, bluetooth: bool },
    TalkDown,
    TalkUp,
}

/// USB output report that switches the controller's built-in mic back on and its mute LED off.
/// (Its mute can be left on, e.g. by the mic button, which silences recordings.)
fn usb_unmute_report() -> [u8; 48] {
    let mut report = [0u8; 48];
    report[0] = 0x02; // output report id (USB)
    report[2] = 0x01 | 0x02; // valid_flag1: mic-mute LED + power-save (mic mute) control
    report[9] = 0x00; // mute button LED off
    report[10] = 0x00; // power_save_control: clear MIC_MUTE (bit 4)
    report
}

/// PS button state from an input report, or None for reports we don't understand.
/// Windows pads every report to the longest input report, so lengths don't tell them apart.
fn talk_pressed(report: &[u8], bluetooth: bool) -> Option<bool> {
    let byte = match (report.first()?, bluetooth) {
        (0x01, false) => 10, // USB full report
        (0x01, true) => 7,   // Bluetooth basic report (the rest of the byte is a frame counter)
        (0x31, _) => 11,     // Bluetooth full report (only if something else switched to it)
        _ => return None,
    };
    report.get(byte).map(|b| b & PS_BUTTON_MASK != 0)
}

struct Pad {
    device: HidDevice,
    bluetooth: bool,
}

impl Pad {
    fn unmute(&self) {
        if !self.bluetooth {
            let _ = self.device.write(&usb_unmute_report());
        }
    }
}

fn open_pad(api: &mut HidApi) -> Option<Pad> {
    let _ = api.refresh_devices();
    api.device_list()
        .filter(|d| d.vendor_id() == SONY_VID && DUALSENSE_PIDS.contains(&d.product_id()))
        // Gamepad top-level collection (Generic Desktop / Game Pad).
        .filter(|d| d.usage_page() == 0x01 && d.usage() == 0x05)
        .find_map(|d| {
            let device = d.open_device(api).ok()?;
            Some(Pad { device, bluetooth: matches!(d.bus_type(), BusType::Bluetooth) })
        })
}

/// Spawns the reader thread. It only holds the device open while `enabled` is set.
/// Setting `unmute` asks it to switch the controller's mic back on (USB only, checked every ~50 ms).
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

            let Some(pad) = open_pad(&mut api) else {
                thread::sleep(Duration::from_secs(2));
                continue;
            };
            let _ = tx.send(PadEvent::Connected { connected: true, bluetooth: pad.bluetooth });
            pad.unmute();
            let mut pressed = false;

            while enabled.load(Ordering::Relaxed) {
                if unmute.swap(false, Ordering::Relaxed) {
                    pad.unmute();
                }
                match pad.device.read_timeout(&mut buf, 50) {
                    Ok(0) => {}
                    Ok(n) => {
                        if let Some(now_pressed) = talk_pressed(&buf[..n], pad.bluetooth) {
                            if now_pressed != pressed {
                                pressed = now_pressed;
                                let _ = tx.send(if pressed { PadEvent::TalkDown } else { PadEvent::TalkUp });
                            }
                        }
                    }
                    Err(_) => break, // unplugged / disconnected
                }
            }

            if pressed {
                let _ = tx.send(PadEvent::TalkUp);
            }
            let _ = tx.send(PadEvent::Connected { connected: false, bluetooth: pad.bluetooth });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_unmute_report_layout() {
        let r = usb_unmute_report();
        assert_eq!((r[0], r[1], r[2], r[10]), (0x02, 0x00, 0x03, 0x00));
    }

    #[test]
    fn reads_ps_button_over_usb_and_bluetooth() {
        let mut usb = [0u8; 64];
        usb[0] = 0x01;
        usb[10] = 0x04; // mic button: not the talk button
        assert_eq!(talk_pressed(&usb, false), Some(false));
        usb[10] |= 0x01;
        assert_eq!(talk_pressed(&usb, false), Some(true));

        // Bluetooth basic report, padded by Windows.
        let mut basic = [0u8; 78];
        basic[0] = 0x01;
        basic[10] = 0x01;
        basic[7] = 0x08; // frame counter only
        assert_eq!(talk_pressed(&basic, true), Some(false));
        basic[7] |= 0x01;
        assert_eq!(talk_pressed(&basic, true), Some(true));

        let mut full = [0u8; 78];
        full[0] = 0x31;
        full[11] = 0x01;
        assert_eq!(talk_pressed(&full, true), Some(true));

        assert_eq!(talk_pressed(&[0x01, 0, 0], false), None);
        assert_eq!(talk_pressed(&[0x05; 20], false), None);
    }
}
