//! Reads text off the game window: a screen grab of the foreground window's client area, cropped to
//! a region and run through the Windows OCR engine (offline, built into Windows 10/11).

use std::sync::mpsc;
use std::sync::OnceLock;
use std::thread;
use tokio::sync::oneshot;
use windows::Foundation::Rect;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetForegroundWindow};

/// Part of the window, as fractions of its client area (left, top, right, bottom).
pub type Region = (f32, f32, f32, f32);

/// A line of text and where it is on screen (screen pixels).
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub center: (i32, i32),
    /// Left edge of the text, as a fraction of the window width.
    pub left: f32,
    /// A white round icon (the speech bubble on dialogue options) sits just left of the text.
    pub icon: bool,
    /// Gold text: a speaker's name (or title) above a subtitle; spoken lines are white.
    pub gold: bool,
}

/// One look at the game window.
pub struct Screen {
    /// Lines read in each requested region, in order.
    pub regions: Vec<Vec<Line>>,
    /// The gold "continue" diamond is at the bottom: a conversation line is finished and waiting.
    pub can_continue: bool,
    /// Centres of red "!" badges in the middle of the window (screen pixels). On the character
    /// screen these sit at the top right of empty artifact slots.
    pub red_badges: Vec<(i32, i32)>,
    /// Character screen, Artifacts page: Some(true) when the Max HP / ATK / DEF / EM bonuses all
    /// show "+0", Some(false) when one is wider (a real value), None when they can't be seen.
    pub artifact_stats_zero: Option<bool>,
}

/// A BGRA screen grab of the window's client area.
struct Frame {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

fn capture_foreground() -> Option<Frame> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut rc = RECT::default();
        GetClientRect(hwnd, &mut rc).ok()?;
        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        let (width, height) = (rc.right - rc.left, rc.bottom - rc.top);
        if width < 320 || height < 240 {
            return None;
        }

        // Copy from the screen, not the window: games render with DirectX and a window DC reads black.
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bitmap.into());
        let copied = BitBlt(mem, 0, 0, width, height, Some(screen), origin.x, origin.y, SRCCOPY).is_ok();
        SelectObject(mem, old);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let rows = GetDIBits(mem, bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        (copied && rows == height).then_some(Frame { left: origin.x, top: origin.y, width, height, pixels })
    }
}

/// Share (0-100) of near-white pixels in a box, given as offsets from a text line's left edge and
/// vertical centre in thousandths of the window size.
fn bright_share(frame: &Frame, text_left: i32, center_y: i32, x: (i32, i32), y: (i32, i32)) -> u32 {
    let (w, h) = (frame.width, frame.height);
    let (x0, x1) = (text_left + w * x.0 / 1000, text_left + w * x.1 / 1000);
    let (y0, y1) = (center_y + h * y.0 / 1000, center_y + h * y.1 / 1000);
    if x0 < 0 || y0 < 0 || x1 >= w || y1 >= h {
        return 0;
    }
    let (mut bright, mut total) = (0u32, 0u32);
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let i = ((yy * w + xx) * 4) as usize;
            total += 1;
            if frame.pixels[i].min(frame.pixels[i + 1]).min(frame.pixels[i + 2]) > 200 {
                bright += 1;
            }
        }
    }
    bright * 100 / total.max(1)
}

/// True for the white speech-bubble icon that every talk prompt and dialogue option has, sitting on
/// the prompt's dark bar. Party names, nameplates and quest text have no icon; light menu rows (shop
/// items) are bright around the "icon" too, so they're rejected.
fn has_icon(frame: &Frame, text_left: i32, center_y: i32) -> bool {
    let icon = bright_share(frame, text_left, center_y, (-26, -6), (-10, 10));
    let outside = bright_share(frame, text_left, center_y, (-34, -30), (-6, 6));
    // Some column between the icon and the text is dark (the text's left edge is only known to a few
    // pixels, so look across a small band rather than at one spot).
    let gap = (-10..=-1).any(|x| bright_share(frame, text_left, center_y, (x, x), (-10, 10)) < 20);
    icon >= 30 && outside < 40 && gap
}

