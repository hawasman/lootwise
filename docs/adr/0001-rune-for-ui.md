# Build the UI on Rune, pinned to one `dev` revision

Lootwise's main window and in-game overlay are built with Rune (github.com/anomalyco/rune), a pre-release GPU-rendered Rust UI framework, because it ships what a price-check overlay needs natively: transparent, non-activating, click-through topmost overlays on Windows/X11/Wayland, global shortcuts, clipboard access, a virtualised table, and headless rendering for tests. We accept the cost of a 0.0.1 framework that takes no pull requests and has no crates.io release: the whole workspace pins a single `dev` rev and matches Rune's `rust-toolchain.toml`, and bumps happen only in a dedicated commit that re-runs the headless snapshot tests.

## Consequences

Rune lacks keystroke synthesis, an HTTP client, charts, tabs, autostart, and single-instance on Linux; Lootwise supplies these itself. Rune's Windows overlay path is only compile-checked upstream, so it must be verified early on a real Windows desktop.
