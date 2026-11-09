//! [`PointerLocator`] from the system: XWayland's `QueryPointer` on Linux (Path of Exile runs
//! there under Proton), `GetCursorPos` and `GetAsyncKeyState` on Windows.

use crate::{Pointer, PointerLocator};

/// Connects on first use, so a missing X display shows up as an unknown pointer rather than a
/// failed launch.
#[derive(Default)]
pub struct SystemPointer {
    #[cfg(target_os = "linux")]
    x: Option<(x11rb::rust_connection::RustConnection, u32)>,
}

impl SystemPointer {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "linux")]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};

        if self.x.is_none() {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            self.x = Some((conn, root));
        }
        let (conn, root) = self.x.as_ref()?;
        let Some(reply) = conn
            .query_pointer(*root)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            // Reconnect next time: the X server may have gone and come back.
            self.x = None;
            return None;
        };
        // Buttons 4 and 5 are the wheel: scrolling isn't a press.
        let buttons = KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3;
        Some(Pointer {
            at: (reply.root_x.into(), reply.root_y.into()),
            pressed: reply.mask.intersects(buttons),
        })
    }
}

#[cfg(target_os = "windows")]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut at = POINT::default();
        // SAFETY: `at` is a valid, writable POINT for the call's duration.
        unsafe { GetCursorPos(&mut at) }.ok()?;
        let pressed = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
            .into_iter()
            // SAFETY: GetAsyncKeyState takes any virtual-key code and has no other inputs.
            // The high bit is set while the button is down.
            .any(|vk| unsafe { GetAsyncKeyState(vk.0.into()) } < 0);
        Some(Pointer {
            at: (at.x, at.y),
            pressed,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        None
    }
}
