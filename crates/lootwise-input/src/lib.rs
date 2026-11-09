//! Input Lootwise sends to the game, the pointer it watches, and the clipboard sequence a Price
//! Check runs.
//!
//! Path of Exile puts an item's text on the clipboard when the player presses Ctrl+C over it.
//! A Price Check sends that keystroke for the player ([`KeySender`]), reads the text, and puts
//! the player's own clipboard back afterwards ([`ClipboardCopy`]). While its card is shown,
//! Lootwise samples the pointer ([`PointerLocator`]) to close the card once the player leaves
//! it, without taking focus from the game. Each is a small interface so tests and other
//! platforms can swap them (ADR 0001: Rune has no keystroke synthesis or global pointer).

mod copy;
mod enigo_input;
mod pointer;

pub use copy::{CLEAR_SETTLE, COPY_WAIT, ClipboardCopy, Tick};
pub use enigo_input::EnigoInput;
pub use pointer::SystemPointer;

/// Why a keystroke failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputError(pub String);

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InputError {}

/// Sends the copy chord to whichever window has keyboard focus (the game, during a Price Check).
pub trait KeySender {
    /// Press and release Ctrl+C.
    fn send_copy(&mut self) -> Result<(), InputError>;
}

/// The mouse pointer at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    /// Where it is, in global desktop pixels.
    pub at: (i32, i32),
    /// Whether a mouse button (left, right, middle or a side button) is held down.
    pub pressed: bool,
}

/// Samples the mouse pointer.
pub trait PointerLocator {
    /// `None` when the platform can't say (a Wayland-native window under the pointer).
    fn pointer(&mut self) -> Option<Pointer>;
}
