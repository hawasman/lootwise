//! Lootwise domain engine: parses clipboard item text, classifies Commodity Items and
//! values them for a Price Check in one League. No I/O and no UI: prices and time come in
//! through the [`PriceFeed`] and [`Clock`] ports.

mod commodity;
mod item;

use std::time::{Duration, SystemTime};

pub use commodity::CommodityItem;
pub use item::{Item, NotAnItem, Rarity, parse_item};

/// Feed data older than this gives a low-confidence Valuation.
pub const MAX_CONFIDENT_AGE: Duration = Duration::from_secs(60 * 60);
/// Fewer listings than this give a low-confidence Valuation.
pub const MIN_CONFIDENT_LISTINGS: u32 = 10;

/// The economy an item or price belongs to; prices are never compared across Leagues.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct League(String);

impl League {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

/// One normalised price of a Commodity Item in a League.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceEntry {
    /// Price of one unit, in chaos orbs.
    pub chaos_value: f64,
    /// Listings the price was aggregated from.
    pub listing_count: u32,
    /// When the price source produced this price.
    pub fetched_at: SystemTime,
    /// Where the price came from, e.g. `poe.ninja`.
    pub source: String,
}

/// Port: aggregated market prices, scoped to one League per call.
pub trait PriceFeed {
    /// The price of `item` in `league`, or `None` when the feed has no price for it.
    fn price(&self, league: &League, item: &CommodityItem) -> Option<PriceEntry>;

    /// Chaos orbs per divine orb in `league`, or `None` when unknown.
    fn divine_rate(&self, league: &League) -> Option<f64>;
}

/// Port: the current time.
pub trait Clock {
    fn now(&self) -> SystemTime;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    /// Fewer than [`MIN_CONFIDENT_LISTINGS`] listings, or feed data older than
    /// [`MAX_CONFIDENT_AGE`].
    Low,
}

/// The value Lootwise assigns to a Commodity Item, with where it came from and how
/// confident it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Valuation {
    pub item: CommodityItem,
    /// Value of one unit in chaos orbs, the stored unit.
    pub chaos_value: f64,
    /// Value of one unit in divine orbs at the League's divine rate, when the feed has one.
    pub divine_value: Option<f64>,
    pub source: String,
    /// Age of the feed data at the time of the Price Check.
    pub age: Duration,
    pub listing_count: u32,
    pub confidence: Confidence,
}

/// The outcome of a Price Check.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceCheck {
    /// The text is not a PoE1 item.
    NotAnItem,
    /// The item is not a Commodity Item; valuing it would need a Live Search.
    NotACommodityItem {
        item: Item,
    },
    /// A Commodity Item the feed has no price for in this League.
    NoPrice {
        item: Item,
        commodity: CommodityItem,
    },
    Valued {
        item: Item,
        valuation: Valuation,
    },
}

/// The engine the app calls for a Price Check.
pub struct Engine<F, C> {
    feed: F,
    clock: C,
}

impl<F: PriceFeed, C: Clock> Engine<F, C> {
    pub fn new(feed: F, clock: C) -> Self {
        Self { feed, clock }
    }

    /// Price Checks the item in `clipboard` text against `league`'s prices.
    pub fn price_check(&self, league: &League, clipboard: &str) -> PriceCheck {
        let Ok(item) = parse_item(clipboard) else {
            return PriceCheck::NotAnItem;
        };
        let Some(commodity) = CommodityItem::classify(&item) else {
            return PriceCheck::NotACommodityItem { item };
        };
        let Some(entry) = self.feed.price(league, &commodity) else {
            return PriceCheck::NoPrice { item, commodity };
        };
        let valuation = self.value(league, commodity, entry);
        PriceCheck::Valued { item, valuation }
    }

    fn value(&self, league: &League, item: CommodityItem, entry: PriceEntry) -> Valuation {
        // Data stamped in the future (clock skew) counts as fresh.
        let age = self
            .clock
            .now()
            .duration_since(entry.fetched_at)
            .unwrap_or_default();
        let confidence = if entry.listing_count < MIN_CONFIDENT_LISTINGS || age > MAX_CONFIDENT_AGE
        {
            Confidence::Low
        } else {
            Confidence::High
        };
        let divine_value = self
            .feed
            .divine_rate(league)
            .filter(|rate| *rate > 0.0)
            .map(|rate| entry.chaos_value / rate);
        Valuation {
            item,
            chaos_value: entry.chaos_value,
            divine_value,
            source: entry.source,
            age,
            listing_count: entry.listing_count,
            confidence,
        }
    }
}
