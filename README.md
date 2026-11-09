# Lootwise

A Path of Exile pricing tool that tells a player what their items and stash are worth, both on demand in-game and over time. Vocabulary: [`GLOSSARY.md`](GLOSSARY.md). Decisions: [`docs/adr`](docs/adr).

## Workspace

| Crate | Holds |
| --- | --- |
| `crates/lootwise-input` | `KeySender` (Ctrl+C to the focused window, through `enigo`), `PointerLocator`, and `ClipboardCopy`, the save → copy → read → restore sequence of a Price Check |
| `crates/lootwise-app` | The `lootwise` binary: a tray app on Rune with the Ctrl+D Price Check overlay |

The toolchain is pinned in `rust-toolchain.toml` (rustup installs it on the first build), and every Rune crate is pinned to one rev (ADR 0001).

```sh
cargo build
cargo clippy --all-targets
cargo test
```

## Running the proof of concept on Linux

The POC does a Price Check without pricing: Ctrl+D over an item in the game shows the item's rarity and name in an overlay beside the pointer.

Tested setup: KDE Plasma on Wayland, Path of Exile running under Proton (so the game is an XWayland window).

1. Build and start it from a terminal in your Plasma session: `cargo run --release -p lootwise-app`. A gold coin appears in the system tray; its menu has **Quit**.
2. The first start asks, through KDE's global shortcuts dialog, to allow **Ctrl+D**. Accept it. (Rune registers it through the desktop's GlobalShortcuts portal on Wayland.)
3. In the game, hover an item and press **Ctrl+D**. Lootwise sends Ctrl+C to the game, reads the item text off the clipboard, puts your previous clipboard back, and shows the rarity and name next to the pointer. Text that isn't an item shows "Not a Path of Exile item".
4. Close the card with **Ctrl+D** again, or click it and press **Esc**.

### Known platform limits

- **Esc needs a click first.** The overlay never takes the keyboard on its own, so the game keeps focus while the card is open. Esc reaches the card only after you click it; Ctrl+D always closes it. (Grabbing Esc as a global shortcut while the card is open is not done: on Wayland it goes through the portal's consent dialog.)
- **The copy keystroke goes through XWayland.** `enigo` sends Ctrl+C with XTEST on the X display (`DISPLAY` must be set), which reaches the game because it runs under XWayland. A Wayland-native game would not receive it. XWayland built with libei may route it through the input-capture portal and ask for permission.
- **Pointer position comes from XWayland too.** It is right while the pointer is over the game (an X window). With KDE's XWayland scaling set to "Apply scaling themselves" (the default) the card lands beside the pointer at any scale; with "Scaled by the system" on a scaled display it lands off by the scale factor.
- **Wayland overlays use the primary display.** Wayland hides the pointer from Rune, so the overlay opens on the primary display's layer; on a multi-display setup with the game on another display the card shows on the primary one. X11 sessions and Windows pick the display under the pointer.
- **Clipboard restore keeps the main form.** Linux and Windows restore one form of what you had copied: files, else an image, else HTML with text, else text. An empty clipboard comes back as empty text.
- **Global shortcuts need the dbus portal on Wayland** (KDE has it; GNOME needs an extension for the tray icon). On an X11 session they are an X grab.
- Windows is expected to build (enigo uses `SendInput`) but is untested.
