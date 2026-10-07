//! Artifact Remover for every character (Shift+F7). It runs unattended, so unlike F7 it checks the
//! screen before every click that matters:
//! - which screen is open (character screen vs artifact screen), so "Back" is never clicked on the
//!   character screen, where that spot is the close button;
//! - whether the character has anything equipped (Max HP / ATK / DEF / EM all "+0" means nothing);
//! - where the slot bubbles are (they move with each character's model: empty slots show a red "!"
//!   at the top right of their bubble, and F7's fixed click path and a coarse grid are fallbacks);
//! - that the bottom-right button says Remove (on an empty slot it says Equip, which would take an
//!   artifact from another character);
//! - when it's back at a character it has already done (the name at the top left).

use crate::macros::{click_at, ARTIFACT_PATH, ARTIFACT_TABS, BACK_BUTTON, NEXT_CHARACTER, REMOVE_BUTTON};
use crate::story::ocr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::time::sleep;

/// "Cryo / Wriothesley" at the top left of the character screen.
const NAME_AREA: ocr::Region = (0.06, 0.02, 0.30, 0.075);
/// Max HP / ATK / DEF / Elemental Mastery bonuses from artifacts, top right of the character screen.
const STATS_AREA: ocr::Region = (0.76, 0.11, 0.95, 0.25);
/// "Artifact Details" button: only on the character screen's Artifacts page.
const DETAILS_AREA: ocr::Region = (0.75, 0.26, 0.95, 0.32);
/// Both bottom-right buttons: "Fast Equip / Switch" on the character screen, "Equip or Remove /
/// Reshape" on the artifact screen.
const BOTTOM_AREA: ocr::Region = (0.74, 0.89, 0.98, 0.98);
/// The bottom-right button on the artifact screen (Remove / Equip).
const BUTTON_AREA: ocr::Region = (0.74, 0.89, 0.88, 0.98);
/// "Artifacts" in the character screen's left menu.
const ARTIFACTS_MENU: (i32, i32) = (241, 293);
/// From a slot's red "!" badge to the middle of its bubble (1080p).
const BADGE_TO_BUBBLE: (i32, i32) = (-55, 58);
/// Safety cap on the roster size.
const MAX_CHARACTERS: usize = 150;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Fixed waits as a percentage of normal, from the speed slider (100 = slowest, 25 = fastest).
static SPEED: AtomicU32 = AtomicU32::new(100);

/// Sets the speed from the slider: 1 (normal waits) to 10 (a quarter of them).
#[tauri::command]
pub fn set_artifact_speed(level: u32) {
    let level = level.clamp(1, 10);
    SPEED.store(100 - (level - 1) * 75 / 9, Ordering::Relaxed);
}

/// A fixed wait, shortened by the Speed setting (never below 60 ms, the game needs a frame or two).
async fn pause(ms: u64) {
    let scaled = ms * SPEED.load(Ordering::Relaxed) as u64 / 100;
    sleep(Duration::from_millis(scaled.max(60))).await;
}
static CANCEL: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app(app: AppHandle) {
    let _ = APP.set(app);
}

pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Stops a run after the click in progress (F7 or Shift+F7 again).
pub fn cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

fn status(text: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit("artifact-status", text);
    }
}

fn stopped() -> bool {
    CANCEL.load(Ordering::Relaxed) || !crate::hook::is_genshin_active()
}

fn region_text(screen: &ocr::Screen, i: usize) -> String {
    screen.regions[i].iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase()
}

async fn text_in(region: ocr::Region) -> String {
    match ocr::read(&[region]).await {
        Some(screen) => region_text(&screen, 0),
        None => String::new(),
    }
}

/// Menus animate in, so a check that fails is retried for up to 3 s before giving up. This waits
/// for the game, so it doesn't follow the Speed setting.
async fn eventually<F, Fut>(check: F) -> bool
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for attempt in 0..15 {
        if check().await {
            return true;
        }
        if attempt < 14 {
            sleep(Duration::from_millis(200)).await;
        }
    }
    false
}

