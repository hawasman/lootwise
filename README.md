# Lootwise

A Path of Exile pricing tool that tells a player what their items and stash are worth, both on demand in-game and over time. Vocabulary: [`GLOSSARY.md`](GLOSSARY.md). Decisions: [`docs/adr`](docs/adr).

## Workspace

| Crate | Holds |
| --- | --- |
| `crates/lootwise-core` | The domain engine: clipboard item parsing, Commodity Item classification and Valuation, with `PriceFeed` and `Clock` ports ([README](crates/lootwise-core/README.md)) |
| `crates/lootwise-sources` | `RelayFeed`, the `PriceFeed` backed by the price relay: downloads the relayed poe.ninja files whose manifest etag changed into a disk cache, at most every 15 minutes, and normalises them for the engine |
| `crates/lootwise-input` | `KeySender` (Ctrl+C to the focused window, through `enigo`), `PointerLocator`, and `ClipboardCopy`, the save → copy → read → restore sequence of a Price Check |
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

   The portal only asks for an app it can name, so on Linux Lootwise gives itself the app id `io.github.hawasman.Lootwise` before registering: it installs its desktop file, [`packaging/linux/io.github.hawasman.Lootwise.desktop`](packaging/linux/io.github.hawasman.Lootwise.desktop), into `$XDG_DATA_HOME/applications` (`~/.local/share/applications`) with `Exec` set to the running binary, replacing a copy that launches another build, and moves itself into a systemd user scope `app-io.github.hawasman.Lootwise-<pid>.scope`. Its log says what it did. Launched from the desktop's menu it is in such a scope already. If Ctrl+D still can't be registered (no systemd user session, say), Lootwise prints why and how to fix it.
4. In the game, hover an item and press **Ctrl+D**. Lootwise sends Ctrl+C to the game, reads the item text off the clipboard, puts your previous clipboard back, and shows the item's Valuation next to the pointer. Text that isn't an item shows "Not a Path of Exile item".
5. Close the card with **Ctrl+D** again, or click it and press **Esc**.

### Known platform limits

- **Esc needs a click first.** The overlay never takes the keyboard on its own, so the game keeps focus while the card is open. Esc reaches the card only after you click it; Ctrl+D always closes it. (Grabbing Esc as a global shortcut while the card is open is not done: on Wayland it goes through the portal's consent dialog.)
- **The copy keystroke goes through XWayland.** `enigo` sends Ctrl+C with XTEST on the X display (`DISPLAY` must be set), which reaches the game because it runs under XWayland. A Wayland-native game would not receive it. XWayland built with libei may route it through the input-capture portal and ask for permission.
- **Pointer position comes from XWayland too.** It is right while the pointer is over the game (an X window). With KDE's XWayland scaling set to "Apply scaling themselves" (the default) the card lands beside the pointer at any scale; with "Scaled by the system" on a scaled display it lands off by the scale factor.
- **Wayland overlays use the primary display.** Wayland hides the pointer from Rune, so the overlay opens on the primary display's layer; on a multi-display setup with the game on another display the card shows on the primary one. X11 sessions and Windows pick the display under the pointer.
- **Clipboard restore keeps the main form.** Linux and Windows restore one form of what you had copied: files, else an image, else HTML with text, else text. An empty clipboard comes back as empty text.
- **Global shortcuts need the dbus portal on Wayland** (KDE has it; GNOME needs an extension for the tray icon). On an X11 session they are an X grab.
- Windows is expected to build (enigo uses `SendInput`) but is untested.
