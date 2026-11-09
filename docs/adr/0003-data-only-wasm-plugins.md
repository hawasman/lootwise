# Plugins extend data, not UI, and will run as WASM components

Plugins may add price sources, valuation rules, and extra item details expressed as structured data that Lootwise renders itself; they may not contribute UI panels. Rune has no stable ABI and changes between revisions, so UI plugins would break on every Rune bump, and plugins run third-party code in a process that holds the player's OAuth token. The first release defines these extension points as Rust traits in `lootwise-core`; a later plugin host will implement them over sandboxed WASM components (wasmtime) with no access to credentials.

## Considered Options

Native dynamic libraries (no sandbox, unstable Rust ABI) and embedded scripting (weaker isolation story, a second language to support) were rejected.