async fn on_character_screen() -> bool {
    eventually(|| async { text_in(DETAILS_AREA).await.contains("artifact") }).await
}

/// The artifact screen's bottom buttons: "Equip" or "Remove", and "Reshape" or "Enhance" (an
/// artifact below +20). The character screen has "Fast Equip" and "Switch" there instead.
fn is_artifact_screen(bottom: &str) -> bool {
    !bottom.contains("fast")
        && !bottom.contains("switch")
        && ["reshape", "enhance", "remove", "equip"].iter().any(|w| bottom.contains(w))
}

async fn on_artifact_screen() -> bool {
    eventually(|| async {
        let Some(screen) = ocr::read(&[BOTTOM_AREA, BUTTON_AREA]).await else { return false };
        is_artifact_screen(&format!("{} {}", region_text(&screen, 0), region_text(&screen, 1)))
    })
    .await
}

/// Nothing equipped: none of the Max HP / ATK / DEF / EM bonuses reads above 0, and either all four
/// read "+0" or all five slots show the red "!" of an empty slot (the reader sometimes skips a value).
fn nothing_equipped(stats: &str, badges: usize, stats_zero: Option<bool>) -> bool {
    match stats_zero {
        Some(true) => return true,  // all four values are "+0" (pixel check)
        Some(false) => return false, // a real value is showing
        None => {}
    }
    let values: Vec<String> = stats
        .split('+')
        .skip(1)
        .map(|v| v.chars().take_while(|c| !c.is_whitespace()).collect::<String>().replace(['o', 'O'], "0"))
        .map(|v| v.trim_end_matches(['.', ',']).to_string())
        .collect();
    if values.iter().any(|v| v != "0") {
        return false;
    }
    values.len() >= 4 || badges >= 5
}

/// Where to click to open the artifact screen: bubbles found from their badges first, then F7's
/// fixed path, then a coarse grid over the area where bubbles can be (nothing else to hit there).
fn bubble_guesses(badges: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut points: Vec<(i32, i32)> =
        badges.iter().map(|&(x, y)| (x + BADGE_TO_BUBBLE.0, y + BADGE_TO_BUBBLE.1)).collect();
    points.extend_from_slice(ARTIFACT_PATH);
    for y in (480..=900).step_by(140) {
        for x in (450..=1400).step_by(150) {
            points.push((x, y));
        }
    }
    points
}

/// Opens the artifact screen from the character screen. False if no click opened it.
/// The slot bubbles slowly orbit the character, so their badges are looked up again right before
/// each click; the fixed spots and the grid come after.
async fn open_artifacts() -> bool {
    let mut tried_badges = 0;
    let mut fallback = bubble_guesses(&[]).into_iter();
    loop {
        if stopped() {
            return false;
        }
        let target = if tried_badges < 5 {
            let badges = ocr::read(&[]).await.map(|s| s.red_badges).unwrap_or_default();
            tried_badges += 1;
            badges.get(tried_badges - 1).map(|&(x, y)| (x + BADGE_TO_BUBBLE.0, y + BADGE_TO_BUBBLE.1))
        } else {
            None
        };
        let (x, y) = match target {
            Some(point) => point,
            None => {
                tried_badges = 5;
                match fallback.next() {
                    Some(point) => point,
                    None => return false,
                }
            }
        };
        click_at(x, y);
        pause(800).await;
        // A miss leaves the character screen as it was: try the next spot straight away. Anything
        // else is the artifact screen still opening, or something unexpected.
        if text_in(DETAILS_AREA).await.contains("artifact") {
            continue;
        }
        if on_artifact_screen().await {
            return true;
        }
        return false; // something else opened; don't keep clicking blind
    }
}

