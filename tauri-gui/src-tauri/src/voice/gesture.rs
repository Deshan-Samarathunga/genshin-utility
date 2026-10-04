//! Mic-button gesture state machine. Pure (timestamps are passed in) so it can be unit-tested.
//!
//! hold, speak, release -> record and transcribe (typed at the end of the chat box)
//! double tap           -> undo the last typed take
//! single tap           -> nothing
//!
//! It deliberately keeps no idea of a "message": the chat box may already hold text the user left
//! there, so every take is simply added at the end.

/// Presses shorter than this are taps, not recordings.
pub const MIN_RECORD_MS: u64 = 250;
/// Max gap between releasing the first tap and pressing the second.
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
    /// The press was a tap: drop what was recorded.
    CancelRecording,
    StopAndTranscribe,
    /// Remove the last take that was typed.
    UndoLast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Button held; audio is being captured from the moment of the press.
    Recording { since: u64 },
    Transcribing,
    /// A tap was released; a second press soon after makes it a double tap.
    Tapped { at: u64 },
    /// Second press of a double tap is still held; wait for its release.
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

    /// Called by the orchestrator when a transcription finished (typed or not).
    pub fn transcribe_done(&mut self) {
        if self.state == State::Transcribing {
            self.state = State::Idle;
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
                    self.state = State::Tapped { at: now };
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
            (State::Tapped { at }, Input::Down) => {
                if now.saturating_sub(at) <= DOUBLE_TAP_MS {
                    self.state = State::WaitRelease;
                    vec![UndoLast]
                } else {
                    // Too late for a double tap: this press is a new recording.
                    self.state = State::Recording { since: now };
                    vec![StartRecording]
                }
            }
            (State::Tapped { at }, Input::Tick) if now.saturating_sub(at) > DOUBLE_TAP_MS => {
                self.state = State::Idle;
                vec![]
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
        g.transcribe_done();
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn single_tap_does_nothing() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Up, 100), (Input::Tick, 300), (Input::Tick, 600)]);
        assert_eq!(a, vec![StartRecording, CancelRecording]);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn double_tap_undoes_last_take() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Up, 120), (Input::Down, 400), (Input::Up, 500)]);
        assert_eq!(a, vec![StartRecording, CancelRecording, UndoLast]);
        assert_eq!(g.state, State::Idle);
    }

    #[test]
    fn slow_second_press_records_instead() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Up, 120), (Input::Down, 700), (Input::Up, 2000)]);
        assert_eq!(a, vec![StartRecording, CancelRecording, StartRecording, StopAndTranscribe]);
    }

    #[test]
    fn presses_while_transcribing_are_ignored() {
        let mut g = Gesture { state: State::Transcribing };
        assert!(run(&mut g, &[(Input::Down, 0), (Input::Up, 500)]).is_empty());
    }

    #[test]
    fn recording_is_capped() {
        let mut g = Gesture::default();
        let a = run(&mut g, &[(Input::Down, 0), (Input::Tick, MAX_RECORD_MS), (Input::Up, MAX_RECORD_MS + 500)]);
        assert_eq!(a, vec![StartRecording, StopAndTranscribe]);
    }
}
