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
- **Valuation**: chaos value per unit (the stored unit), divine value at the League's divine rate, source, age of the feed data, listing count and confidence. Confidence is `Low` when there are fewer than 10 listings or the data is older than 1 hour.

## Ports

- `PriceFeed`: `price(league, commodity) -> Option<PriceEntry>` and `divine_rate(league) -> Option<f64>` (chaos per divine). A `PriceEntry` is one normalised price: chaos value, listing count, fetch time and source. Every call is scoped to one League; prices are never compared across Leagues. Adapters (the price relay, later Plugins) implement this trait.
- `Clock`: `now() -> SystemTime`.

Tests drive the engine API only, with fake ports and real clipboard fixtures in `tests/fixtures/`.