/// On the artifact screen: each slot tab, Remove only where the button says Remove.
async fn remove_open_slots() -> usize {
    let mut removed = 0;
    for &(x, y) in ARTIFACT_TABS {
        if stopped() {
            break;
        }
        click_at(x, y);
        pause(450).await;
        let button = text_in(BUTTON_AREA).await;
        if button.contains("remove") || button.contains("unequip") {
            click_at(REMOVE_BUTTON.0, REMOVE_BUTTON.1);
            removed += 1;
            pause(350).await;
        }
    }
    removed
}

/// Removes artifacts from every character, starting at the one on screen.
pub async fn remove_all() {
    if RUNNING.swap(true, Ordering::Relaxed) {
        return;
    }
    CANCEL.store(false, Ordering::Relaxed);
    status("Removing artifacts from every character… (F7 to stop)");

    let mut seen: Vec<String> = Vec::new();
    let (mut characters, mut artifacts, mut skipped) = (0, 0, Vec::<String>::new());
    let outcome: String = 'run: loop {
        if CANCEL.load(Ordering::Relaxed) {
            break "stopped".into();
        }
        if !crate::hook::is_genshin_active() {
            break "stopped: Genshin isn't in front".into();
        }
        // Make sure the Artifacts page of the character screen is showing.
        if !on_character_screen().await {
            click_at(ARTIFACTS_MENU.0, ARTIFACTS_MENU.1);
            pause(700).await;
            if !on_character_screen().await {
                break "stopped: open a character's Artifacts page first".into();
            }
        }
        let Some(screen) = ocr::read(&[NAME_AREA, STATS_AREA]).await else {
            break "stopped: couldn't read the screen".into();
        };
        let name = region_text(&screen, 0);
        let signature: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
        if signature.is_empty() {
            break "stopped: couldn't read the character's name".into();
        }
        if seen.contains(&signature) {
            break "done".into();
        }
        seen.push(signature);
        let who = name.rsplit('/').next().unwrap_or(&name).trim().to_string();

        // Decide by majority over three looks: the numbers fade in after switching characters and
        // sparkles drift across the background, but a real value stays put.
        let (mut zero_votes, mut value_votes) = (0, 0);
        let mut text_says_empty = nothing_equipped(&region_text(&screen, 1), screen.red_badges.len(), None);
        let mut look = Some(screen.artifact_stats_zero);
        for i in 0..3 {
            match look {
                Some(Some(true)) => zero_votes += 1,
                Some(Some(false)) => value_votes += 1,
                _ => {}
            }
            if i == 2 || zero_votes >= 2 || value_votes >= 2 {
                break;
            }
            pause(300).await;
            look = match ocr::read(&[STATS_AREA]).await {
                Some(again) => {
                    text_says_empty |= nothing_equipped(&region_text(&again, 0), again.red_badges.len(), None);
                    Some(again.artifact_stats_zero)
                }
                None => None,
            };
        }
        let empty = if zero_votes + value_votes > 0 { zero_votes > value_votes } else { text_says_empty };
        if !empty {
            if open_artifacts().await {
                artifacts += remove_open_slots().await;
                // Wait for the last Remove to finish before Back, and press Back again if the game
                // ignored it (it can, mid-animation). Never pressed unless the artifact screen is up,
                // since that spot closes the character screen.
                pause(300).await;
                let mut back = false;
                for _ in 0..3 {
                    click_at(BACK_BUTTON.0, BACK_BUTTON.1);
                    pause(300).await;
                    if on_character_screen().await {
                        back = true;
                        break;
                    }
                    if !on_artifact_screen().await {
                        break;
                    }
                }
                if !back {
                    break 'run "stopped: didn't get back to the character screen".into();
                }
                // The character page looks ready before its arrows respond; the game's own load
                // time, so the speed slider doesn't shorten it.
                sleep(Duration::from_millis(600)).await;
            } else if stopped() {
                continue;
            } else if on_character_screen().await {
                skipped.push(who.clone());
            } else {
                break "stopped: an unexpected screen opened".into();
            }
        }
        characters += 1;
        status(&format!("Removing artifacts… {characters} characters, {artifacts} artifacts (F7 to stop)"));
        if characters >= MAX_CHARACTERS {
            break "done".into();
        }
        // Next character; if the game ignored the click (the name hasn't changed after ~2.5 s),
        // press it again. The wait is for the game, so it doesn't follow the speed slider.
        let previous = seen.last().cloned().unwrap_or_default();
        let mut moved = false;
        'next: for _ in 0..3 {
            if stopped() {
                break;
            }
            click_at(NEXT_CHARACTER.0, NEXT_CHARACTER.1);
            for _ in 0..12 {
                sleep(Duration::from_millis(200)).await;
                let now: String = text_in(NAME_AREA).await.chars().filter(|c| c.is_alphanumeric()).collect();
                if !now.is_empty() && now != previous {
                    moved = true;
                    break 'next;
                }
            }
        }
        if !moved && !stopped() {
            break "stopped: the Next character button didn't respond".into();
        }
        pause(250).await; // let the stats and slots fade in
    };

    let mut message = format!("Artifact Remover {outcome}: {artifacts} artifacts from {characters} characters");
    if !skipped.is_empty() {
        message.push_str(&format!(" (couldn't open: {})", skipped.join(", ")));
    }
    status(&message);
    RUNNING.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_empty_artifact_stats() {
        assert!(nothing_equipped("max hp +0 atk +0 def +0 elemental mastery +0", 0, None));
        assert!(nothing_equipped("max hp +o atk +0 def +O elemental mastery +0", 0, None));
        assert!(!nothing_equipped("max hp +4,780 atk +33 def +0 elemental mastery +21", 5, None));
        assert!(!nothing_equipped("max hp +0 atk +0", 2, None), "too little read to be sure");
        // Chiori: one "+0" skipped by the reader, but all five slots show the empty-slot badge.
        assert!(nothing_equipped("max hp atk def elemental mastery +0 +0 +0", 5, None));
        // The pixel check wins over the text reader either way.
        assert!(nothing_equipped("max hp atk +0", 1, Some(true)));
        assert!(!nothing_equipped("max hp +0 atk +0 def +0 elemental mastery +0", 5, Some(false)));
    }

    /// Manual check on screenshots: `ART_FRAME=shot.bgra cargo test --lib reads_artifact_screens -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_artifact_screens() {
        let path = std::env::var("ART_FRAME").unwrap();
        let areas = [NAME_AREA, STATS_AREA, DETAILS_AREA, BOTTOM_AREA, BUTTON_AREA];
        let screen = ocr::screen_from_file(&path, &areas);
        for (name, i) in [("name", 0), ("stats", 1), ("details", 2), ("bottom", 3), ("button", 4)] {
            println!("{name:8} {:?}", region_text(&screen, i));
        }
        println!("nothing equipped: {}", nothing_equipped(&region_text(&screen, 1), screen.red_badges.len(), screen.artifact_stats_zero));
        println!("artifact screen: {}", is_artifact_screen(&region_text(&screen, 3)));
        println!("badges: {:?}", screen.red_badges);
        println!("stats zero (pixels): {:?}", screen.artifact_stats_zero);
    }

    #[test]
    fn tells_the_two_screens_apart() {
        assert!(!is_artifact_screen("fast equip switch"));
        assert!(is_artifact_screen("equip reshape"));
        assert!(is_artifact_screen("remove reshape"));
        assert!(is_artifact_screen("enhance equip"), "artifact below +20");
        assert!(!is_artifact_screen(""));
    }

    #[test]
    fn bubbles_from_badges_come_first() {
        let points = bubble_guesses(&[(576, 527)]);
        assert_eq!(points[0], (521, 585));
        assert_eq!(points[1], ARTIFACT_PATH[0]);
    }
}
