//! [`PointerLocator`] from the system: XWayland's `QueryPointer` and RandR monitors on Linux
//! (Path of Exile runs there under Proton), `GetCursorPos`, `GetAsyncKeyState` and
//! `MonitorFromPoint` on Windows.

use crate::{Pointer, PointerDisplay, PointerLocator};

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
impl SystemPointer {
    /// The X connection and its root window, connecting if need be.
    fn x(&mut self) -> Option<&(x11rb::rust_connection::RustConnection, u32)> {
        use x11rb::connection::Connection;

        if self.x.is_none() {
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            self.x = Some((conn, root));
        }
        self.x.as_ref()
    }
}

#[cfg(target_os = "linux")]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};

        let (conn, root) = self.x()?;
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

    /// The RandR monitor holding `at`: XWayland lists one per Wayland output, named after its
    /// connector.
    fn display(&mut self, at: (i32, i32)) -> Option<PointerDisplay> {
        use x11rb::protocol::randr::ConnectionExt as _;
        use x11rb::protocol::xproto::ConnectionExt as _;

        let (conn, root) = self.x()?;
        let monitors = conn.randr_get_monitors(*root, true).ok()?.reply().ok()?;
        let (mut display, name) = monitors
            .monitors
            .into_iter()
            .map(|m| {
                let display = PointerDisplay {
                    name: None,
                    origin: (m.x.into(), m.y.into()),
                    size: (m.width.into(), m.height.into()),
                    primary: m.primary,
                };
                (display, m.name)
            })
            .find(|(display, _)| display.contains(at))?;
        display.name = conn
            .get_atom_name(name)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| String::from_utf8(reply.name).ok());
        Some(display)
    }
}

#[cfg(target_os = "windows")]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut at = POINT::default();
        // SAFETY: `at` is a valid, writable POINT for the call's duration.
        unsafe { GetCursorPos(&mut at) }.ok()?;
        let pressed = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .into_iter()
            // SAFETY: GetAsyncKeyState takes any virtual-key code and has no other inputs.
            // The high bit is set while the button is down.
            .any(|vk| unsafe { GetAsyncKeyState(vk.0.into()) } < 0);
        Some(Pointer {
            at: (at.x, at.y),
            pressed,
        })
    }

    fn display(&mut self, at: (i32, i32)) -> Option<PointerDisplay> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
        };
        use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

        // SAFETY: MonitorFromPoint takes a point by value and has no other inputs.
        let monitor =
            unsafe { MonitorFromPoint(POINT { x: at.0, y: at.1 }, MONITOR_DEFAULTTONULL) };
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` is a valid, writable MONITORINFOEXW whose cbSize says so, for the
        // call's duration.
        if !unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
            return None;
        }
        let rect = info.monitorInfo.rcMonitor;
        let device = &info.szDevice;
        let len = device.iter().position(|&c| c == 0).unwrap_or(device.len());
        Some(PointerDisplay {
            name: String::from_utf16(&device[..len]).ok(),
            origin: (rect.left, rect.top),
            size: (
                rect.right.saturating_sub(rect.left).max(0) as u32,
                rect.bottom.saturating_sub(rect.top).max(0) as u32,
            ),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl PointerLocator for SystemPointer {
    fn pointer(&mut self) -> Option<Pointer> {
        None
    }

    fn display(&mut self, _: (i32, i32)) -> Option<PointerDisplay> {
        None
    }
}