/// Crops a region, turns bright text black on white (game text is light on a busy background) and
/// scales small windows up so the text is big enough for the OCR engine.
fn prepare(frame: &Frame, region: Region) -> (Vec<u8>, i32, i32, i32, i32, i32) {
    let x0 = (frame.width as f32 * region.0) as i32;
    let y0 = (frame.height as f32 * region.1) as i32;
    let w = (frame.width as f32 * (region.2 - region.0)) as i32;
    let h = (frame.height as f32 * (region.3 - region.1)) as i32;
    let scale = if frame.height < 1400 { 2 } else { 1 };
    let (out_w, out_h) = (w * scale, h * scale);
    let mut out = vec![255u8; (out_w * out_h * 4) as usize];
    for y in 0..out_h {
        let src_row = ((y0 + y / scale) * frame.width) as usize;
        for x in 0..out_w {
            let i = (src_row + (x0 + x / scale) as usize) * 4;
            let (b, g, r) = (frame.pixels[i] as u32, frame.pixels[i + 1] as u32, frame.pixels[i + 2] as u32);
            let luma = (r * 299 + g * 587 + b * 114) / 1000;
            if luma > 175 {
                let o = ((y * out_w + x) * 4) as usize;
                out[o..o + 3].fill(0);
            }
        }
    }
    (out, out_w, out_h, x0, y0, scale)
}

/// Words of each recognized line with their boxes (in the prepared image's pixels).
fn recognize(engine: &OcrEngine, pixels: &[u8], width: i32, height: i32) -> windows::core::Result<Vec<Vec<(String, Rect)>>> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(pixels)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width,
        height,
        BitmapAlphaMode::Premultiplied,
    )?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut words = Vec::new();
        for word in line.Words()? {
            words.push((word.Text()?.to_string_lossy(), word.BoundingRect()?));
        }
        if !words.is_empty() {
            lines.push(words);
        }
    }
    Ok(lines)
}

/// A one- or two-character "word" with no real letters, like the speech-bubble icon read as "o" or
/// "@". It isn't part of the text and would hide where the text starts.
/// Windows OCR reads the bubble as "Q" (sometimes "O" or a symbol); a lone lowercase letter there is
/// the icon too. A real "I" or "A" starting an option is kept.
fn is_icon_word(word: &str) -> bool {
    let mut chars = word.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(c), None, None) => !c.is_alphabetic() || c.is_lowercase() || matches!(c, 'Q' | 'O' | 'Ø'),
        (Some(a), Some(b), None) => !a.is_alphabetic() && !b.is_alphabetic(),
        _ => false,
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.X.min(b.X), a.Y.min(b.Y));
    let (x1, y1) = ((a.X + a.Width).max(b.X + b.Width), (a.Y + a.Height).max(b.Y + b.Height));
    Rect { X: x0, Y: y0, Width: x1 - x0, Height: y1 - y0 }
}

/// True when most of the bright (text) pixels in a box are gold. Names measure 57-100% gold, white
/// subtitles at most ~22% (a gold carpet showing between the letters).
fn gold_text(frame: &Frame, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
    let (mut bright, mut gold) = (0u32, 0u32);
    for y in y0.max(0)..y1.min(frame.height) {
        for x in x0.max(0)..x1.min(frame.width) {
            let i = ((y * frame.width + x) * 4) as usize;
            let (b, g, r) = (frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2]);
            if r.max(g).max(b) > 200 {
                bright += 1;
                gold += is_gold(b, g, r) as u32;
            }
        }
    }
    bright > 0 && gold * 100 >= bright * 40
}

/// Reads one region of a frame into screen-positioned lines.
fn read_region(engine: &OcrEngine, frame: &Frame, region: Region) -> Vec<Line> {
    let (pixels, w, h, x0, y0, scale) = prepare(frame, region);
    recognize(engine, &pixels, w, h)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|mut words| {
            if words.len() > 1 && is_icon_word(&words[0].0) {
                words.remove(0);
            }
            let bounds = words.iter().map(|(_, r)| *r).reduce(union)?;
            let text = words.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" ");
            let left = x0 + bounds.X as i32 / scale;
            let cy = y0 + ((bounds.Y + bounds.Height / 2.0) as i32) / scale;
            let (top, right, bottom) = (
                y0 + bounds.Y as i32 / scale,
                x0 + (bounds.X + bounds.Width) as i32 / scale,
                y0 + (bounds.Y + bounds.Height) as i32 / scale,
            );
            Some(Line {
                text,
                center: (frame.left + x0 + ((bounds.X + bounds.Width / 2.0) as i32) / scale, frame.top + cy),
                left: left as f32 / frame.width as f32,
                icon: has_icon(frame, left, cy),
                gold: gold_text(frame, left, top, right, bottom),
            })
        })
        .collect()
}

/// Gold like the continue diamond's outline (dim edges included), from BGRA bytes.
fn is_gold(b: u8, g: u8, r: u8) -> bool {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    r >= 130.0 && r - b >= 70.0 && g >= 0.5 * r && g <= 0.92 * r && b <= 0.45 * r + 20.0
}

