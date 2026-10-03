//! Mic-button gesture state machine. Pure (timestamps are passed in) so it can be unit-tested.
//!
//! idle      hold          -> record, release -> transcribe
//! draft     single tap    -> send
//! draft     double tap    -> discard
//! draft     hold          -> discard + record a new take

/// Presses shorter than this from idle are ignored (accidental bumps).
pub const MIN_RECORD_MS: u64 = 250;
/// In draft, holding at least this long means "redo".
pub const HOLD_MS: u64 = 500;
/// Max gap between releasing the first tap and pressing the second. Also how long a single tap waits before sending.
pub const DOUBLE_TAP_MS: u64 = 450;
pub const MAX_RECORD_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Down,
    Up,
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    StartRecording,
    CancelRecording,
    StopAndTranscribe,
    Send,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording { since: u64 },
    Transcribing,
    Draft,
    /// Pressed while a draft is pending; audio is already being captured in case it becomes a hold.
    DraftPressed { since: u64 },
    DraftTapped { at: u64 },
    /// Second tap of a double tap is still held; wait for its release.
    WaitRelease,
}

#[derive(Debug)]
pub struct Gesture {
    pub state: State,
}

impl Default for Gesture {
    fn default() -> Self {
        Self { state: State::Idle }
    }
}

impl Gesture {
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Called by the orchestrator when a transcription finished; `has_draft` = text was typed into chat.
    pub fn transcribe_done(&mut self, has_draft: bool) {
        if self.state == State::Transcribing {
            self.state = if has_draft { State::Draft } else { State::Idle };
        }
    }

    pub fn step(&mut self, input: Input, now: u64) -> Vec<Action> {
        use Action::*;
        match (self.state, input) {
            (State::Idle, Input::Down) => {
                self.state = State::Recording { since: now };
                vec![StartRecording]
            }
            (State::Recording { since }, Input::Up) => {
                if now.saturating_sub(since) < MIN_RECORD_MS {
                    self.state = State::Idle;
                    vec![CancelRecording]
                } else {
                    self.state = State::Transcribing;
                    vec![StopAndTranscribe]
                }
            }
            (State::Recording { since }, Input::Tick) if now.saturating_sub(since) >= MAX_RECORD_MS => {
                self.state = State::Transcribing;
                vec![StopAndTranscribe]
            }
            (State::Draft, Input::Down) => {
                self.state = State::DraftPressed { since: now };
                vec![StartRecording]
            }
            (State::DraftPressed { since }, Input::Tick) if now.saturating_sub(since) >= HOLD_MS => {
                self.state = State::Recording { since };
                vec![Discard]
            }
            (State::DraftPressed { since }, Input::Up) => {
                if now.saturating_sub(since) >= HOLD_MS {
                    // Tick was late; treat as a (very short) redo.
                    self.state = State::Transcribing;
                    vec![Discard, StopAndTranscribe]
                } else {
                    self.state = State::DraftTapped { at: now };
                    vec![CancelRecording]
                }
            }
            (State::DraftTapped { at }, Input::Down) if now.saturating_sub(at) <= DOUBLE_TAP_MS => {
                self.state = State::WaitRelease;
                vec![Discard]
            }
            (State::DraftTapped { at }, Input::Tick) if now.saturating_sub(at) > DOUBLE_TAP_MS => {
                self.state = State::Idle;
                vec![Send]
            }
            (State::WaitRelease, Input::Up) => {
                self.state = State::Idle;
                vec![]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;

    fn run(g: &mut Gesture, events: &[(Input, u64)]) -> Vec<Action> {
        events.iter().flat_map(|&(i, t)| g.step(i, t)).collect()
    }

    #[test]
    fn hold_records_and_transcribes() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Tick, 500), (Input::Up, 1500)]);
        assert_eq!(a, vec![StartRecording, StopAndTranscribe]);
        assert_eq!(g.state, State::Transcribing);
    }

    #[test]
    fn short_press_from_idle_is_ignored() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Up, 100)]);
        assert_eq!(a, vec![StartRecording, CancelRecording]);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn single_tap_on_draft_sends_after_window() {
        let mut g = Gesture { state: State::Draft };
        let a = run(&mut g, &[(Input::Down, 0), (Input::Up, 80), (Input::Tick, 300), (Input::Tick, 600)]);
        assert_eq!(a, vec![StartRecording, CancelRecording, Send]);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn double_tap_on_draft_discards() {
        let mut g = Gesture { state: State::Draft };
        let a = run(
            &mut g,
            &[(Input::Down, 0), (Input::Up, 120), (Input::Down, 520), (Input::Tick, 700), (Input::Up, 800)],
        );
        assert_eq!(a, vec![StartRecording, CancelRecording, Discard]);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn hold_on_draft_discards_and_rerecords() {
        let mut g = Gesture { state: State::Draft };
        let a = run(&mut g, &[(Input::Down, 0), (Input::Tick, 550), (Input::Up, 2000)]);
        assert_eq!(a, vec![StartRecording, Discard, StopAndTranscribe]);
        assert_eq!(g.state, State::Transcribing);
    }

    #[test]
    fn presses_while_transcribing_are_ignored() {
        let mut g = Gesture { state: State::Transcribing };
        assert!(run(&mut g, &[(Input::Down, 0), (Input::Up, 500)]).is_empty());
        g.transcribe_done(true);
        assert_eq!(g.state, State::Draft);
    }

    #[test]
    fn recording_is_capped() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Tick, MAX_RECORD_MS), (Input::Up, MAX_RECORD_MS + 500)]);
        assert_eq!(a, vec![StartRecording, StopAndTranscribe]);
    }
}
