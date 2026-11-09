# Lootwise reference design

The single design the Rune build copies. It combines design 3 (the glanceable Session screen and its reading order on the overlay card) with design 4 (the quiet rail, status bar, quote card, one accent selection line), plus the inline rare-item minimums and Ctrl+K palette from design 1 and the recent Price Check list from design 2. Dark only, one typeface (Public Sans).

## Open it

Open `designs/reference.html` in a browser (no build, fonts from `designs/assets/`). A 1280×800 app window sits inside the page.

- Switcher above the window lists every surface; `←` / `→` step through them; `#surface` deep-links (for example `reference.html#check-rare`).
- In the window: `1` Session, `2` Portfolio, `3` Holdings, `Ctrl K` command palette (type, `↑ ↓`, `Enter`, `Esc`), `↑ ↓` + `Enter` on Recent Price Checks.
- Price Check surfaces are the transparent overlay over the game; click the game area to go back to Session.

## Decisions

- **Shell**: rail with three destinations, Session (home), Portfolio, Holdings; Settings at the foot. Status bar shows both hotkeys and price freshness. One 1px accent line marks the current item everywhere.
- **Session** answers "what am I earning right now": earnings per hour is the biggest figure (96px), then elapsed, net worth change, last/next Snapshot, the Session strip chart (Snapshot ticks, dashed start baseline), Start/Stop Session and Take Snapshot, and 5 Recent Price Checks that re-open their card.
- **Portfolio**: net worth over the League with Sessions as bands, Positions by stash tab. **Holdings**: Snapshot by tab, by item, and unpriced items.
- **Overlay card**: price first and largest (divine, chaos beside it), then name, a small ledger (listings, price age), the 7-day line with start and end values, the poe.ninja link. A confidence pill appears only when low, with its reason. Rare cards show the Live Search median, filter rows with checkbox, roll and inline editable minimum, and one primary "Open on trade site". The card sits beside the hovered item (right, flipping left near the screen edge) and says "Click the game or move away to close"; it never mentions Esc (Wayland cannot deliver it to the card).
- **Data hue**: divine gold `#e4c76b`, used only on the main data element (earnings figure, chart line, card price). Chosen over design 3's lime because the economy is priced in Divine Orbs, gold ties the figure to its unit, and lime reads as "profit" even on a loss. Signs, not colour, show direction; red is reserved for errors.
- **Tabs changed mid-Session**: earnings per hour and net worth change become dashes with the reason; the stash tab setup warns before it happens.

## Tokens

| Role | Value |
| --- | --- |
| Ground (window) | `#171719` |
| Surface (cards, overlay card) | `#131217` |
| Raised (hover, skeleton, palette row) | `#212023` |
| Border / rule / control | `#222224` / `#1b1a1f` / `#3a3a3e` |
| Text / muted | `#ffffff` / `#8a8c92` |
| Data hue (accent) | `#e4c76b` |
| Positive / negative / warning | `#44a282` / `#e5566f` / `#d9a441` |
| Pills (bg / ink) | positive `#1b2828` `#5fc39c`, negative `#2c1a20` `#e5566f`, warning `#2a2416` `#d9a441`, neutral `#212023` `#bdbdc0` |
| Radius | cards 6px, controls 6px, buttons and pills 999px |
| Spacing | gutter 24px, window padding 32px, rail 72px, top bar 44px, status bar 32px |
| Font | Public Sans 400 / 500 / 600 / 700, tabular figures |
| Type sizes | hero 96 / 700, overlay price 48 / 700, lead 32 / 600, title 26 / 600, KPI 20 / 500, section 16 / 600, body 13, small 12, meta 11 (rail labels and chart axes only) |

Overlay card: 320px wide, 16px padding, `#34343a` 1px border, one soft shadow, the only opaque element over the game.

## Sample content to replace

Everything is sample data: the prices (a poe.ninja snapshot through the relay mirror, 2026-11-09), the Allflame League, stash tabs and items, the three Sessions and Snapshot history, "Wanderer#1042", the unpriced rares, the five Recent Price Checks, the Havoc Crest rare and its filters, the Divine Font gem options, and the stand-in game screen behind the cards. All figures are computed from the sample rows, so they agree wherever they repeat.
