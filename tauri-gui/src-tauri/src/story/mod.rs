//! Story Mode, a separate take on Auto Dialogue: while it skips through dialogue, the subtitle text is read off the
//! screen (Windows OCR), dialogue choices are picked by an AI instead of always taking the first one,
//! and when the run stops (F4) the AI writes a summary of everything that was said.
//!
//! Layout assumes Genshin's 16:9 dialogue screen; regions are fractions of the game window.

pub mod ai;
pub mod history;
pub mod ocr;

use crate::voice::VoiceHandle;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_F, VK_SPACE};

/// Speaker name + subtitle lines at the bottom centre.
pub(crate) const DIALOGUE: ocr::Region = (0.15, 0.72, 0.85, 0.93);
/// Bottom-left corner: the open world shows the chat hint ("Enter") there, conversations don't.
pub(crate) const CHAT_HINT: ocr::Region = (0.0, 0.93, 0.12, 1.0);
/// Characters in a line the delay setting is meant for; longer lines wait longer.
const AVERAGE_LINE: f32 = 60.0;

/// How long to wait after a line: the set delay for an average line, scaled to this one's length
/// (between 0.4x and 3x).
fn line_delay(base_ms: u64, line: &str, match_length: bool) -> u64 {
    if !match_length {
        return base_ms;
    }
    let chars = normalized(line).chars().count() as f32;
    (base_ms as f32 * (chars / AVERAGE_LINE).clamp(0.4, 3.0)) as u64
}

/// How often to look again while a line is still typing out (no diamond yet).
const POLL_MS: u64 = 300;

/// Whether to press Space: the line is finished (diamond showing) and this isn't open-world chatter.
fn advance(can_continue: bool, open_world: bool) -> bool {
    can_continue && !open_world
}

/// Bottom-right corner, where menus put their action button (Purchase, Confirm, Craft...).
const MENU_BUTTON: ocr::Region = (0.70, 0.86, 1.0, 1.0);
/// Words that mean a menu that spends something is open: Story Mode then presses nothing.
const RISKY_WORDS: &[&str] = &[
    "purchase", "buy", "exchange", "confirm", "craft", "convert", "sell", "redeem", "synthesize", "wish",
    "obtain", "forge", "enhance", "upgrade", "refine",
];
/// Dialogue options that lead into a shop or a trade; never picked.
const RISKY_OPTIONS: &[&str] = &[
    "buy", "purchase", "shop", "trade", "exchange", "craft", "sell", "wares", "goods", "forge", "synthes",
    "convert", "what do you have", "what have you got", "let me see",
];

fn mentions(text: &str, words: &[&str]) -> bool {
    let text = text.to_lowercase();
    words.iter().any(|w| text.contains(w))
}

/// Dialogue options: a column right of centre (the party list further right is left out).
pub(crate) const CHOICES: ocr::Region = (0.55, 0.22, 0.85, 0.80);
/// Letters/digits needed in the subtitle area before it counts as a conversation (stray HUD text is shorter).
const MIN_SUBTITLE_CHARS: usize = 8;
/// Transcript lines sent along when asking for a choice.
const CHOICE_CONTEXT: usize = 40;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct StorySettings {
    /// Chat provider id from `ai::PROVIDERS`; its key comes from the API Keys tab.
    pub provider: String,
    /// Empty = the provider's default model.
    pub model: String,
    /// Scale the delay to each line's length (the delay setting is for an average line).
    pub match_length: bool,
    /// History session that new runs are added to (playing one story over several sittings).
    pub continue_from: Option<u64>,
    /// Summary length: "short", "detailed" or "full" (part by part, the most detail).
    pub detail: String,
}

/// The run in progress, written to disk as it's read so a crash, closed app or lost connection
/// never loses it. Recovered into the history on the next start.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedRun {
    started: Option<u64>,
    transcript: Vec<String>,
    continue_from: Option<u64>,
}

#[derive(Default)]
struct Run {
    /// Subtitle blocks in order, plus "> Chose: ..." entries.
    transcript: Vec<String>,
    /// Choices picked in the current choice menu, so a repeating menu (side questions) moves on.
    picked: Vec<String>,
    /// When the first line of this run was read (ms since the Unix epoch).
    started: Option<u64>,
    /// Talk prompts already opened with F this run, so a finished chat isn't started again.
    talked: Vec<String>,
    /// A spending menu was on screen at the last check.
    paused: bool,
    /// Transcript of the session being continued, to skip lines already read there (a replayed
    /// scene after the game went back to a save point), and which session it is.
    earlier: Vec<String>,
    earlier_id: Option<u64>,
    /// Last line skipped as already read, so the status says so once.
    last_repeat: String,
}

