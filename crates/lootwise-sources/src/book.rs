//! Normalising relayed poe.ninja overviews into one League's prices (the mapping table in
//! `crates/lootwise-core/README.md`). Unknown fields are ignored; lines missing a field the
//! mapping needs are skipped.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::SystemTime;

use lootwise_core::{CommodityItem, PriceEntry};
use serde::Deserialize;

/// The source every relayed price is attributed to.
pub const SOURCE: &str = "poe.ninja";

/// `item-*` overviews of stackable currency priced by name alone, besides the `exchange-*`
/// and `currency-*` ones.
const NAMED_ITEM_CATEGORIES: &[&str] = &["item-Vial", "item-Incubator"];

/// Whether the feed reads `category` (a relay object name such as `exchange-Currency`).
/// Other categories are neither downloaded nor normalised.
pub fn is_priced_category(category: &str) -> bool {
    category.starts_with("exchange-")
        || category.starts_with("currency-")
        || category == "item-SkillGem"
        || category.starts_with("item-Unique")
        || NAMED_ITEM_CATEGORIES.contains(&category)
}

/// Which of several poe.ninja rows for one key wins; the smallest rank does.
type Rank = (u8, bool, Reverse<u32>, OrdF64, String);

#[derive(Debug, Clone, Copy, PartialEq)]
struct OrdF64(f64);
impl Eq for OrdF64 {}
impl PartialOrd for OrdF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrdF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// The normalised prices of one League.
#[derive(Debug, Default, Clone)]
pub struct PriceBook {
    /// Currency, fragments, scarabs, essences, divination cards and other stackables.
    named: HashMap<String, (Rank, PriceEntry)>,
    gems: HashMap<(String, u8, u8, bool), (Rank, PriceEntry)>,
    uniques: HashMap<(String, String), (Rank, PriceEntry)>,
    /// Chaos per divine: from an `exchange-*` file, else `currency-Currency`.
    divine_rate: Option<(u8, f64)>,
}

impl PriceBook {
    pub fn price(&self, item: &CommodityItem) -> Option<PriceEntry> {
        let found = match item {
            CommodityItem::Currency { name }
            | CommodityItem::Fragment { name }
            | CommodityItem::Scarab { name }
            | CommodityItem::Essence { name }
            | CommodityItem::DivinationCard { name } => self.named.get(name),
            CommodityItem::SkillGem {
                name,
                level,
                quality,
                corrupted,
            } => self.gems.get(&(name.clone(), *level, *quality, *corrupted)),
            CommodityItem::Unique { name, base_type } => {
                self.uniques.get(&(name.clone(), base_type.clone()))
            }
        };
        found.map(|(_, entry)| entry.clone())
    }

    pub fn divine_rate(&self) -> Option<f64> {
        self.divine_rate.map(|(_, rate)| rate)
    }

