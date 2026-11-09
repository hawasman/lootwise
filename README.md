# Lootwise

A Path of Exile pricing tool that tells a player what their items and stash are worth, both on demand in-game and over time. Vocabulary: [`GLOSSARY.md`](GLOSSARY.md). Decisions: [`docs/adr`](docs/adr).

## Workspace

| Crate | Holds |
| --- | --- |
| `crates/lootwise-core` | The domain engine: clipboard item parsing, Commodity Item classification and Valuation, with `PriceFeed` and `Clock` ports ([README](crates/lootwise-core/README.md)) |
| `crates/lootwise-sources` | `RelayFeed`, the `PriceFeed` backed by the price relay: downloads the relayed poe.ninja files whose manifest etag changed into a disk cache, at most every 15 minutes, and normalises them for the engine |
| `crates/lootwise-input` | `KeySender` (Ctrl+C to the focused window, through `enigo`), `PointerLocator` (`SystemPointer`: the pointer's position, whether a mouse button is held and the display it is on, through XWayland and RandR or Win32), and `ClipboardCopy`, the save → copy → read → restore sequence of a Price Check |
| `crates/lootwise-app` | The `lootwise` binary: a tray app on Rune with the Ctrl+D Price Check overlay |

`relay/` is the Cloudflare Worker that relays poe.ninja prices to R2 (ADR 0004); see [`relay/README.md`](relay/README.md).

The toolchain is pinned in `rust-toolchain.toml` (rustup installs it on the first build), and every Rune crate is pinned to one rev (ADR 0001).

```sh
cargo build
cargo clippy --all-targets
cargo test
```

## Running the proof of concept on Linux

Ctrl+D over a Commodity Item in the game shows its Valuation in an overlay beside the pointer: chaos and divine value, listing count (poe.ninja's Currency Exchange prices have none), price age and source, marked **Low confidence** when there are fewer than 10 listings or the prices are over an hour old. Other items show "Not a Commodity Item" (they would need a Live Search, not built yet) or "No price". If the relay has no prices for the configured League at all, every Commodity Item shows "Prices unavailable for this League".

Tested setup: KDE Plasma on Wayland, Path of Exile running under Proton (so the game is an XWayland window).

1. Make a local mirror of the price relay (the deployed relay doesn't exist yet). Needs Node 22.18+ and pnpm:

   ```sh
   cd relay && pnpm install
   pnpm dev:mirror ~/lootwise-mirror
   ```

   With no `--league`, the mirror covers the Leagues the relay serves: the current challenge League and `Standard`. It prints one line per League, named as poe.ninja names it, e.g. `Allflame: 48/48 categories current`; `ls ~/lootwise-mirror/v1` lists them too. Use `--league <name>` to mirror only one. Rerun it to refresh prices; Lootwise picks up changed categories within 15 minutes, or on its next start.
2. Point Lootwise at the folder, with `LOOTWISE_LEAGUE` set to the challenge League's name exactly as the mirror printed it (Lootwise defaults to `Standard`), and start it from a terminal in your Plasma session:

   ```sh
   LOOTWISE_RELAY=~/lootwise-mirror LOOTWISE_LEAGUE=Allflame cargo run --release -p lootwise-app
   ```

   | Variable | Meaning |
   | --- | --- |
   | `LOOTWISE_RELAY` | Required. The relay's `http(s)://` base URL (the folder holding `v1/`), or a local mirror folder. |
   | `LOOTWISE_LEAGUE` | The League to price in, spelled as poe.ninja does (`Standard`, `Hardcore Settlers`, ...). Default `Standard`. |
   | `LOOTWISE_CACHE` | Where downloaded prices are kept. Default `~/.local/share/lootwise/relay`. |

   A gold coin appears in the system tray; its menu has **Quit**. If the relay is unreachable, Lootwise prices from its cache, and those Valuations turn low-confidence as they age.
3. The first start asks, through KDE's global shortcuts dialog, to allow **Ctrl+D**. Accept it. (Rune registers it through the desktop's GlobalShortcuts portal on Wayland.)

   The portal only asks for an app it can name, so on Linux Lootwise gives itself the app id `io.github.hawasman.Lootwise` before registering: it installs its desktop file, [`packaging/linux/io.github.hawasman.Lootwise.desktop`](packaging/linux/io.github.hawasman.Lootwise.desktop), into `$XDG_DATA_HOME/applications` (`~/.local/share/applications`) with `Exec` set to the running binary, and moves itself into a systemd user scope `app-io.github.hawasman.Lootwise-<pid>.scope`. It rewrites a desktop file there only if it installed that file itself (its first line says so) and it launches another build; one you wrote or edited (delete that first line) is kept. Its log says what it did. Launched from the desktop's menu it is in such a scope already. If Ctrl+D still can't be registered (no systemd user session, say), Lootwise prints why and how to fix it.
4. In the game, hover an item and press **Ctrl+D**. Lootwise sends Ctrl+C to the game, reads the item text off the clipboard, puts your previous clipboard back, and shows the item's Valuation next to the pointer. Text that isn't an item shows "Not a Path of Exile item". Ctrl+D over another item while the card is open prices that one and moves the card beside it.
5. The card closes when you leave it: click anywhere outside it (back into the game, say), or move the pointer more than about a card's width away from it. Clicking the card itself keeps it open and lets **Esc** close it (not yet on Wayland; see the xkbcommon limit below). The card opens on the display the pointer is on.

### Known platform limits

- **Esc needs a click first.** The overlay never takes the keyboard on its own, so the game keeps focus while the card is open. Esc reaches the card only after you click it. (Grabbing Esc as a global shortcut while the card is open is not done: on Wayland it goes through the portal's consent dialog.)
- **Leaving the card is seen by sampling the pointer.** Without focus the overlay hears no clicks outside it, so while the card is shown Lootwise reads the pointer's position and buttons every 20 ms (XWayland's `QueryPointer`, or `GetCursorPos` and `GetAsyncKeyState` on Windows), and stops once it is hidden. A click shorter than that can be missed; a button already held when the card opens (walking, say) only counts once released and pressed again. XWayland sees the pointer only over X windows such as the game: over a Wayland-native window it keeps the last position there.
- **The copy keystroke goes through XWayland.** `enigo` sends Ctrl+C with XTEST on the X display (`DISPLAY` must be set), which reaches the game because it runs under XWayland. A Wayland-native game would not receive it. XWayland built with libei may route it through the input-capture portal and ask for permission.
- **Pointer position comes from XWayland too.** It is right while the pointer is over the game (an X window). With KDE's XWayland scaling set to "Apply scaling themselves" (the default) the card lands beside the pointer at any scale; with "Scaled by the system" on a scaled display it lands off by the scale factor.
- **The card opens on the display under the pointer.** At each Ctrl+D Lootwise asks XWayland's RandR (Windows: `MonitorFromPoint`) which display the pointer is on, finds that display in Rune's list by connector name (`HDMI-A-1`), else by position and size, and moves the overlay there (`OverlayHandle::set_display`; on Wayland Rune destroys the layer surface and makes a new one on that output). If none matches, the card goes to the primary display; on Wayland Rune's list marks no display primary, so the compositor picks the output instead. Either way the card keeps its spot relative to the top-left of the pointer's display. Rune lists displays once, at startup (the startup log prints them), so restart Lootwise after plugging in or rearranging displays; an unplugged display's overlay also goes to the compositor's choice. On Wayland a display's overlay reports its own origin as `(0, 0)` (Rune never reads a layer surface's position), so the card's placement and the leave check measure the pointer from its display's top-left rather than in desktop pixels.
- **`xkbcommon: ERROR: [XKB-822] Failed to parse input xkb string` on each card, and no Esc on Wayland.** It comes from Rune, not Lootwise or `enigo`. The cause is in `crates/rune/src/platform/layer_shell.rs` at the pinned rev `6945e51`, in the `wl_keyboard::Event::Keymap` arm of `impl Dispatch<WlKeyboard, ()> for State` (line 165): it reads the keymap fd with `std::fs::File::from(fd).take(size).read_to_string(..)`, a plain `read()`. Since `wl_keyboard` v7 clients must `mmap` that fd `MAP_PRIVATE`, because a compositor may send the same open file description to every client and every keyboard. A `read()` moves that shared file offset to the end, so the next reader gets an empty string, `read_to_string` still succeeds, and `xkb::Keymap::new_from_string` fails with XKB-822. Rune hides an overlay by destroying its layer surface, so each show of an overlay that may take the keyboard (Lootwise's `KeyPolicy::OnDemand`, so a click lets Esc close the card) binds a new `wl_keyboard` and reads the keymap again; moving the card to another display does the same. The same overlay without keyboard access binds no keyboard and prints nothing. With no keymap, Rune sets `state.xkb = None` and drops that overlay's keys, so on Wayland Esc doesn't close the card: click the game or move away instead. Upstream already tracks this as [anomalyco/rune#319](https://github.com/anomalyco/rune/issues/319) (reported on Hyprland). The fix there is to use `xkb::Keymap::new_from_fd` (an `mmap`, as winit and smithay-client-toolkit do) or `FileExt::read_exact_at(.., 0)`. Lootwise doesn't hide the message.
- **Clipboard restore keeps the main form.** Linux and Windows restore one form of what you had copied: files, else an image, else HTML with text, else text. An empty clipboard comes back as empty text.
- **Global shortcuts need the dbus portal on Wayland** (KDE has it; GNOME needs an extension for the tray icon). On an X11 session they are an X grab.
- Windows is expected to build (enigo uses `SendInput`) but is untested.
