//! Price Checks through the engine API, with a fake `PriceFeed` and `Clock`.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use lootwise_core::{
    Clock, CommodityItem, Confidence, Engine, League, PriceCheck, PriceEntry, PriceFeed, Rarity,
};

const CURRENCY_STACK: &str = include_str!("fixtures/currency_stack.txt");
const SCARAB: &str = include_str!("fixtures/scarab.txt");
const DIVINATION_CARD: &str = include_str!("fixtures/divination_card.txt");
const UNIQUE: &str = include_str!("fixtures/unique.txt");
const TRANSFIGURED_GEM: &str = include_str!("fixtures/transfigured_gem_20_20_corrupted.txt");
const RARE: &str = include_str!("fixtures/rare.txt");
const MAGIC: &str = include_str!("fixtures/magic.txt");
const NOT_AN_ITEM: &str = include_str!("fixtures/not_an_item.txt");

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

struct FixedClock(SystemTime);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

#[derive(Default)]
struct FakeFeed {
    prices: HashMap<(League, CommodityItem), PriceEntry>,
    divine_rates: HashMap<League, f64>,
}

impl FakeFeed {
    fn with_price(mut self, league: &League, item: CommodityItem, entry: PriceEntry) -> Self {
        self.prices.insert((league.clone(), item), entry);
        self
    }

    fn with_divine_rate(mut self, league: &League, chaos_per_divine: f64) -> Self {
        self.divine_rates.insert(league.clone(), chaos_per_divine);
        self
    }
}

impl PriceFeed for FakeFeed {
    fn covers(&self, league: &League) -> bool {
        self.prices.keys().any(|(l, _)| l == league)
    }

    fn price(&self, league: &League, item: &CommodityItem) -> Option<PriceEntry> {
        self.prices.get(&(league.clone(), item.clone())).cloned()
    }

    fn divine_rate(&self, league: &League) -> Option<f64> {
        self.divine_rates.get(league).copied()
    }
}

fn settlers() -> League {
    League::new("Settlers")
}

fn entry(chaos_value: f64, listing_count: u32, fetched_at: SystemTime) -> PriceEntry {
    PriceEntry {
        chaos_value,
        listing_count: Some(listing_count),
        fetched_at,
        source: "poe.ninja".into(),
    }
}

#[test]
fn currency_stack_is_valued_from_the_feed() {
    let league = settlers();
    let divine = CommodityItem::Currency {
        name: "Divine Orb".into(),
    };
    let feed = FakeFeed::default()
        .with_price(&league, divine.clone(), entry(180.0, 2_500, t0()))
        .with_divine_rate(&league, 180.0);
    let engine = Engine::new(feed, FixedClock(t0() + Duration::from_secs(600)));

    let PriceCheck::Valued { item, valuation } = engine.price_check(&league, CURRENCY_STACK) else {
        panic!("expected a Valuation");
    };

    assert_eq!(item.item_class, "Stackable Currency");
    assert_eq!(item.rarity, Rarity::Currency);
    assert_eq!(item.name, "Divine Orb");
    assert_eq!(item.stack_size, Some(7));
    assert_eq!(valuation.item, divine);
    assert_eq!(valuation.chaos_value, 180.0);
    assert_eq!(valuation.divine_value, Some(1.0));
    assert_eq!(valuation.source, "poe.ninja");
    assert_eq!(valuation.age, Duration::from_secs(600));
    assert_eq!(valuation.listing_count, Some(2_500));
    assert_eq!(valuation.confidence, Confidence::High);
}

fn divine_orb() -> CommodityItem {
    CommodityItem::Currency {
        name: "Divine Orb".into(),
    }
}

/// Price Checks `text` in `league` against a feed holding only `item` at `entry`.
fn check_with(
    league: &League,
    item: CommodityItem,
    entry: PriceEntry,
    now: SystemTime,
    text: &str,
) -> PriceCheck {
    let feed = FakeFeed::default()
        .with_price(league, item, entry)
        .with_divine_rate(league, 200.0);
    Engine::new(feed, FixedClock(now)).price_check(league, text)
}