/// Finds the gold diamond outline Genshin shows at the bottom centre when a conversation line has
/// finished. Matches the shape (gold on the outline, none inside or around it), because gold carpets
/// and flowers near the bottom have plenty of gold pixels too.
fn continue_marker(frame: &Frame) -> bool {
    let (w, h) = (frame.width, frame.height);
    let scale = h as f32 / 1080.0;
    let gold = |x: i32, y: i32| -> Option<bool> {
        if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        }
        let i = ((y * w + x) * 4) as usize;
        Some(is_gold(frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2]))
    };
    let (r_min, r_max) = ((10.0 * scale) as i32, (17.0 * scale) as i32);
    for cy in (h * 94 / 100)..(h * 99 / 100) {
        for cx in (w / 2 - 3)..=(w / 2 + 3) {
            for r in r_min..=r_max {
                let [mut ring, mut ring_n, mut inside, mut inside_n, mut outside, mut outside_n] = [0u32; 6];
                for dy in -(r + 5)..=(r + 5) {
                    for dx in -(r + 5)..=(r + 5) {
                        let d = dx.abs() + dy.abs();
                        let Some(g) = gold(cx + dx, cy + dy) else { continue };
                        let g = g as u32;
                        if (d - r).abs() <= 2 {
                            ring += g;
                            ring_n += 1;
                        } else if d <= r - 5 {
                            inside += g;
                            inside_n += 1;
                        } else if d >= r + 4 {
                            outside += g;
                            outside_n += 1;
                        }
                    }
                }
                let share = |n: u32, of: u32| n as f32 / of.max(1) as f32;
                if share(ring, ring_n) - share(inside, inside_n) - share(outside, outside_n) >= 0.25 {
                    return true;
                }
            }
        }
    }
    false
}

/// Red "!" badges between the side menus (x 18-84%, y 39-91% of the window). Clusters of bright red
/// of badge size; big red areas (clothing) are left out.
/// The four artifact stat values are right-aligned white text ending at x≈1792 (1080p). "+0" starts
/// at x≈1766; any real value ("+33", "+4,780") starts further left. Measured per row from the
/// leftmost column with white text, ignoring the labels further left.
fn artifact_stats_zero(frame: &Frame) -> Option<bool> {
    let (w, h) = (frame.width, frame.height);
    let sx = |x: i32| x * w / 1920;
    let sy = |y: i32| y * h / 1080;
    let mut all_zero = true;
    for row in [142, 178, 214, 250] {
        let text_column = |x: i32| {
            (sy(row - 9)..sy(row + 10))
                .filter(|&y| {
                    let i = ((y * w + x) * 4) as usize;
                    frame.pixels[i].min(frame.pixels[i + 1]).min(frame.pixels[i + 2]) > 215
                })
                .count()
                >= 2
        };
        // Walk left from the right edge through the value; a gap wider than the space between two
        // characters ends it, so background sparkles further left don't count.
        let (mut left, mut gap) = (None, 0);
        for x in (sx(1700)..sx(1806)).rev() {
            if text_column(x) {
                left = Some(x);
                gap = 0;
            } else if left.is_some() {
                gap += 1;
                if gap > sx(9) {
                    break;
                }
            }
        }
        let left = left?; // no value in this row: not the stats panel
        if left < sx(1758) {
            all_zero = false;
        }
    }
    Some(all_zero)
}

fn red_badges(frame: &Frame) -> Vec<(i32, i32)> {
    let (w, h) = (frame.width, frame.height);
    let mut clusters: Vec<(i64, i64, i64)> = Vec::new(); // sum x, sum y, count (sampled every 2 px)
    for y in (h * 39 / 100..h * 91 / 100).step_by(2) {
        for x in (w * 18 / 100..w * 84 / 100).step_by(2) {
            let i = ((y * w + x) * 4) as usize;
            let (b, g, r) = (frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2]);
            if r > 200 && g < 100 && b < 110 {
                let near = clusters.iter_mut().find(|c| {
                    (c.0 / c.2 - x as i64).abs() < 30 && (c.1 / c.2 - y as i64).abs() < 30
                });
                match near {
                    Some(c) => {
                        c.0 += x as i64;
                        c.1 += y as i64;
                        c.2 += 1;
                    }
                    None => clusters.push((x as i64, y as i64, 1)),
                }
            }
        }
    }
    let scale = (h as i64 * h as i64) / (1080 * 1080); // cluster size grows with resolution
    clusters
        .into_iter()
        .filter(|c| (40 * scale.max(1)..=320 * scale.max(1)).contains(&c.2))
        .map(|c| (frame.left + (c.0 / c.2) as i32, frame.top + (c.1 / c.2) as i32))
        .collect()
}

type Request = (Vec<Region>, oneshot::Sender<Option<Screen>>);

