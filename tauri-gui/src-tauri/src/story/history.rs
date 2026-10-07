//! Saved Story Mode sessions: every run is kept with its transcript and summary, and sessions can be
//! merged (e.g. one quest played over several evenings) into one summary. Related sessions are found
//! by the names they share, so the app can suggest what to merge.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Shared-name overlap (Jaccard) from which two sessions count as related.
const RELATED_MIN: f32 = 0.2;

/// Names that show up in almost every conversation, so they say nothing about which story it is.
const COMMON: &[&str] = &[
    "paimon", "traveler", "traveller", "aether", "lumine", "the", "you", "and", "but", "what", "that",
    "this", "there", "then", "well", "yes", "yeah", "okay", "huh", "hmm", "uh", "oh", "ah", "hey", "who",
    "why", "how", "when", "where", "let", "just", "now", "chose", "don", "can", "it's", "i'm", "we",
    "our", "they", "she", "he", "his", "her", "thanks", "thank", "sorry", "please", "good", "great",
];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Session {
    /// Start time in ms since the Unix epoch; also the id.
    pub id: u64,
    pub title: String,
    /// Empty when summarizing failed (it can be retried).
    pub summary: String,
    pub transcript: Vec<String>,
    /// How many runs went into this one (1 = not merged).
    pub parts: usize,
}

/// What the page shows: everything but the transcript, plus suggestions.
#[derive(Serialize, Clone, Debug)]
pub struct SessionView {
    pub id: u64,
    pub title: String,
    pub summary: String,
    pub lines: usize,
    pub parts: usize,
    /// Other sessions that look like the same story, most similar first.
    pub related: Vec<u64>,
}

/// Loads the history, tidying transcripts saved before speakers were split out. The bool is true when
/// something changed (so it gets saved back).
pub fn load(path: &Path) -> (Vec<Session>, bool) {
    let mut sessions: Vec<Session> =
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut changed = false;
    for session in &mut sessions {
        changed |= tidy_transcript(&mut session.transcript);
    }
    (sessions, changed)
}

/// Words that start sentences, never a speaker's name.
const NOT_NAMES: &[&str] = &[
    "i", "the", "a", "an", "what", "but", "and", "so", "oh", "hmm", "hm", "mm", "well", "yes", "no", "you", "we",
    "he", "she", "it", "they", "this", "that", "these", "those", "then", "wait", "okay", "ok", "ah", "huh", "let",
    "hey", "my", "our", "your", "if", "now", "there", "here", "thank", "thanks", "sorry", "who", "why", "how",
    "when", "where", "is", "are", "do", "did", "of", "in", "on", "right", "just", "please", "come", "look",
    "alright", "anyway", "ugh", "uh", "um", "not", "really", "wow", "hehe", "haha", "yeah", "yep", "nope", "sure",
    "all", "one", "even", "still", "as", "at", "for", "to", "with", "after", "before", "maybe", "perhaps", "since",
    "because", "still", "very", "good", "great", "fine", "true", "indeed", "listen", "see", "go", "don't", "i'm",
    "it's", "that's", "there's", "you're", "we're", "can", "could", "would", "should", "will", "may", "might",
    "her", "his", "their", "its", "some", "every", "no-one", "nothing", "everyone", "someone",
];

/// The game's gold "continue" diamond read as a stray character after the sentence ("…plan. 4").
pub(crate) fn strip_marker_tail(line: &str) -> String {
    let trimmed = line.trim_end();
    if let Some((head, last)) = trimmed.rsplit_once(' ') {
        let stray = last.chars().count() <= 2 && !last.chars().any(char::is_alphabetic);
        let after_sentence = head.trim_end().ends_with(['.', '!', '?', '…', '"', '”', ')']);
        if stray && after_sentence {
            return head.trim_end().to_string();
        }
    }
    trimmed.to_string()
}

fn has_speaker(line: &str) -> bool {
    line.split_once(": ").is_some_and(|(name, _)| {
        (1..=40).contains(&name.chars().count()) && !name.contains(['.', '!', '?', ':'])
    })
}

fn is_name_word(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(char::is_uppercase)
        && word.chars().count() >= 2
        && word.chars().all(|c| c.is_alphabetic() || c == '-' || c == '\'')
        && !NOT_NAMES.contains(&word.to_lowercase().as_str())
}

