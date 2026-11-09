//! Input Lootwise sends to the game, and the clipboard sequence a Price Check runs.
//!
//! Path of Exile puts an item's text on the clipboard when the player presses Ctrl+C over it.
//! A Price Check sends that keystroke for the player ([`KeySender`]), reads the text, and puts
//! the player's own clipboard back afterwards ([`ClipboardCopy`]). Both are small interfaces so
//! tests and other platforms can swap them (ADR 0001: Rune has no keystroke synthesis).

mod copy;
mod enigo_input;

pub use copy::{ClipboardCopy, Tick, CLEAR_SETTLE, COPY_WAIT};
pub use enigo_input::EnigoInput;

/// Why a keystroke or pointer query failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputError(pub String);

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for InputError {}

/// Sends the copy chord to whichever window has keyboard focus (the game, during a Price Check).
pub trait KeySender {
    /// Press and release Ctrl+C.
    fn send_copy(&mut self) -> Result<(), InputError>;
}

/// Where the mouse pointer is, in global desktop pixels.
pub trait PointerLocator {
    /// `None` when the platform can't say (a Wayland-native window under the pointer).
    fn pointer(&mut self) -> Option<(i32, i32)>;
}