/// The OCR engine lives on one thread (WinRT objects and blocking waits stay off the async runtime).
fn worker() -> &'static mpsc::Sender<Request> {
    static WORKER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Request>();
        thread::spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let engine = OcrEngine::TryCreateFromUserProfileLanguages().ok();
            for (regions, reply) in rx {
                let result = engine.as_ref().and_then(|engine| {
                    let frame = capture_foreground()?;
                    Some(Screen {
                        regions: regions.iter().map(|&region| read_region(engine, &frame, region)).collect(),
                        can_continue: continue_marker(&frame),
                        red_badges: red_badges(&frame),
                        artifact_stats_zero: artifact_stats_zero(&frame),
                    })
                });
                let _ = reply.send(result);
            }
        });
        tx
    })
}

/// Grabs the foreground window once and reads each region. None if the screen or OCR isn't available.
pub async fn read(regions: &[Region]) -> Option<Screen> {
    let (tx, rx) = oneshot::channel();
    worker().send((regions.to_vec(), tx)).ok()?;
    rx.await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual check: reads the whole foreground window. `cargo test --lib reads_screen -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn reads_screen() {
        let screen = read(&[(0.0, 0.0, 1.0, 1.0)]).await.expect("capture/OCR failed");
        for line in &screen.regions[0] {
            println!("{:?} {}", line.center, line.text);
        }
        assert!(!screen.regions[0].is_empty());
    }

    /// Manual check on a screenshot saved as raw BGRA (8-byte header: width, height as u32 LE).
    /// `STORY_FRAME=shot.bgra cargo test --lib reads_frame_file -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_frame_file() {
        let data = std::fs::read(std::env::var("STORY_FRAME").unwrap()).unwrap();
        let width = u32::from_le_bytes(data[0..4].try_into().unwrap()) as i32;
        let height = u32::from_le_bytes(data[4..8].try_into().unwrap()) as i32;
        let frame = Frame { left: 0, top: 0, width, height, pixels: data[8..].to_vec() };
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().unwrap();
        println!("--- can_continue: {}", continue_marker(&frame));
        println!("--- red badges: {:?}", red_badges(&frame));
        println!("--- artifact stats zero: {:?}", artifact_stats_zero(&frame));
        for (name, region) in [("dialogue", crate::story::DIALOGUE), ("choices", crate::story::CHOICES), ("chat hint", crate::story::CHAT_HINT)] {
            println!("--- {name}");
            for line in read_region(&engine, &frame, region) {
                println!("{:?} left={:.3} icon={} gold={} {}", line.center, line.left, line.icon, line.gold, line.text);
            }
        }
    }

    /// A dark 1920x1080 frame; `paint` sets chosen pixels to the diamond's gold.
    fn frame_with(paint: impl Fn(i32, i32) -> bool) -> Frame {
        let (width, height) = (1920, 1080);
        let mut pixels = vec![30u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                if paint(x, y) {
                    let i = ((y * width + x) * 4) as usize;
                    pixels[i..i + 3].copy_from_slice(&[0x28, 0x80, 0xA6]); // BGR of #a68028
                }
            }
        }
        Frame { left: 0, top: 0, width, height, pixels }
    }

    #[test]
    fn finds_the_continue_diamond_by_shape() {
        let diamond = frame_with(|x, y| ((x - 960).abs() + (y - 1043).abs() - 12).abs() <= 1);
        assert!(continue_marker(&diamond));
        // Lots of gold, but no diamond outline (a patterned carpet).
        let carpet = frame_with(|x, y| y > 990 && (x * 7 + y * 13) % 5 < 2);
        assert!(!continue_marker(&carpet));
        assert!(!continue_marker(&frame_with(|_, _| false)));
    }

    #[test]
    fn icon_words() {
        assert!(is_icon_word("o"));
        assert!(is_icon_word("@"));
        assert!(is_icon_word(".."));
        assert!(is_icon_word("Q"));
        assert!(!is_icon_word("I"));
        assert!(!is_icon_word("A"));
        assert!(!is_icon_word("About"));
        assert!(!is_icon_word("I'm"));
    }
}

/// Test helper: reads regions from a screenshot saved as raw BGRA (8-byte header: width, height as
/// u32 LE), the same way a live read would.
#[cfg(test)]
pub(crate) fn screen_from_file(path: &str, regions: &[Region]) -> Screen {
    let data = std::fs::read(path).unwrap();
    let width = u32::from_le_bytes(data[0..4].try_into().unwrap()) as i32;
    let height = u32::from_le_bytes(data[4..8].try_into().unwrap()) as i32;
    let frame = Frame { left: 0, top: 0, width, height, pixels: data[8..].to_vec() };
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().unwrap();
    Screen {
        regions: regions.iter().map(|&r| read_region(&engine, &frame, r)).collect(),
        can_continue: continue_marker(&frame),
        red_badges: red_badges(&frame),
        artifact_stats_zero: artifact_stats_zero(&frame),
    }
}