fn valued(check: PriceCheck) -> lootwise_core::Valuation {
    match check {
        PriceCheck::Valued { valuation, .. } => valuation,
        other => panic!("expected a Valuation, got {other:?}"),
    }
}

#[test]
fn scarab_is_a_commodity_item() {
    let league = settlers();
    let scarab = CommodityItem::Scarab {
        name: "Ambush Scarab of Containment".into(),
    };
    let check = check_with(
        &league,
        scarab.clone(),
        entry(50.0, 300, t0()),
        t0(),
        SCARAB,
    );

    let PriceCheck::Valued { item, valuation } = check else {
        panic!("expected a Valuation, got {check:?}");
    };
    assert_eq!(item.stack_size, Some(3));
    assert_eq!(valuation.item, scarab);
    assert_eq!(valuation.divine_value, Some(0.25));
}

#[test]
fn divination_card_is_keyed_by_its_own_name_not_its_reward() {
    let league = settlers();
    let card = CommodityItem::DivinationCard {
        name: "The Doctor".into(),
    };
    let check = check_with(
        &league,
        card.clone(),
        entry(1_000.0, 40, t0()),
        t0(),
        DIVINATION_CARD,
    );

    let PriceCheck::Valued { item, valuation } = check else {
        panic!("expected a Valuation, got {check:?}");
    };
    assert_eq!(item.rarity, Rarity::DivinationCard);
    assert_eq!(item.stack_size, Some(2));
    assert_eq!(item.base_type, None);
    assert_eq!(valuation.item, card);
}

#[test]
fn unique_is_keyed_by_name_and_base_type() {
    let league = settlers();
    let tabula = CommodityItem::Unique {
        name: "Tabula Rasa".into(),
        base_type: "Simple Robe".into(),
    };
    let valuation = valued(check_with(
        &league,
        tabula.clone(),
        entry(12.0, 900, t0()),
        t0(),
        UNIQUE,
    ));

    assert_eq!(valuation.item, tabula);
    assert_eq!(valuation.chaos_value, 12.0);
}

#[test]
fn transfigured_gem_is_keyed_by_name_level_quality_and_corruption() {
    let league = settlers();
    let gem = CommodityItem::SkillGem {
        name: "Cleave of Rage".into(),
        level: 20,
        quality: 20,
        corrupted: true,
    };
    let check = check_with(
        &league,
        gem.clone(),
        entry(30.0, 15, t0()),
        t0(),
        TRANSFIGURED_GEM,
    );

    let PriceCheck::Valued { item, valuation } = check else {
        panic!("expected a Valuation, got {check:?}");
    };
    assert_eq!(item.item_class, "Skill Gems");
    assert_eq!(item.rarity, Rarity::Gem);
    assert_eq!(item.gem_level, Some(20));
    assert_eq!(item.gem_quality, Some(20));
    assert!(item.corrupted);
    assert_eq!(valuation.item, gem);
}

#[test]
fn gem_variant_with_different_corruption_has_no_price() {
    let league = settlers();
    let uncorrupted = CommodityItem::SkillGem {
        name: "Cleave of Rage".into(),
        level: 20,
        quality: 20,
        corrupted: false,
    };
    let check = check_with(
        &league,
        uncorrupted,
        entry(30.0, 15, t0()),
        t0(),
        TRANSFIGURED_GEM,
    );

    assert!(matches!(check, PriceCheck::NoPrice { .. }), "got {check:?}");
}

#[test]
fn rare_is_not_a_commodity_item() {
    let check = check_with(
        &settlers(),
        divine_orb(),
        entry(200.0, 100, t0()),
        t0(),
        RARE,
    );

    let PriceCheck::NotACommodityItem { item } = check else {
        panic!("expected NotACommodityItem, got {check:?}");
    };
    assert_eq!(item.rarity, Rarity::Rare);
    assert_eq!(item.name, "Storm Loop");
    assert_eq!(item.base_type.as_deref(), Some("Amethyst Ring"));
    assert_eq!(item.gem_level, None);
}

