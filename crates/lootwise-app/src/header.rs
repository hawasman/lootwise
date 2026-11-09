//! The header of Path of Exile's copied item text: its rarity and name lines. Raw, on purpose;
//! real item parsing belongs to `lootwise-core`.
//!
//! ```text
//! Item Class: Rings
//! Rarity: Rare
//! Doom Loop
//! Two-Stone Ring
//! --------
//! ...
//! ```

/// The rarity and the one or two name lines that follow it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemHeader {
    pub rarity: String,
    pub name: Vec<String>,
}

const SEPARATOR: &str = "--------";

/// `None` when the text isn't Path of Exile item text.
pub fn parse_header(text: &str) -> Option<ItemHeader> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .take_while(|line| *line != SEPARATOR);
    let rarity = lines
        .find_map(|line| line.strip_prefix("Rarity: "))?
        .trim()
        .to_owned();
    let name: Vec<String> = lines
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    (!rarity.is_empty() && !name.is_empty() && text.lines().any(|line| line.trim() == SEPARATOR))
        .then_some(ItemHeader { rarity, name })
}