/// The next word starts a sentence (capital, "...", dash or quote), as it does after a glued name.
fn starts_sentence(word: &str) -> bool {
    word.chars().next().is_some_and(|c| c.is_uppercase() || "….—–-\"“'(".contains(c))
}

/// Old transcripts glued the speaker's name onto the line ("Vesna We came for..."). Names are the
/// first word (or two) that starts many lines and is followed by a new sentence; those lines become
/// "Vesna: We came for...". Also trims the stray continue-marker character. Returns true if changed.
pub fn tidy_transcript(lines: &mut [String]) -> bool {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in lines.iter().filter(|l| !l.starts_with("> ") && !l.starts_with("--- ") && !has_speaker(l)) {
        let words: Vec<&str> = line.split_whitespace().take(3).collect();
        if words.len() >= 2 && is_name_word(words[0]) && starts_sentence(words[1]) {
            *counts.entry(words[0].to_string()).or_default() += 1;
        }
        if words.len() >= 3 && is_name_word(words[0]) && is_name_word(words[1]) && starts_sentence(words[2]) {
            *counts.entry(format!("{} {}", words[0], words[1])).or_default() += 1;
        }
    }
    let names: BTreeSet<&String> = counts.iter().filter(|(_, n)| **n >= 3).map(|(name, _)| name).collect();

    let mut changed = false;
    for line in lines.iter_mut() {
        if line.starts_with("--- ") {
            continue;
        }
        let mut fixed = strip_marker_tail(line);
        if !fixed.starts_with("> ") && !has_speaker(&fixed) {
            let words: Vec<&str> = fixed.split_whitespace().collect();
            // Prefer a two-word name ("Tsaritsa Anastasya") over its first word.
            let speaker = [2, 1].into_iter().find_map(|n| {
                let name = words.get(..n)?.join(" ");
                (names.contains(&name) && words.get(n).is_some_and(|w| starts_sentence(w))).then_some((name, n))
            });
            if let Some((name, n)) = speaker {
                fixed = format!("{name}: {}", words[n..].join(" "));
            }
        }
        if fixed != *line {
            *line = fixed;
            changed = true;
        }
    }
    changed
}

pub fn save(path: &Path, sessions: &[Session]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string(sessions).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Capitalized words (names, places) in a transcript, minus the ones every story has.
fn names(transcript: &[String]) -> BTreeSet<String> {
    transcript
        .iter()
        .flat_map(|line| line.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-')))
        .filter(|w| w.chars().count() >= 3 && w.chars().next().is_some_and(char::is_uppercase))
        .map(str::to_lowercase)
        .filter(|w| !COMMON.contains(&w.as_str()))
        .collect()
}

fn similarity(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f32 / union as f32
}

/// Newest first, each with the sessions it's probably part of the same story as.
pub fn views(sessions: &[Session]) -> Vec<SessionView> {
    let names: Vec<_> = sessions.iter().map(|s| names(&s.transcript)).collect();
    let mut views: Vec<SessionView> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut related: Vec<(f32, u64)> = sessions
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(j, o)| (similarity(&names[i], &names[j]), o.id))
                .filter(|(score, _)| *score >= RELATED_MIN)
                .collect();
            related.sort_by(|a, b| b.0.total_cmp(&a.0));
            SessionView {
                id: s.id,
                title: s.title.clone(),
                summary: s.summary.clone(),
                lines: s.transcript.iter().filter(|l| !l.starts_with("> ") && !l.starts_with("--- ")).count(),
                parts: s.parts.max(1),
                related: related.into_iter().map(|(_, id)| id).collect(),
            }
        })
        .collect();
    views.sort_by(|a, b| b.id.cmp(&a.id));
    views
}

/// Joins sessions in the order they were played, marking where each one starts.
pub fn merged_transcript(parts: &[&Session]) -> Vec<String> {
    let mut parts = parts.to_vec();
    parts.sort_by_key(|s| s.id);
    let mut out = Vec::new();
    for (i, s) in parts.iter().enumerate() {
        if parts.len() > 1 {
            out.push(format!("--- Session {} ---", i + 1));
        }
        for line in s.transcript.iter().filter(|l| !l.starts_with("--- ")) {
            // A scene replayed after the game went back to a save point is kept only once.
            let dialogue = !line.starts_with("> ");
            if dialogue && out.iter().any(|o: &String| !o.starts_with("--- ") && super::near_equal(o, line)) {
                continue;
            }
            out.push(line.clone());
        }
    }
    out
}