#[test]
fn magic_is_not_a_commodity_item() {
    let check = check_with(
        &settlers(),
        divine_orb(),
        entry(200.0, 100, t0()),
        t0(),
        MAGIC,
    );

    let PriceCheck::NotACommodityItem { item } = check else {
        panic!("expected NotACommodityItem, got {check:?}");
    };
    assert_eq!(item.rarity, Rarity::Magic);
    assert_eq!(item.name, "Experimenter's Divine Life Flask of the Rainbow");
    assert_eq!(item.base_type, None);
}

#[test]
fn non_item_text_is_not_an_item() {
    for text in [
        NOT_AN_ITEM,
        "",
        "Item Class: Rings\nRarity: Shiny\nStorm Loop\n",
    ] {
        let check = check_with(
            &settlers(),
            divine_orb(),
            entry(200.0, 100, t0()),
            t0(),
            text,
        );
        assert_eq!(check, PriceCheck::NotAnItem, "for {text:?}");
    }
}

#[test]
fn unknown_commodity_item_has_no_price() {
    let check = check_with(
        &settlers(),
        divine_orb(),
        entry(200.0, 100, t0()),
        t0(),
        SCARAB,
    );

    let PriceCheck::NoPrice { commodity, .. } = check else {
        panic!("expected NoPrice, got {check:?}");
    };
    assert_eq!(
        commodity,
        CommodityItem::Scarab {
            name: "Ambush Scarab of Containment".into()
        }
    );
}

#[test]
fn nine_listings_give_low_confidence_and_ten_give_high() {
    let league = settlers();
    let nine = valued(check_with(
        &league,
        divine_orb(),
        entry(200.0, 9, t0()),
        t0(),
        CURRENCY_STACK,
    ));
    let ten = valued(check_with(
        &league,
        divine_orb(),
        entry(200.0, 10, t0()),
        t0(),
        CURRENCY_STACK,
    ));

    assert_eq!(nine.listing_count, Some(9));
    assert_eq!(nine.confidence, Confidence::Low);
    assert_eq!(ten.confidence, Confidence::High);
}

#[test]
fn a_price_without_a_listing_count_is_judged_by_age_alone() {
    // Currency Exchange overviews report trade volume, not listings.
    let league = settlers();
    let hour = Duration::from_secs(3_600);
    let unlisted = |now| {
        let entry = PriceEntry {
            listing_count: None,
            ..entry(200.0, 0, t0())
        };
        valued(check_with(
            &league,
            divine_orb(),
            entry,
            now,
            CURRENCY_STACK,
        ))
    };
    let fresh = unlisted(t0());
    let stale = unlisted(t0() + hour + Duration::from_secs(1));

    assert_eq!(fresh.listing_count, None);
    assert_eq!(fresh.confidence, Confidence::High);
    assert_eq!(stale.confidence, Confidence::Low);
}

#[test]
fn data_just_under_an_hour_old_is_confident_and_just_over_is_not() {
    let league = settlers();
    let hour = Duration::from_secs(3_600);
    let second = Duration::from_secs(1);
    let under = valued(check_with(
        &league,
        divine_orb(),
        entry(200.0, 100, t0()),
        t0() + hour - second,
        CURRENCY_STACK,
    ));
    let over = valued(check_with(
        &league,
        divine_orb(),
        entry(200.0, 100, t0()),
        t0() + hour + second,
        CURRENCY_STACK,
    ));

    assert_eq!(under.age, hour - second);
    assert_eq!(under.confidence, Confidence::High);
    assert_eq!(over.age, hour + second);
    assert_eq!(over.confidence, Confidence::Low);
}

#[test]
fn prices_and_divine_rate_come_only_from_the_checked_league() {
    let settlers = settlers();
    let standard = League::new("Standard");
    let scarab = CommodityItem::Scarab {
        name: "Ambush Scarab of Containment".into(),
    };
    let feed = FakeFeed::default()
        .with_price(&standard, scarab.clone(), entry(5.0, 100, t0()))
        .with_divine_rate(&standard, 100.0)
        .with_price(&settlers, divine_orb(), entry(200.0, 100, t0()))
        .with_divine_rate(&settlers, 200.0);
    let engine = Engine::new(feed, FixedClock(t0()));

    assert!(matches!(
        engine.price_check(&settlers, SCARAB),
        PriceCheck::NoPrice { .. }
    ));
    let in_standard = valued(engine.price_check(&standard, SCARAB));
    assert_eq!(in_standard.chaos_value, 5.0);
    assert_eq!(in_standard.divine_value, Some(0.05));
    assert!(matches!(
        engine.price_check(&standard, CURRENCY_STACK),
        PriceCheck::NoPrice { .. }
    ));
}

