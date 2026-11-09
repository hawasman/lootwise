//! Input Lootwise sends to the game, the pointer it watches, and the clipboard sequence a Price
//! Check runs.
//!
//! Path of Exile puts an item's text on the clipboard when the player presses Ctrl+C over it.
//! A Price Check sends that keystroke for the player ([`KeySender`]), reads the text, and puts
//! the player's own clipboard back afterwards ([`ClipboardCopy`]). While its card is shown,
//! Lootwise samples the pointer ([`PointerLocator`]) to close the card once the player leaves
//! it, without taking focus from the game; the card opens on the [`Display`] under the pointer.
//! Each is a small interface so tests and other platforms can swap them (ADR 0001: Rune has no
//! keystroke synthesis or global pointer).

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
    /// Whether the left, right or middle mouse button is held down.
    pub pressed: bool,
}

/// A display as the pointer sees it, in the pointer's global desktop pixels (on Wayland,
/// XWayland's root coordinates).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    /// The connector (`HDMI-A-1`) on Linux, the device (`\\.\DISPLAY1`) on Windows.
    pub name: Option<String>,
    /// Its top-left corner.
    pub origin: (i32, i32),
    /// Its width and height.
    pub size: (u32, u32),
    pub primary: bool,
}

impl Display {
    /// Whether `at`, in global desktop pixels, lies on this display.
    pub fn contains(&self, (x, y): (i32, i32)) -> bool {
        let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
        let (x, y) = (i64::from(x), i64::from(y));
        x >= ox && y >= oy && x < ox + i64::from(self.size.0) && y < oy + i64::from(self.size.1)
    }
}

/// Samples the mouse pointer.
pub trait PointerLocator {
    /// `None` when the platform can't say (a Wayland-native window under the pointer).
    fn pointer(&mut self) -> Option<Pointer>;
    /// The display holding `at` (global desktop pixels); `None` when the platform can't say.
    fn display(&mut self, at: (i32, i32)) -> Option<Display>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_holds_the_points_from_its_origin_up_to_its_far_edges() {
        let right = Display {
            name: Some("HDMI-A-1".into()),
            origin: (1920, 0),
            size: (1920, 1080),
            primary: true,
        };
        assert!(right.contains((1920, 0)));
        assert!(right.contains((2264, 606)));
        assert!(right.contains((3839, 1079)));
        assert!(!right.contains((1919, 500)), "the display to its left");
        assert!(!right.contains((3840, 500)));
        assert!(!right.contains((2000, 1080)));
        let left = Display {
            origin: (-1920, -200),
            ..right
        };
        assert!(left.contains((-1, -200)), "negative origins");
    }
}
