//! [`KeySender`] through `enigo`: XTEST on X11 (and on XWayland, where Path of Exile runs under
//! Proton), `SendInput` on Windows.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::{InputError, KeySender};

/// Connects on first use, so a missing X display shows up as a failed Price Check rather than
/// a failed launch.
#[derive(Default)]
pub struct EnigoInput {
    enigo: Option<Enigo>,
}

impl EnigoInput {
    pub fn new() -> Self {
        Self::default()
    }

    fn enigo(&mut self) -> Result<&mut Enigo, InputError> {
        if self.enigo.is_none() {
            let enigo = Enigo::new(&Settings::default())
                .map_err(|e| InputError(format!("can't connect for input: {e}")))?;
            self.enigo = Some(enigo);
        }
        self.enigo
            .as_mut()
            .ok_or_else(|| InputError("input connection missing".into()))
    }
}

impl KeySender for EnigoInput {
    fn send_copy(&mut self) -> Result<(), InputError> {
        let enigo = self.enigo()?;
        let fail = |e: enigo::InputError| InputError(format!("can't send Ctrl+C: {e}"));
        enigo.key(Key::Control, Direction::Press).map_err(fail)?;
        let copied = enigo.key(Key::Unicode('c'), Direction::Click).map_err(fail);
        // Release Ctrl even when the C failed, so the game isn't left with it held.
        enigo.key(Key::Control, Direction::Release).map_err(fail)?;
        copied
    }
}
