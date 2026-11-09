//! The clipboard save → copy → read → restore sequence of a Price Check, as a state machine.
//!
//! The UI owns the clipboard and the clock (Rune reads the clipboard asynchronously and wakes
//! its poll at deadlines), so this type never touches either: it says what to do next, and the
//! caller reports back. `C` is whatever the caller's clipboard content type is.
//!
//! 1. [`ClipboardCopy::begin`]: the caller reads the clipboard and hands it to
//!    [`ClipboardCopy::saved`], then blanks the clipboard so a game that copies nothing (the
//!    pointer wasn't over an item) can't leave a stale item behind.
//! 2. [`ClipboardCopy::tick`] on each poll: once the blank has settled it sends Ctrl+C, and once
//!    the game had time to answer it asks the caller to read the clipboard.
//! 3. [`ClipboardCopy::read`]: the caller has the item text; it gets back the saved content to
//!    write to the clipboard again.

use crate::{InputError, KeySender};

/// Seconds between blanking the clipboard and sending Ctrl+C, so the blank lands first.
pub const CLEAR_SETTLE: f64 = 0.03;
/// Seconds the game gets to put the item text on the clipboard after Ctrl+C.
pub const COPY_WAIT: f64 = 0.1;

#[derive(Debug, Default)]
enum Phase<C> {
    #[default]
    Idle,
    Saving,
    Clearing { saved: C, due: f64 },
    Copying { saved: C, due: f64 },
    Reading { saved: C },
}

/// What the caller does after a [`ClipboardCopy::tick`].
#[derive(Debug, PartialEq)]
pub enum Tick<C> {
    /// Nothing to do.
    Idle,
    /// Tick again once the clock reaches this time.
    WakeAt(f64),
    /// Read the clipboard now and pass the result's arrival to [`ClipboardCopy::read`].
    ReadClipboard,
    /// The keystroke failed: write `restore` back to the clipboard. The sequence is over.
    Failed { restore: C, error: InputError },
}

#[derive(Debug)]
pub struct ClipboardCopy<C> {
    phase: Phase<C>,
}

impl<C> Default for ClipboardCopy<C> {
    fn default() -> Self { Self { phase: Phase::Idle } }
}

impl<C> ClipboardCopy<C> {
    pub fn new() -> Self { Self::default() }

    /// A sequence is under way.
    pub fn is_busy(&self) -> bool { !matches!(self.phase, Phase::Idle) }

    /// Start a sequence: the caller reads the clipboard next. False while one is under way.
    pub fn begin(&mut self) -> bool {
        if self.is_busy() { return false; }
        self.phase = Phase::Saving;
        true
    }

    /// The clipboard as it was before the Price Check, read at `now`. The caller blanks the
    /// clipboard and ticks again at the returned time. Ignored outside step 1.
    pub fn saved(&mut self, content: C, now: f64) -> Option<f64> {
        if !matches!(self.phase, Phase::Saving) { return None; }
        let due = now + CLEAR_SETTLE;
        self.phase = Phase::Clearing { saved: content, due };
        Some(due)
    }

    /// Advance at clock `now`, sending Ctrl+C through `keys` when it's time.
    pub fn tick(&mut self, now: f64, keys: &mut dyn KeySender) -> Tick<C> {
        match std::mem::take(&mut self.phase) {
            Phase::Clearing { saved, due } if now >= due => match keys.send_copy() {
                Ok(()) => {
                    let due = now + COPY_WAIT;
                    self.phase = Phase::Copying { saved, due };
                    Tick::WakeAt(due)
                }
                Err(error) => Tick::Failed { restore: saved, error },
            },
            Phase::Copying { saved, due } if now >= due => {
                self.phase = Phase::Reading { saved };
                Tick::ReadClipboard
            }
            phase @ (Phase::Clearing { due, .. } | Phase::Copying { due, .. }) => {
                self.phase = phase;
                Tick::WakeAt(due)
            }
            phase => {
                self.phase = phase;
                Tick::Idle
            }
        }
    }

    /// The clipboard read after the copy has arrived. Returns the saved content to restore,
    /// ending the sequence; `None` outside step 3.
    pub fn read(&mut self) -> Option<C> {
        match std::mem::take(&mut self.phase) {
            Phase::Reading { saved } => Some(saved),
            phase => {
                self.phase = phase;
                None
            }
        }
    }
}
