//! Parsing of PoE1 English clipboard item text (the `Item Class:` / `Rarity:` / `--------` format).

const SECTION_SEPARATOR: &str = "--------";

/// An item as copied from the game: the facts Lootwise reads off its clipboard text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The `Item Class:` value, e.g. `Stackable Currency`, `Skill Gems`, `Body Armours`.
    pub item_class: String,
    pub rarity: Rarity,
    /// The first name line: the unique or rare name, the full magic name, or the item's
    /// only name (currency, gems, divination cards, normal items). Transfigured gems keep
    /// their transfigured name, e.g. `Cleave of Rage`.
    pub name: String,
    /// The second name line, present only on identified rare and unique items.
    pub base_type: Option<String>,
    /// Items in the stack, for stackable items.
    pub stack_size: Option<u32>,
    /// Gem level, for gems.
    pub gem_level: Option<u8>,
    /// Gem quality in percent, for gems with quality.
    pub gem_quality: Option<u8>,
    pub corrupted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
    Normal,
    Magic,
    Rare,
    Unique,
    Currency,
    Gem,
    DivinationCard,
    Quest,
}

impl Rarity {
    fn from_clipboard(value: &str) -> Option<Self> {
        Some(match value {
            "Normal" => Self::Normal,
            "Magic" => Self::Magic,
            "Rare" => Self::Rare,
            "Unique" => Self::Unique,
            "Currency" => Self::Currency,
            "Gem" => Self::Gem,
            "Divination Card" => Self::DivinationCard,
            "Quest" => Self::Quest,
            _ => return None,
        })
    }
}

/// Clipboard text that is not a PoE1 item: unknown, garbled or empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAnItem;

/// Parses clipboard text into an [`Item`].
pub fn parse_item(text: &str) -> Result<Item, NotAnItem> {
    let mut sections = sections(text);
    let header = sections.next().ok_or(NotAnItem)?;
    let mut header = header.into_iter();

    let item_class = header
        .next()
        .and_then(|line| line.strip_prefix("Item Class: "))
        .filter(|class| !class.is_empty())
        .ok_or(NotAnItem)?;
    let rarity = header
        .next()
        .and_then(|line| line.strip_prefix("Rarity: "))
        .and_then(Rarity::from_clipboard)
        .ok_or(NotAnItem)?;
    let name = header.next().ok_or(NotAnItem)?;
    let base_type = header.next();
    if header.next().is_some() {
        return Err(NotAnItem);
    }

    let mut item = Item {
        item_class: item_class.to_owned(),
        rarity,
        name: name.to_owned(),
        base_type: base_type.map(str::to_owned),
        stack_size: None,
        gem_level: None,
        gem_quality: None,
        corrupted: false,
    };

    for section in sections {
        // Requirement sections also hold a `Level:` line: the character level, not the gem's.
        let is_requirements = section.first() == Some(&"Requirements:");
        for line in section {
            if line == "Corrupted" {
                item.corrupted = true;
            } else if let Some(value) = line.strip_prefix("Stack Size: ") {
                item.stack_size = Some(parse_stack_size(value).ok_or(NotAnItem)?);
            } else if rarity == Rarity::Gem && !is_requirements {
                if let Some(value) = line.strip_prefix("Level: ") {
                    item.gem_level = Some(leading_number(value).ok_or(NotAnItem)?);
                } else if let Some(value) = line.strip_prefix("Quality: +") {
                    item.gem_quality = Some(leading_number(value).ok_or(NotAnItem)?);
                }
            }
        }
    }

    Ok(item)
}

/// Splits text into its `--------`-separated sections of trimmed, non-empty lines.
fn sections(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.split(SECTION_SEPARATOR).filter_map(|section| {
        let lines: Vec<&str> = section
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        (!lines.is_empty()).then_some(lines)
    })
}

/// `7/20` or `1,234/5,000` → the count before the slash.
fn parse_stack_size(value: &str) -> Option<u32> {
    let (count, _max) = value.split_once('/')?;
    count.replace(',', "").trim().parse().ok()
}

/// `20 (Max)` or `20% (augmented)` → `20`.
fn leading_number<T: std::str::FromStr>(value: &str) -> Option<T> {
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    value[..end].parse().ok()
}