#[test]
fn a_league_the_feed_does_not_cover_has_prices_unavailable_not_no_price() {
    let standard = League::new("Standard");
    let feed = FakeFeed::default().with_price(&standard, divine_orb(), entry(200.0, 100, t0()));
    let engine = Engine::new(feed, FixedClock(t0()));

    let check = engine.price_check(&League::new("Standrad"), CURRENCY_STACK);
    let PriceCheck::PricesUnavailable { commodity, .. } = check else {
        panic!("expected PricesUnavailable, got {check:?}");
    };
    assert_eq!(commodity, divine_orb());
    assert_eq!(
        engine.price_check(&League::new("Standrad"), RARE),
        engine.price_check(&standard, RARE),
        "a non-Commodity Item reads the same in any League"
    );
}

#[test]
fn divine_value_is_absent_without_a_divine_rate() {
    let league = settlers();
    let feed = FakeFeed::default().with_price(&league, divine_orb(), entry(200.0, 100, t0()));
    let valuation =
        valued(Engine::new(feed, FixedClock(t0())).price_check(&league, CURRENCY_STACK));

    assert_eq!(valuation.divine_value, None);
}

#[test]
fn essence_and_fragment_are_commodity_items() {
    let league = settlers();
    let essence = "Item Class: Stackable Currency\nRarity: Currency\nDeafening Essence of Greed\n--------\nStack Size: 4/9\n--------\nUpgrades a normal item to rare quality with one guaranteed property\n";
    let fragment = "Item Class: Map Fragments\nRarity: Normal\nFragment of the Hydra\n--------\nCan be used in a personal Map Device.\n";
    let essence_key = CommodityItem::Essence {
        name: "Deafening Essence of Greed".into(),
    };
    let fragment_key = CommodityItem::Fragment {
        name: "Fragment of the Hydra".into(),
    };

    assert_eq!(
        valued(check_with(
            &league,
            essence_key.clone(),
            entry(3.0, 50, t0()),
            t0(),
            essence
        ))
        .item,
        essence_key
    );
    assert_eq!(
        valued(check_with(
            &league,
            fragment_key.clone(),
            entry(40.0, 50, t0()),
            t0(),
            fragment
        ))
        .item,
        fragment_key
    );
}

#[test]
fn windows_line_endings_parse_the_same() {
    let crlf = CURRENCY_STACK.replace('\n', "\r\n");
    let valuation = valued(check_with(
        &settlers(),
        divine_orb(),
        entry(200.0, 100, t0()),
        t0(),
        &crlf,
    ));

    assert_eq!(valuation.item, divine_orb());
}

#[test]
fn unidentified_unique_is_not_a_commodity_item() {
    let unidentified = "Item Class: Body Armours\nRarity: Unique\nSimple Robe\n--------\nItem Level: 84\n--------\nUnidentified\n";
    let tabula = CommodityItem::Unique {
        name: "Tabula Rasa".into(),
        base_type: "Simple Robe".into(),
    };
    let check = check_with(
        &settlers(),
        tabula,
        entry(12.0, 900, t0()),
        t0(),
        unidentified,
    );

    assert!(
        matches!(check, PriceCheck::NotACommodityItem { .. }),
        "got {check:?}"
    );
}

#[test]
fn splinter_is_a_fragment() {
    let splinter = "Item Class: Stackable Currency\nRarity: Currency\nSplinter of Xoph\n--------\nStack Size: 37/100\n--------\nCombine 100 Splinters to create Xoph's Breachstone.\n";
    let key = CommodityItem::Fragment {
        name: "Splinter of Xoph".into(),
    };
    let valuation = valued(check_with(
        &settlers(),
        key.clone(),
        entry(1.5, 200, t0()),
        t0(),
        splinter,
    ));

    assert_eq!(valuation.item, key);
}
