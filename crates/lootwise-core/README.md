# lootwise-core

The Lootwise domain engine. No I/O and no UI: it turns clipboard item text into a Price Check outcome for one League, with prices and time supplied through ports.

## Engine API

```rust
let engine = Engine::new(feed, clock); // feed: impl PriceFeed, clock: impl Clock
match engine.price_check(&League::new("Settlers"), clipboard_text) {
    PriceCheck::NotAnItem => {}                       // not PoE1 item text
    PriceCheck::NotACommodityItem { item } => {}      // would need a Live Search
    PriceCheck::NoPrice { item, commodity } => {}     // feed has no price in this League
    PriceCheck::Valued { item, valuation } => {}
}
```

- **Parsing** (`parse_item`): PoE1 English clipboard text (`Item Class:` / `Rarity:` / `--------`). Reads item class, rarity, name, base type (identified rares and uniques only), stack size, gem level, gem quality and corruption. Transfigured gems keep their transfigured name. Anything else is `NotAnItem`.
- **Commodity Items** (`CommodityItem::classify`): currency, fragments (including splinters), scarabs, essences, divination cards, skill gems (name + level + quality + corrupted) and identified uniques (name + base type). Everything else, including rares, magic items and unidentified uniques, is not a Commodity Item.
- **Valuation**: chaos value per unit (the stored unit), divine value at the League's divine rate, source, age of the feed data, listing count (when the source reports one) and confidence. Confidence is `Low` when there are fewer than 10 listings or the data is older than 1 hour; a price with no listing count is judged by age alone.

## Ports

- `PriceFeed`: `price(league, commodity) -> Option<PriceEntry>` and `divine_rate(league) -> Option<f64>` (chaos per divine). A `PriceEntry` is one normalised price: chaos value, listing count (`None` when the source has none), fetch time (when the feed last fetched or confirmed it) and source. Every call is scoped to one League; prices are never compared across Leagues. Adapters (the price relay, later Plugins) implement this trait.
- `Clock`: `now() -> SystemTime`.

### Mapping the relayed poe.ninja files

What a relay-backed `PriceFeed` reads from `v1/{league}/` (see `relay/README.md`):

| Port value | poe.ninja field |
| --- | --- |
| `fetched_at` | the category's `fetchedAt` in `manifest.json` (exchange lines carry no sample time) |
| `listing_count` | `listingCount` on `item-*` lines, `receive.listing_count` on `currency-*` lines; `None` for `exchange-*` lines, which report `volumePrimaryValue` instead |
| `divine_rate` | `1 / core.rates.divine` of an `exchange-*` file (poe.ninja gives divine per chaos), or `chaosEquivalent` of `Divine Orb` in `currency-Currency` |
| `SkillGem { name, level, quality, corrupted }` | `item-SkillGem` `name`, `gemLevel`, `gemQuality`, `corrupted` (`null` means not corrupted) |
| `Unique { name, base_type }` | `item-Unique*` `name` and `baseType`. poe.ninja splits some uniques into rows by `links` and mod `variant`, which the key doesn't carry, so the adapter must pick one row per key deterministically. `lootwise-sources` picks the unlinked row, then the most listed, then the cheapest. |
| `Currency`/`Fragment`/`Scarab`/`Essence`/`DivinationCard { name }` | the `exchange-*` file's `items[].name`, joined to `lines[]` by `id`; `primaryValue` is in chaos. Names missing from the exchange fall back to `currency-*` `chaosEquivalent` and `item-Vial`/`item-Incubator` `chaosValue`. |

Tests drive the engine API only, with fake ports and real clipboard fixtures in `tests/fixtures/`.