#[derive(Default)]
pub struct StoryState {
    settings: Mutex<StorySettings>,
    run: Mutex<Run>,
    /// Saved sessions, oldest first; persisted to `history_path`.
    history: Mutex<Vec<history::Session>>,
    history_path: Mutex<Option<std::path::PathBuf>>,
}

pub type StoryHandle = Arc<StoryState>;

#[derive(Serialize, Clone)]
struct StoryEvent<'a> {
    /// "reading" | "choosing" | "summarizing" | "done" | "error" | "idle"
    state: &'a str,
    text: &'a str,
    lines: usize,
}

fn emit(app: &AppHandle, state: &str, text: &str, lines: usize) {
    let _ = app.emit("story", StoryEvent { state, text, lines });
}

impl StoryState {
    pub fn load_history(&self, path: std::path::PathBuf) {
        let (sessions, tidied) = history::load(&path);
        *self.history.lock().unwrap() = sessions;
        *self.history_path.lock().unwrap() = Some(path.clone());
        if tidied {
            // Keep the file as it was before tidying, just in case.
            let backup = path.with_file_name("story_history.before-tidy.json");
            if !backup.exists() {
                let _ = std::fs::copy(&path, &backup);
            }
            self.save_history();
        }
        self.recover();
    }

    fn run_path(&self) -> Option<std::path::PathBuf> {
        self.history_path.lock().unwrap().as_ref().map(|p| p.with_file_name("story_current.json"))
    }