    /// Adds one relayed category file, fetched (or confirmed unchanged) at `fetched_at`.
    /// The result does not depend on the order categories are added in.
    pub fn add_category(
        &mut self,
        category: &str,
        json: &[u8],
        fetched_at: SystemTime,
    ) -> Result<(), serde_json::Error> {
        let entry = |chaos_value: f64, listing_count: Option<u32>| PriceEntry {
            chaos_value,
            listing_count,
            fetched_at,
            source: SOURCE.to_owned(),
        };
        if category.starts_with("exchange-") {
            let file: ExchangeFile = serde_json::from_slice(json)?;
            // Values are in the file's primary currency; only chaos is understood.
            if file.core.primary.as_deref().is_some_and(|p| p != "chaos") {
                return Ok(());
            }
            if let Some(divine_per_chaos) = file.core.rates.divine.filter(|r| *r > 0.0) {
                offer_rate(&mut self.divine_rate, 0, 1.0 / divine_per_chaos);
            }
            let names: HashMap<&str, &str> = file
                .items
                .iter()
                .map(|i| (i.id.as_str(), i.name.as_str()))
                .collect();
            for line in &file.lines {
                let (Some(name), Some(chaos)) = (names.get(line.id.as_str()), line.primary_value)
                else {
                    continue;
                };
                let rank = (0, false, Reverse(0), OrdF64(chaos), String::new());
                offer(
                    &mut self.named,
                    (*name).to_owned(),
                    rank,
                    entry(chaos, None),
                );
            }
        } else if category.starts_with("currency-") {
            let file: CurrencyFile = serde_json::from_slice(json)?;
            for line in &file.lines {
                let Some(chaos) = line.chaos_equivalent else {
                    continue;
                };
                if category == "currency-Currency" && line.currency_type_name == "Divine Orb" {
                    offer_rate(&mut self.divine_rate, 1, chaos);
                }
                let listings = line.receive.as_ref().and_then(|r| r.listing_count);
                let rank = (
                    1,
                    false,
                    Reverse(listings.unwrap_or(0)),
                    OrdF64(chaos),
                    String::new(),
                );
                offer(
                    &mut self.named,
                    line.currency_type_name.clone(),
                    rank,
                    entry(chaos, listings),
                );
            }
        } else {
            let file: ItemFile = serde_json::from_slice(json)?;
            for line in file.lines {
                let Some(chaos) = line.chaos_value else {
                    continue;
                };
                // Prefer the unlinked row (poe.ninja splits some uniques by links), then
                // the most listed, then the cheapest; the variant breaks remaining ties.
                let rank = (
                    2,
                    line.links.is_some(),
                    Reverse(line.listing_count.unwrap_or(0)),
                    OrdF64(chaos),
                    line.variant.clone().unwrap_or_default(),
                );
                let price = entry(chaos, line.listing_count);
                if category == "item-SkillGem" {
                    let Some(level) = line.gem_level else {
                        continue;
                    };
                    let key = (
                        line.name,
                        level,
                        line.gem_quality.unwrap_or(0),
                        line.corrupted.unwrap_or(false),
                    );
                    offer(&mut self.gems, key, rank, price);
                } else if category.starts_with("item-Unique") {
                    let Some(base_type) = line.base_type else {
                        continue;
                    };
                    offer(&mut self.uniques, (line.name, base_type), rank, price);
                } else {
                    offer(&mut self.named, line.name, rank, price);
                }
            }
        }
        Ok(())
    }
}

fn offer<K: std::hash::Hash + Eq>(
    map: &mut HashMap<K, (Rank, PriceEntry)>,
    key: K,
    rank: Rank,
    entry: PriceEntry,
) {
    match map.entry(key) {
        Entry::Vacant(slot) => {
            slot.insert((rank, entry));
        }
        Entry::Occupied(mut slot) => {
            if rank < slot.get().0 {
                slot.insert((rank, entry));
            }
        }
    }
}

/// Keeps the rate from the most preferred source (lowest `priority`), then the lowest rate.
fn offer_rate(current: &mut Option<(u8, f64)>, priority: u8, rate: f64) {
    let better = match *current {
        None => true,
        Some((p, r)) => (priority, OrdF64(rate)) < (p, OrdF64(r)),
    };
    if better {
        *current = Some((priority, rate));
    }
}

#[derive(Deserialize)]
struct ExchangeFile {
    core: ExchangeCore,
    #[serde(default)]
    lines: Vec<ExchangeLine>,
    #[serde(default)]
    items: Vec<ExchangeItem>,
}

#[derive(Deserialize)]
struct ExchangeCore {
    #[serde(default)]
    rates: ExchangeRates,
    primary: Option<String>,
}

#[derive(Deserialize, Default)]
struct ExchangeRates {
    divine: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeLine {
    id: String,
    primary_value: Option<f64>,
}

#[derive(Deserialize)]
struct ExchangeItem {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct CurrencyFile {
    #[serde(default)]
    lines: Vec<CurrencyLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrencyLine {
    currency_type_name: String,
    chaos_equivalent: Option<f64>,
    receive: Option<CurrencySample>,
}

#[derive(Deserialize)]
struct CurrencySample {
    listing_count: Option<u32>,
}

#[derive(Deserialize)]
struct ItemFile {
    #[serde(default)]
    lines: Vec<ItemLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemLine {
    name: String,
    base_type: Option<String>,
    chaos_value: Option<f64>,
    listing_count: Option<u32>,
    gem_level: Option<u8>,
    gem_quality: Option<u8>,
    corrupted: Option<bool>,
    links: Option<u8>,
    variant: Option<String>,
}