/// Splits "Title: ...\n<summary>" from the model into (title, summary).
pub fn split_title(answer: &str) -> (String, String) {
    let answer = answer.trim();
    let (first, rest) = answer.split_once('\n').unwrap_or((answer, ""));
    match first.trim().strip_prefix("Title:").or_else(|| first.trim().strip_prefix("TITLE:")) {
        Some(title) => (title.trim().trim_matches(['*', '"']).to_string(), rest.trim().to_string()),
        None => (String::new(), answer.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: u64, lines: &[&str]) -> Session {
        Session { id, transcript: lines.iter().map(|l| l.to_string()).collect(), parts: 1, ..Default::default() }
    }

    #[test]
    fn related_by_shared_names() {
        let a = session(1, &["Navia: Welcome to the Spina di Rosula.", "Paimon: Wow!", "Navia: Clorinde is here."]);
        let b = session(2, &["Clorinde: Navia, the Spina needs you.", "Paimon: Hmm?"]);
        let c = session(3, &["Sara: Welcome to Good Hunter!", "Paimon: Food!"]);
        let v = views(&[a, b, c]);
        let find = |id| v.iter().find(|s| s.id == id).unwrap();
        assert_eq!(find(1).related, vec![2]);
        assert_eq!(find(2).related, vec![1]);
        assert!(find(3).related.is_empty());
        assert_eq!(v[0].id, 3, "newest first");
    }

    #[test]
    fn merges_in_play_order() {
        let a = session(5, &["Second"]);
        let b = session(2, &["First"]);
        assert_eq!(merged_transcript(&[&a, &b]), ["--- Session 1 ---", "First", "--- Session 2 ---", "Second"]);
    }

    #[test]
    fn merge_drops_replayed_lines() {
        let a = session(1, &["Vesna: The second card.", "Vesna: Let's go."]);
        let b = session(2, &["Vesna: The second card.", "Marozov: Intruders!"]);
        assert_eq!(
            merged_transcript(&[&a, &b]),
            ["--- Session 1 ---", "Vesna: The second card.", "Vesna: Let's go.", "--- Session 2 ---", "Marozov: Intruders!"]
        );
    }

    /// Dry run on a copy of a real history file: `STORY_HISTORY=copy.json cargo test --lib tidy_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tidy_real_history() {
        let (sessions, changed) = load(Path::new(&std::env::var("STORY_HISTORY").unwrap()));
        println!("changed: {changed}");
        for s in &sessions {
            println!("=== {} ({} lines)", s.title, s.transcript.len());
            for line in s.transcript.iter().take(25) {
                println!("  {line}");
            }
        }
    }

    #[test]
    fn splits_glued_speakers_in_old_transcripts() {
        let mut t: Vec<String> = [
            "Vesna We came for a bite to eat.",
            "Vodyanitsa Alright, everyone's here.",
            "Odette This morning, the Veche announced it.",
            "Vodyanitsa ...Wow, so much more complicated.",
            "Odette Her Majesty must have known all along. 4",
            "Vesna Who, me? Not to worry!",
            "Odette Since Her Majesty said so.",
            "Vodyanitsa But Vesna's power was taken away...",
            "Vesna That power never belonged to me.",
            "I know, right? Anyway, what now?",
            "> Chose: Let's go.",
            "Paimon: Already split.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(tidy_transcript(&mut t));
        assert_eq!(t[0], "Vesna: We came for a bite to eat.");
        assert_eq!(t[3], "Vodyanitsa: ...Wow, so much more complicated.");
        assert_eq!(t[4], "Odette: Her Majesty must have known all along.");
        assert_eq!(t[9], "I know, right? Anyway, what now?", "not a name");
        assert_eq!(t[10], "> Chose: Let's go.");
        assert_eq!(t[11], "Paimon: Already split.");
        assert!(!tidy_transcript(&mut t), "running again changes nothing");
    }

    #[test]
    fn splits_title_line() {
        assert_eq!(split_title("Title: Navia's Request\nShe asks for help."), ("Navia's Request".into(), "She asks for help.".into()));
        assert_eq!(split_title("Just a summary."), ("".into(), "Just a summary.".into()));
    }
}