    /// Writes the run in progress to disk (temp file + rename, so a crash mid-write can't corrupt it).
    fn save_run(&self) {
        let Some(path) = self.run_path() else { return };
        let saved = {
            let run = self.run.lock().unwrap();
            SavedRun {
                started: run.started,
                transcript: run.transcript.clone(),
                continue_from: self.settings.lock().unwrap().continue_from,
            }
        };
        let tmp = path.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(&saved) {
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    fn clear_saved_run(&self) {
        if let Some(path) = self.run_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    /// A run left on disk by a crash or closed app goes into the history (summarized later).
    fn recover(&self) {
        let Some(path) = self.run_path() else { return };
        let Some(saved) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<SavedRun>(&s).ok())
        else {
            return;
        };
        if dialogue_count(&saved.transcript) >= 3 {
            self.store_run(saved.transcript, saved.started, saved.continue_from);
        }
        self.clear_saved_run();
    }

    /// Adds a finished run to the history: onto the continued session if there is one (marking
    /// where each sitting starts), otherwise as a new session. Returns the session's id.
    fn store_run(&self, transcript: Vec<String>, started: Option<u64>, continue_from: Option<u64>) -> u64 {
        let id = {
            let mut history = self.history.lock().unwrap();
            match continue_from.and_then(|id| history.iter_mut().find(|s| s.id == id)) {
                Some(session) => {
                    if !session.transcript.first().is_some_and(|l| l.starts_with("--- ")) {
                        session.transcript.insert(0, "--- Session 1 ---".into());
                    }
                    session.parts = session.parts.max(1) + 1;
                    session.transcript.push(format!("--- Session {} ---", session.parts));
                    session.transcript.extend(transcript);
                    session.summary.clear(); // re-summarized with the new part
                    session.id
                }
                None => {
                    let id = started.unwrap_or_else(now_ms);
                    history.push(history::Session { id, transcript, parts: 1, ..Default::default() });
                    history.sort_by_key(|s| s.id);
                    id
                }
            }
        };
        self.save_history();
        id
    }

    pub fn history(&self) -> Vec<history::Session> {
        self.history.lock().unwrap().clone()
    }

    /// Adds sessions from a backup, skipping ones already here.
    pub fn import_history(&self, app: &AppHandle, sessions: Vec<history::Session>) {
        {
            let mut history = self.history.lock().unwrap();
            for session in sessions {
                if !history.iter().any(|s| s.id == session.id) {
                    history.push(session);
                }
            }
            history.sort_by_key(|s| s.id);
        }
        self.save_history();
        emit_history(app, self);
    }

    fn save_history(&self) {
        if let Some(path) = self.history_path.lock().unwrap().as_ref() {
            if let Err(e) = history::save(path, &self.history.lock().unwrap()) {
                eprintln!("story: couldn't save history: {e}");
            }
        }
    }

    pub fn set_settings(&self, settings: StorySettings) {
        *self.settings.lock().unwrap() = settings;
    }
}

/// Lowercase letters and digits only, for comparing OCR text that flickers between frames.
fn normalized(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// True when two reads are the same subtitle (one may still be typing out, or have an OCR slip).
fn same_block(a: &str, b: &str) -> bool {
    let (a, b) = (normalized(a), normalized(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.starts_with(&b) || b.starts_with(&a) {
        return true;
    }
    let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    common * 10 >= a.chars().count().min(b.chars().count()) * 8
}

/// Turns the subtitle area into one transcript line, "Speaker: what they said". The speaker's name is
/// the first gold line (a gold title under it, like "Boss, Cat's Tail", is left out); the white
/// lines are what was said. Options reaching into the area are skipped. Empty while only a name shows.
fn subtitle(lines: &[ocr::Line]) -> String {
    let lines: Vec<&ocr::Line> = lines.iter().filter(|l| !l.icon).collect();
    let speaker = lines.iter().find(|l| l.gold).map(|l| l.text.trim().trim_end_matches(':').trim());
    let said = history::strip_marker_tail(
        &lines.iter().filter(|l| !l.gold).map(|l| l.text.trim()).collect::<Vec<_>>().join(" "),
    );
    match speaker {
        _ if said.is_empty() => String::new(),
        Some(name) if !name.is_empty() => format!("{name}: {said}"),
        _ => said,
    }
}

/// Letter pairs of a normalized line, sorted, for fuzzy comparison.
fn bigrams(norm: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = norm.chars().collect();
    let mut grams: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
    grams.sort_unstable();
    grams
}

/// The same line read twice: equal, or nearly so (OCR slips), judged by shared letter pairs.
pub(crate) fn near_equal(a: &str, b: &str) -> bool {
    let (a, b) = (normalized(a), normalized(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    if la.min(lb) * 10 < la.max(lb) * 8 {
        return false;
    }
    let (ga, gb) = (bigrams(&a), bigrams(&b));
    let (mut i, mut j, mut shared) = (0, 0, 0);
    while i < ga.len() && j < gb.len() {
        match ga[i].cmp(&gb[j]) {
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    shared * 2 * 100 >= (ga.len() + gb.len()) * 85
}

fn dialogue_count(transcript: &[String]) -> usize {
    transcript.iter().filter(|l| !l.starts_with("> ") && !l.starts_with("--- ")).count()
}

/// True when `line` was already read: in this run's earlier lines or in the continued session.
fn seen_before(run: &[String], earlier: &[String], line: &str) -> bool {
    run.iter().chain(earlier).any(|l| !l.starts_with("> ") && !l.starts_with("--- ") && near_equal(l, line))
}

#[derive(Debug, PartialEq)]
enum Reading {
    /// A new line was added.
    New,
    /// Same line as last time (or still typing out).
    Same,
    /// Already read earlier (a replayed scene): not added again.
    Repeat,
}

/// Adds a subtitle read to the transcript, merging it with the previous one while it types out, and
/// skipping lines already read in this run or in `earlier` (the session being continued).
fn record(transcript: &mut Vec<String>, block: String, earlier: &[String]) -> Reading {
    if normalized(&block).chars().count() < 3 {
        return Reading::Same;
    }
    if let Some(last) = transcript.last() {
        if !last.starts_with("> ") && same_block(last, &block) {
            let longer = block.len() > last.len();
            let n = transcript.len();
            if longer {
                transcript[n - 1] = block;
            }
            // A replayed line that started typing as "new" turns out to be one already read.
            if longer && seen_before(&transcript[..n - 1], earlier, &transcript[n - 1]) {
                transcript.pop();
                return Reading::Repeat;
            }
            return Reading::Same;
        }
    }
    if seen_before(transcript, earlier, &block) {
        return Reading::Repeat;
    }
    transcript.push(block);
    Reading::New
}

/// Keeps the lines that look like dialogue options: a speech-bubble icon on the left and a left
/// edge lined up with the other options. Sorted top to bottom.
fn option_lines(lines: Vec<ocr::Line>) -> Vec<ocr::Line> {
    let mut lines: Vec<ocr::Line> =
        lines.into_iter().filter(|l| l.icon && normalized(&l.text).chars().count() >= 2).collect();
    let anchor = lines.iter().map(|l| l.left).fold(None, |best: Option<(f32, usize)>, x| {
        let n = lines.iter().filter(|o| (o.left - x).abs() < 0.015).count();
        if best.is_none_or(|(_, m)| n > m) { Some((x, n)) } else { best }
    });
    let Some((anchor, _)) = anchor else { return Vec::new() };
    lines.retain(|l| (l.left - anchor).abs() < 0.015);
    lines.sort_by_key(|l| l.center.1);
    lines
}

/// Parses the model's answer ("2", "Option 2", ...) into a 0-based index.
fn parse_choice(answer: &str, count: usize) -> Option<usize> {
    let digits: String = answer.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    let n: usize = digits.parse().ok()?;
    (1..=count).contains(&n).then(|| n - 1)
}

async fn ask(app: &AppHandle, settings: &StorySettings, system: &str, user: &str) -> Result<String, String> {
    let voice = app.state::<VoiceHandle>();
    let config = voice.settings().cloud.get(&settings.provider).cloned().unwrap_or_default();
    ai::chat(&voice.client, &settings.provider, &config, &settings.model, system, user).await
}

async fn choose(app: &AppHandle, story: &StoryState, choices: &[ocr::Line]) -> usize {
    let settings = story.settings.lock().unwrap().clone();
    let (context, picked) = {
        let run = story.run.lock().unwrap();
        let start = run.transcript.len().saturating_sub(CHOICE_CONTEXT);
        (run.transcript[start..].join("\n"), run.picked.clone())
    };
    let options: Vec<String> =
        choices.iter().enumerate().map(|(i, c)| format!("{}. {}", i + 1, c.text)).collect();
    let system = "You choose dialogue options for the player in Genshin Impact story quests. The text was \
                  read from the screen with OCR, so expect small errors. Pick the option that best follows the \
                  story and reveals the most about it, prefers options not taken yet, and moves the conversation \
                  forward. Never pick options that open a shop, trade, crafting, or spend currency or items. Reply \
                  with the option number only.";
    let user = format!(
        "Recent dialogue:\n{context}\n\nAlready picked in this menu: {}\n\nOptions:\n{}",
        if picked.is_empty() { "none".to_string() } else { picked.join(" | ") },
        options.join("\n")
    );
    match ask(app, &settings, system, &user).await {
        Ok(answer) => parse_choice(&answer, choices.len()).unwrap_or(0),
        Err(e) => {
            emit(app, "error", &format!("{e} — picked the first option"), 0);
            0
        }
    }
}

/// One Auto Dialogue step with AI on: read the screen, pick a choice if a menu is open, otherwise skip
/// ahead like the plain mode.
/// Returns how long to wait before the next step, given the delay setting.
pub async fn step(app: &AppHandle, story: &StoryState, base_ms: u64) -> u64 {
    let Some(screen) = ocr::read(&[DIALOGUE, CHOICES, MENU_BUTTON, CHAT_HINT]).await else {
        return base_ms;
    };
    let can_continue = screen.can_continue;
    let Ok([dialogue, choices, menu, hud]) = <[_; 4]>::try_from(screen.regions) else {
        return base_ms;
    };
    let open_world = hud.iter().any(|l| l.text.to_lowercase().contains("enter"));
    // A shop, crafting or other spending menu is open: touch nothing until the player closes it.
    if menu.iter().any(|l| mentions(&l.text, RISKY_WORDS)) {
        let paused = {
            let mut run = story.run.lock().unwrap();
            !std::mem::replace(&mut run.paused, true)
        };
        if paused {
            emit(app, "error", "Paused: a shop or menu is open. Close it to continue.", 0);
        }
        return base_ms;
    }
    if std::mem::take(&mut story.run.lock().unwrap().paused) {
        emit(app, "reading", "Continuing", 0);
    }
    // The lowest options can reach into the subtitle area; they aren't part of what was said.
    let block = subtitle(&dialogue);
    // Only real dialogue is saved: a line with a (gold) speaker, or one the game is waiting on (the
    // continue diamond). Loading-screen tips show up in the same place but have neither.
    let worth_saving = can_continue || dialogue.iter().any(|l| l.gold && !l.icon);
    let choices = option_lines(choices);
    // Outside a conversation: never Space (it jumps). Press F only to start talking to someone new,
    // shown as a speech-bubble prompt; F interacts with the highlighted (top) prompt.
    if normalized(&block).chars().count() < MIN_SUBTITLE_CHARS {
        if let Some(prompt) = choices.first().filter(|p| !mentions(&p.text, RISKY_OPTIONS)) {
            let name = normalized(&prompt.text);
            let new = {
                let mut run = story.run.lock().unwrap();
                let new = !run.talked.contains(&name);
                if new {
                    run.talked.push(name);
                }
                new
            };
            if new {
                emit(app, "reading", &format!("Talking to {}", prompt.text), 0);
                crate::macros::press_key(VK_F);
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
        }
        return base_ms;
    }

    let continue_from = story.settings.lock().unwrap().continue_from;
    if story.run.lock().unwrap().earlier_id != continue_from {
        let earlier = continue_from
            .and_then(|id| story.history.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.transcript.clone()))
            .unwrap_or_default();
        let mut run = story.run.lock().unwrap();
        run.earlier = earlier;
        run.earlier_id = continue_from;
    }
    let (reading, lines, repeat_text) = {
        let mut run = story.run.lock().unwrap();
        let earlier = std::mem::take(&mut run.earlier);
        let reading = if worth_saving { record(&mut run.transcript, block.clone(), &earlier) } else { Reading::Same };
        run.earlier = earlier;
        if reading == Reading::New && run.started.is_none() {
            run.started = Some(now_ms());
        }
        if reading == Reading::New && choices.is_empty() {
            run.picked.clear(); // the menu is gone and the story moved on
        }
        let repeat_text = (reading == Reading::Repeat && run.last_repeat != normalized(&block)).then(|| {
            run.last_repeat = normalized(&block);
        });
        (reading, run.transcript.len(), repeat_text)
    };
    let added = reading == Reading::New;
    if added {
        story.save_run();
    }
    if repeat_text.is_some() {
        emit(app, "reading", "Already read — skipping", lines);
    }
    if added {
        emit(app, "reading", "", lines);
    }

    if choices.is_empty() {
        // Space only once the gold "continue" diamond shows the line is finished and waiting. Chatter
        // in the open world has no diamond (and the HUD is up), and there Space would jump.
        if !advance(can_continue, open_world) {
            return base_ms.min(POLL_MS);
        }
        crate::macros::press_key(VK_SPACE);
        return line_delay(base_ms, &block, story.settings.lock().unwrap().match_length);
    }

    // Options that open a shop or trade are never offered to the AI.
    let safe: Vec<ocr::Line> = choices.into_iter().filter(|c| !mentions(&c.text, RISKY_OPTIONS)).collect();
    if safe.is_empty() {
        emit(app, "error", "Paused: only shop/trade options here. Pick one yourself.", lines);
        return base_ms;
    }
    emit(app, "choosing", "", lines);
    let index = choose(app, story, &safe).await;
    let picked = &safe[index];
    {
        let mut run = story.run.lock().unwrap();
        run.transcript.push(format!("> Chose: {}", picked.text));
        run.picked.push(picked.text.clone());
    }
    story.save_run();
    crate::macros::click_at(picked.center.0, picked.center.1);
    emit(app, "reading", &format!("Chose: {}", picked.text), lines + 1);
    // Let the menu close before the next read.
    tokio::time::sleep(Duration::from_millis(600)).await;
    base_ms
}

/// What every summary prompt needs to know about the transcript.
const TRANSCRIPT_NOTES: &str = "The text is Genshin Impact story dialogue read from the screen with OCR: speaker \
    names may be glued to the start of lines and some words may be misread (fix obvious misreadings, keep names \
    as the game spells them). Lines starting with '> Chose:' are choices the player made; '--- Session N ---' \
    marks where the player stopped and later continued.";

const SHORT_PROMPT: &str = "Write a recap for a player who skipped through this dialogue. Start with one line \
    'Title: <3-6 word title>'. Then a short paragraph and a few bullet points: what happened, who was involved, \
    important reveals, and what the player chose. Under 300 words.";

const DETAILED_PROMPT: &str = "Write a detailed recap for a player who skipped through this dialogue and wants to \
    know everything that happened. Start with one line 'Title: <3-6 word title>'. Then use Markdown: an \
    'Overview' paragraph; then one '## ' section per scene or location in story order, telling what happens in \
    narrative prose with who is present, what each character says and wants (paraphrase important lines, quote \
    memorable ones), and how the scene ends; then '## Characters' (who is who, one line each), '## Lore & \
    reveals' and '## Your choices'. Don't leave out events, names or reveals. Length should follow the amount of \
    dialogue: several hundred to a few thousand words.";

const PART_PROMPT: &str = "This is one part of a longer story. Write very detailed notes on everything in this \
    part, in story order, as Markdown: one '### ' heading per scene or location, then narrative prose telling \
    what happens, who is present, what each character says and wants (paraphrase important lines, quote \
    memorable ones with the speaker), reveals and lore, and the player's choices with what they led to. Don't \
    summarize away details; a reader should know every story beat of this part without reading the dialogue. \
    No title line and no overview.";

const FINAL_PROMPT: &str = "Below are detailed notes on consecutive parts of one story. Write only the framing \
    for them, as Markdown: first one line 'Title: <3-6 word title for the whole story>', then an 'Overview' \
    paragraph of the whole story, then '## Characters' (everyone who appears, who they are and their role, one \
    line each), '## Lore & reveals' (every reveal and piece of lore, as bullets) and '## Your choices' (each \
    choice and what it led to). Don't repeat the part notes themselves.";

/// Transcript lines per part when writing a "full" (very detailed) summary.
const PART_LINES: usize = 120;

fn summary_prompt(base: &str) -> String {
    format!("{TRANSCRIPT_NOTES} {base}")
}

/// Splits a transcript into parts of about `PART_LINES` dialogue lines, cutting at a session break
/// when one is near.
fn parts(transcript: &[String]) -> Vec<&[String]> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < transcript.len() {
        let mut end = (start + PART_LINES).min(transcript.len());
        if end < transcript.len() {
            if let Some(cut) = (start + PART_LINES * 3 / 4..end).rev().find(|&i| transcript[i].starts_with("--- ")) {
                end = cut;
            }
        }
        out.push(&transcript[start..end]);
        start = end;
    }
    out
}

/// Asks the AI, waiting and retrying a few times when the provider rate-limits (long summaries make
/// several calls in a row).
async fn ask_patiently(app: &AppHandle, settings: &StorySettings, system: &str, user: &str) -> Result<String, String> {
    let mut attempt = 0;
    loop {
        match ask(app, settings, system, user).await {
            Err(e) if e.contains("429") && attempt < 3 => {
                attempt += 1;
                emit(app, "summarizing", &format!("Rate limited, waiting {}s…", 20 * attempt), 0);
                tokio::time::sleep(Duration::from_secs(20 * attempt as u64)).await;
            }
            other => return other,
        }
    }
}

/// Summarizes a transcript into (title, summary), as long as the "Summary" setting asks for.
async fn summarize(app: &AppHandle, story: &StoryState, transcript: &[String]) -> Result<(String, String), String> {
    let settings = story.settings.lock().unwrap().clone();
    let model = match settings.model.trim() {
        "" => ai::default_model(&settings.provider).to_string(),
        m => m.to_string(),
    };
    emit(app, "summarizing", &format!("Summarizing with {model}…"), 0);
    match settings.detail.as_str() {
        "short" => {
            let answer = ask_patiently(app, &settings, &summary_prompt(SHORT_PROMPT), &transcript.join("\n")).await?;
            Ok(history::split_title(&answer))
        }
        "detailed" => {
            let answer = ask_patiently(app, &settings, &summary_prompt(DETAILED_PROMPT), &transcript.join("\n")).await?;
            Ok(history::split_title(&answer))
        }
        // "full": a detailed write-up of every part, then a frame (title, overview, characters, lore,
        // choices) around them. Keeps every detail and each request small enough for free tiers.
        _ => {
            let parts = parts(transcript);
            if parts.len() <= 1 {
                let answer = ask_patiently(app, &settings, &summary_prompt(DETAILED_PROMPT), &transcript.join("\n")).await?;
                return Ok(history::split_title(&answer));
            }
            let mut notes: Vec<String> = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                emit(app, "summarizing", &format!("Summarizing part {} of {} with {model}…", i + 1, parts.len()), 0);
                let before = notes.last().map(|n| {
                    let tail: String = n.chars().rev().take(800).collect::<Vec<_>>().into_iter().rev().collect();
                    format!("End of the notes for the previous part (for context only):\n{tail}\n\n")
                });
                let user = format!("{}Part {} of {}:\n{}", before.unwrap_or_default(), i + 1, parts.len(), part.join("\n"));
                notes.push(ask_patiently(app, &settings, &summary_prompt(PART_PROMPT), &user).await?);
            }
            emit(app, "summarizing", "Writing the overview…", 0);
            let all_notes = notes
                .iter()
                .enumerate()
                .map(|(i, n)| format!("Part {}:\n{n}", i + 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            let frame = ask_patiently(app, &settings, &summary_prompt(FINAL_PROMPT), &all_notes).await?;
            let (title, frame) = history::split_title(&frame);
            // Overview first, then every part's notes, then characters / lore / choices.
            let (overview, extras) = match frame.find("\n## ") {
                Some(i) => (frame[..i].trim().to_string(), frame[i..].trim().to_string()),
                None => (frame.trim().to_string(), String::new()),
            };
            let mut summary = overview;
            for (i, n) in notes.iter().enumerate() {
                summary.push_str(&format!("\n\n## Part {}\n\n{}", i + 1, n.trim()));
            }
            if !extras.is_empty() {
                summary.push_str("\n\n");
                summary.push_str(&extras);
            }
            Ok((title, summary))
        }
    }
}

fn emit_history(app: &AppHandle, story: &StoryState) {
    let _ = app.emit("story-history", history::views(&story.history.lock().unwrap()));
}

/// Called when a run stops: saves it to the history and summarizes it. Clears the run for the next one.
pub async fn finish(app: AppHandle, story: StoryHandle) {
    let (transcript, started) = {
        let mut run = story.run.lock().unwrap();
        run.picked.clear();
        run.talked.clear();
        run.last_repeat.clear();
        run.earlier_id = None; // reload: the continued session is about to grow
        (std::mem::take(&mut run.transcript), std::mem::take(&mut run.started))
    };
    if dialogue_count(&transcript) < 3 {
        story.clear_saved_run();
        emit(&app, "idle", "", 0);
        return;
    }
    // Saved before any network call, so a failed summary never loses what was read.
    let continue_from = story.settings.lock().unwrap().continue_from;
    let id = story.store_run(transcript, started, continue_from);
    story.clear_saved_run();
    emit_history(&app, &story);
    match summarize_session(&app, &story, id).await {
        Ok(()) => {
            emit(&app, "done", "", 0);
            // Back online: catch up on runs that couldn't be summarized before.
            let _ = retry_pending(&app, &story).await;
        }
        Err(e) => emit(&app, "error", &format!("{e} — saved; it'll be summarized when the AI is reachable"), 0),
    }
}

/// Summarizes one history session (whole transcript) and saves the result.
async fn summarize_session(app: &AppHandle, story: &StoryState, id: u64) -> Result<(), String> {
    let transcript = story.history.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.transcript.clone());
    let Some(transcript) = transcript else { return Ok(()) };
    emit(app, "summarizing", "", transcript.len());
    let (title, summary) = summarize(app, story, &transcript).await?;
    if let Some(session) = story.history.lock().unwrap().iter_mut().find(|s| s.id == id) {
        if !title.is_empty() {
            session.title = title;
        }
        session.summary = summary;
    }
    story.save_history();
    emit_history(app, story);
    Ok(())
}

/// Summarizes every session still waiting for one (saved offline or recovered after a crash),
/// oldest first. Stops at the first failure (still offline).
async fn retry_pending(app: &AppHandle, story: &StoryState) -> Result<(), String> {
    let pending: Vec<u64> =
        story.history.lock().unwrap().iter().filter(|s| s.summary.is_empty()).map(|s| s.id).collect();
    for id in pending {
        summarize_session(app, story, id).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn story_retry_pending(app: AppHandle, story: tauri::State<'_, StoryHandle>) -> Result<(), String> {
    let story: StoryHandle = story.inner().clone();
    let result = retry_pending(&app, &story).await;
    emit(&app, "idle", "", 0);
    result
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[tauri::command]
pub fn story_history(story: tauri::State<'_, StoryHandle>) -> Vec<history::SessionView> {
    history::views(&story.history.lock().unwrap())
}

/// Everything read in a session, line by line ("Speaker: text", "> Chose: ...", "--- Session N ---").
#[tauri::command]
pub fn story_transcript(id: u64, story: tauri::State<'_, StoryHandle>) -> Vec<String> {
    story.history.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.transcript.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn story_delete(id: u64, app: AppHandle, story: tauri::State<'_, StoryHandle>) {
    story.history.lock().unwrap().retain(|s| s.id != id);
    story.save_history();
    emit_history(&app, &story);
}

/// Merges sessions into one (in play order) with a fresh summary; the originals are replaced.
/// With a single id it just summarizes that session again (e.g. after a failed summary).
#[tauri::command]
pub async fn story_merge(ids: Vec<u64>, app: AppHandle, story: tauri::State<'_, StoryHandle>) -> Result<(), String> {
    let (transcript, parts, first_id) = {
        let history = story.history.lock().unwrap();
        let picked: Vec<&history::Session> = history.iter().filter(|s| ids.contains(&s.id)).collect();
        if picked.is_empty() {
            return Err("Those sessions are gone".into());
        }
        let parts = picked.iter().map(|s| s.parts.max(1)).sum::<usize>();
        let first = picked.iter().map(|s| s.id).min().unwrap_or_default();
        let transcript = if picked.len() == 1 { picked[0].transcript.clone() } else { history::merged_transcript(&picked) };
        (transcript, parts, first)
    };
    emit(&app, "summarizing", "", transcript.len());
    let (title, summary) = summarize(&app, &story, &transcript).await.inspect_err(|e| emit(&app, "error", e, 0))?;
    {
        let mut history = story.history.lock().unwrap();
        history.retain(|s| !ids.contains(&s.id));
        history.push(history::Session { id: first_id, title, summary, transcript, parts });
        history.sort_by_key(|s| s.id);
    }
    story.save_history();
    emit_history(&app, &story);
    emit(&app, "done", "", 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_typing_subtitles() {
        let mut t = Vec::new();
        assert_eq!(record(&mut t, "Paimon: Hey, Travel".into(), &[]), Reading::New);
        assert_eq!(record(&mut t, "Paimon: Hey, Traveler! Look over there!".into(), &[]), Reading::Same);
        assert_eq!(record(&mut t, "Paimon: Hey, Traveler! Look over there!".into(), &[]), Reading::Same);
        assert_eq!(record(&mut t, "Navia: Welcome to the Spina di Rosula.".into(), &[]), Reading::New);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], "Paimon: Hey, Traveler! Look over there!");
    }

    #[test]
    fn ocr_slip_counts_as_same_block() {
        let mut t = vec!["Furina: The trial will now begin, everyone.".to_string()];
        assert_eq!(record(&mut t, "Furina: The trial will now begin, everyane.".into(), &[]), Reading::Same);
        assert_eq!(t.len(), 1);
    }

    fn line(text: &str, left: f32, y: i32, icon: bool) -> ocr::Line {
        ocr::Line { text: text.into(), center: (0, y), left, icon, gold: false }
    }

    #[test]
    fn subtitle_puts_the_speaker_first() {
        let gold = |t: &str| ocr::Line { gold: true, ..line(t, 0.4, 0, false) };
        let white = |t: &str| line(t, 0.2, 0, false);
        let lines = [
            ocr::Line { icon: true, ..white("Goodbye.") },
            gold("Margaret"),
            gold("Boss, Cat's Tail"),
            white("Hehe, that's alright."),
            white("Thanks again."),
        ];
        assert_eq!(subtitle(&lines), "Margaret: Hehe, that's alright. Thanks again.");
        assert_eq!(subtitle(&[white("...And then the lights went out.")]), "...And then the lights went out.");
        assert_eq!(subtitle(&[gold("Vesna")]), "", "only a name so far");
    }

    #[test]
    fn keeps_aligned_options_with_icons() {
        let lines = vec![
            line("Margaret", 0.636, 611, true),
            line("Timaeus", 0.637, 467, true),
            line("Craft", 0.636, 539, true),
            line("<Boss, Cat's Tail>", 0.75, 353, false),
            line("Faruzan", 0.80, 360, true),
        ];
        let kept: Vec<String> = option_lines(lines).into_iter().map(|l| l.text).collect();
        assert_eq!(kept, ["Timaeus", "Craft", "Margaret"]);
    }

    #[test]
    fn spots_risky_menus_and_options() {
        assert!(mentions("Purchase", RISKY_WORDS));
        assert!(!mentions("UID: 886435317", RISKY_WORDS));
        assert!(mentions("I'd like to buy something.", RISKY_OPTIONS));
        assert!(!mentions("Tell me about the Fortress.", RISKY_OPTIONS));
    }

    #[test]
    fn advances_only_on_the_diamond_outside_the_open_world() {
        assert!(advance(true, false));
        assert!(!advance(false, false), "line still typing");
        assert!(!advance(false, true), "open-world chatter");
        assert!(!advance(true, true));
    }

    #[test]
    fn replayed_lines_are_skipped() {
        let earlier = vec!["--- Session 1 ---".to_string(), "Vesna: Okay, this must be the second card.".to_string()];
        let mut t = Vec::new();
        // Typing out a line that was already read: starts as "new", then turns out to be a repeat.
        assert_eq!(record(&mut t, "Vesna: Okay, this".into(), &earlier), Reading::New);
        assert_eq!(record(&mut t, "Vesna: Okay, this must be the second card.".into(), &earlier), Reading::Repeat);
        assert!(t.is_empty());
        // An OCR slip still counts as the same line.
        assert_eq!(record(&mut t, "Vesna: Okay, this must be the secand card.".into(), &earlier), Reading::Repeat);
        assert_eq!(record(&mut t, "Marozov: Stop, which department are you?".into(), &earlier), Reading::New);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn near_equal_tolerates_slips_only() {
        assert!(near_equal("Thanks again for your help.", "Thanks agaln for your help."));
        assert!(!near_equal("Thanks again for your help.", "Thanks again, see you later."));
    }

    #[test]
    fn long_transcripts_split_into_parts_at_session_breaks() {
        let mut t: Vec<String> = (0..300).map(|i| format!("Line {i}")).collect();
        t[110] = "--- Session 2 ---".into();
        let p = parts(&t);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].len(), 110, "cut at the session break");
        assert_eq!(p.iter().map(|x| x.len()).sum::<usize>(), 300);
        assert_eq!(parts(&t[..50]).len(), 1);
    }

    #[test]
    fn delay_follows_line_length() {
        let short = "Okay.";
        let long = "The Golden House? You mean, where Mora is minted? I've never been there, so I can't compare.";
        assert_eq!(line_delay(1000, short, false), 1000);
        assert_eq!(line_delay(1000, short, true), 400);
        assert!(line_delay(1000, long, true) > 1000);
        assert_eq!(line_delay(1000, &long.repeat(5), true), 3000);
    }

    #[test]
    fn parses_choice_numbers() {
        assert_eq!(parse_choice("2", 3), Some(1));
        assert_eq!(parse_choice("Option 3.", 3), Some(2));
        assert_eq!(parse_choice("4", 3), None);
        assert_eq!(parse_choice("none", 3), None);
    }
}

#[tauri::command]
pub fn set_story_settings(settings: StorySettings, story: tauri::State<'_, StoryHandle>) {
    story.set_settings(settings);
}
