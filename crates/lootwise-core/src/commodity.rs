//! Commodity Items: items valued by their identity alone, from aggregated market data.

use crate::item::{Item, Rarity};

/// The identity of a Commodity Item: the key a [`crate::PriceFeed`] prices it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommodityItem {
    Currency {
        name: String,
    },
    Fragment {
        name: String,
    },
    Scarab {
        name: String,
    },
    Essence {
        name: String,
    },
    DivinationCard {
        name: String,
    },
    /// A skill or support gem, including transfigured gems (keyed by their transfigured name).
    SkillGem {
        name: String,
        level: u8,
        /// Quality in percent; 0 when the gem has none.
        quality: u8,
        corrupted: bool,
    },
    /// An identified unique, by name and base type (one variant of the unique).
    Unique {
        name: String,
        base_type: String,
    },
}

impl CommodityItem {
    /// Classifies an item as a Commodity Item, or `None` when valuing it would need a
    /// Live Search (rares, magic and normal equipment, unidentified uniques, ...).
    pub fn classify(item: &Item) -> Option<Self> {
        let name = item.name.clone();
        match (item.rarity, item.item_class.as_str()) {
            (Rarity::Currency, "Stackable Currency") if is_splinter(&name) => {
                Some(Self::Fragment { name })
            }
            (Rarity::Currency, "Stackable Currency") if is_essence(&name) => {
                Some(Self::Essence { name })
            }
            (Rarity::Currency, "Stackable Currency") => Some(Self::Currency { name }),
            (_, "Map Fragments") if name.contains("Scarab") => Some(Self::Scarab { name }),
            (_, "Map Fragments") => Some(Self::Fragment { name }),
            (Rarity::DivinationCard, _) => Some(Self::DivinationCard { name }),
            (Rarity::Gem, _) => Some(Self::SkillGem {
                name,
                level: item.gem_level?,
                quality: item.gem_quality.unwrap_or(0),
                corrupted: item.corrupted,
            }),
            (Rarity::Unique, _) => Some(Self::Unique {
                name,
                base_type: item.base_type.clone()?,
            }),
            _ => None,
        }
    }
}

fn is_essence(name: &str) -> bool {
    name.contains("Essence of") || name == "Remnant of Corruption"
}

/// Splinters (`Splinter of Xoph`, `Timeless Karui Splinter`) stack like currency but are
/// fragments: pieces of a Map Fragment.
fn is_splinter(name: &str) -> bool {
    name.contains("Splinter")
}
