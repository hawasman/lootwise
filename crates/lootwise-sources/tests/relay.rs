//! The relay-backed feed against a recorded relay folder (trimmed from `pnpm dev:mirror`) and
//! a local HTTP server serving it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use lootwise_core::{
    Clock, CommodityItem, Confidence, Engine, League, PriceCheck, PriceEntry, PriceFeed,
};
use lootwise_sources::{PriceBook, RelayFeed, RelaySource, SOURCE, load, sync};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relay")
}

fn standard() -> League {
    League::new("Standard")
}

fn at(rfc3339: &str) -> SystemTime {
    humantime::parse_rfc3339(rfc3339).unwrap()
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Syncs the fixture folder into a fresh cache and loads it.
fn synced_book() -> (tempfile::TempDir, PriceBook) {
    let cache = tempfile::tempdir().unwrap();
    let source = RelaySource::Folder(fixture_root());
    block_on(sync(&client(), &source, cache.path(), &standard())).unwrap();
    let book = load(cache.path(), &standard());
    (cache, book)
}

fn named(kind: fn(String) -> CommodityItem, name: &str) -> CommodityItem {
    kind(name.to_owned())
}

fn currency(name: String) -> CommodityItem {
    CommodityItem::Currency { name }
}

fn gem(name: &str, level: u8, quality: u8, corrupted: bool) -> CommodityItem {
    CommodityItem::SkillGem {
        name: name.into(),
        level,
        quality,
        corrupted,
    }
}

fn unique(name: &str, base_type: &str) -> CommodityItem {
    CommodityItem::Unique {
        name: name.into(),
        base_type: base_type.into(),
    }
}

fn price(chaos_value: f64, listing_count: Option<u32>, fetched_at: &str) -> Option<PriceEntry> {
    Some(PriceEntry {
        chaos_value,
        listing_count,
        fetched_at: at(fetched_at),
        source: SOURCE.into(),
    })
}

#[test]
fn exchange_lines_are_joined_to_their_items_by_id_and_have_no_listing_count() {
    let (_cache, book) = synced_book();
    assert_eq!(
        book.price(&named(currency, "Divine Orb")),
        price(602.6, None, "2026-11-09T12:35:38.455Z"),
        "the exchange price wins over currency-Currency's"
    );
    assert_eq!(
        book.price(&CommodityItem::Scarab {
            name: "Abyss Scarab of Descending".into()
        }),
        price(17.0, None, "2026-11-09T12:35:38.853Z")
    );
    assert_eq!(
        book.price(&CommodityItem::DivinationCard {
            name: "The Doctor".into()
        }),
        price(9039.0, None, "2026-11-09T12:35:38.771Z")
    );
}

#[test]
fn currency_overview_lines_give_chaos_equivalent_and_receive_listings() {
    let (_cache, book) = synced_book();
    // Not on the exchange, so only the stash currency overview prices it.
    assert_eq!(
        book.price(&named(currency, "Orb of Horizons")),
        price(0.33, Some(967), "2026-11-09T12:35:39.506Z")
    );
}

#[test]
fn the_divine_rate_is_the_inverse_of_the_exchange_rate() {
    let (_cache, book) = synced_book();
    let rate = book.divine_rate().unwrap();
    assert!((rate - 1.0 / 0.001659).abs() < 1e-9, "{rate}");
}

#[test]
fn gems_are_keyed_by_level_quality_and_corruption() {
    let (_cache, book) = synced_book();
    let fetched = "2026-11-09T12:35:39.179Z";
    assert_eq!(
        book.price(&gem("Cleave of Rage", 21, 23, true)),
        price(2410.0, Some(21), fetched)
    );
    assert_eq!(
        book.price(&gem("Cleave of Rage", 20, 20, false)),
        price(52.5, Some(59), fetched),
        "a null corrupted is not corrupted"
    );
    assert_eq!(
        book.price(&gem("Cleave of Rage", 21, 0, true)),
        price(30.0, Some(14), fetched),
        "a null quality is 0"
    );
    assert_eq!(book.price(&gem("Cleave of Rage", 20, 20, true)), None);
}

#[test]
fn uniques_pick_one_row_per_name_and_base_type() {
    let (_cache, book) = synced_book();
    let fetched = "2026-11-09T12:35:39.038Z";
    assert_eq!(
        book.price(&unique("Kaom's Heart", "Glorious Plate")),
        price(602.6, Some(245), fetched)
    );
    assert_eq!(
        book.price(&unique("Foulborn Ghostwrithe", "Silken Vest")),
        price(1627.0, Some(176), fetched),
        "of 11 rows split by links and variant: the unlinked one with the most listings"
    );
    assert_eq!(
        book.price(&unique("Tabula Rasa", "Simple Robe")),
        price(5.0, Some(925), fetched),
        "a unique listed only linked still has a price"
    );
}

/// Serves `root` over HTTP and records every requested path.
struct Relay {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn serve(root: PathBuf) -> Relay {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/prices/", server.server_addr().to_ip().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let path = request.url().trim_start_matches("/prices/").to_owned();
            seen.lock().unwrap().push(path.clone());
            let response = match std::fs::read(root.join(&path)) {
                Ok(body) => tiny_http::Response::from_data(body),
                Err(_) => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
            };
            let _ = request.respond(response);
        }
    });
    Relay { url, requests }
}

impl Relay {
    fn take(&self) -> Vec<String> {
        let mut seen = std::mem::take(&mut *self.requests.lock().unwrap());
        seen.sort();
        seen
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn only_categories_whose_etag_changed_are_downloaded_again() {
    let served = tempfile::tempdir().unwrap();
    copy_dir(&fixture_root(), served.path());
    let relay = serve(served.path().to_owned());
    let source = RelaySource::parse(&relay.url).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let league = standard();

    let first = block_on(sync(&client(), &source, cache.path(), &league)).unwrap();
    assert_eq!(first.downloaded.len(), 6);
    assert_eq!(relay.take().len(), 7, "the manifest and every category");

    let second = block_on(sync(&client(), &source, cache.path(), &league)).unwrap();
    assert!(second.downloaded.is_empty());
    assert_eq!(relay.take(), ["v1/Standard/manifest.json"]);

    // The relay stores a new Scarab overview.
    let dir = served.path().join("v1/Standard");
    let manifest = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let old_etag = "\"etag\": \"";
    let scarab = manifest.find("\"exchange-Scarab\"").unwrap();
    let etag_at = scarab + manifest[scarab..].find(old_etag).unwrap() + old_etag.len();
    let mut changed = manifest.clone();
    changed.replace_range(etag_at..etag_at + 32, &"0".repeat(32));
    std::fs::write(dir.join("manifest.json"), changed).unwrap();

    let third = block_on(sync(&client(), &source, cache.path(), &league)).unwrap();
    assert_eq!(third.downloaded, ["exchange-Scarab"]);
    assert_eq!(
        relay.take(),
        [
            "v1/Standard/exchange-Scarab.json",
            "v1/Standard/manifest.json"
        ]
    );
}

#[test]
fn categories_the_feed_does_not_price_are_not_downloaded() {
    let served = tempfile::tempdir().unwrap();
    copy_dir(&fixture_root(), served.path());
    let dir = served.path().join("v1/Standard");
    let manifest = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    let with_maps = manifest.replacen(
        "\"categories\": {",
        "\"categories\": {\n    \"item-Map\": {\"fetchedAt\": \"2026-11-09T12:35:39.000Z\", \"etag\": \"abc\", \"ok\": true},",
        1,
    );
    std::fs::write(dir.join("manifest.json"), with_maps).unwrap();
    let relay = serve(served.path().to_owned());
    let cache = tempfile::tempdir().unwrap();

    let source = RelaySource::parse(&relay.url).unwrap();
    block_on(sync(&client(), &source, cache.path(), &standard())).unwrap();
    assert!(
        !relay
            .take()
            .contains(&"v1/Standard/item-Map.json".to_owned())
    );
}

struct FixedClock(SystemTime);

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

#[test]
fn an_unreachable_relay_leaves_the_cached_prices_with_their_age() {
    let cache = tempfile::tempdir().unwrap();
    let league = standard();
    block_on(sync(
        &client(),
        &RelaySource::Folder(fixture_root()),
        cache.path(),
        &league,
    ))
    .unwrap();

    // A port nothing listens on.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", closed.local_addr().unwrap());
    drop(closed);
    let unreachable = RelaySource::parse(&url).unwrap();
    assert!(block_on(sync(&client(), &unreachable, cache.path(), &league)).is_err());

    let feed = RelayFeed::start(unreachable, cache.path().to_owned(), league.clone()).unwrap();
    let divine = named(currency, "Divine Orb");
    assert_eq!(
        feed.price(&league, &divine),
        price(602.6, None, "2026-11-09T12:35:38.455Z"),
        "priced from the cache, with the relay's fetch time"
    );
    assert_eq!(feed.price(&League::new("Settlers"), &divine), None);

    let two_hours_later = at("2026-11-09T14:35:38.455Z");
    let engine = Engine::new(feed, FixedClock(two_hours_later));
    let text = "Item Class: Stackable Currency\nRarity: Currency\nDivine Orb\n--------\nStack Size: 1/20\n--------\n";
    let PriceCheck::Valued { valuation, .. } = engine.price_check(&league, text) else {
        panic!("Divine Orb is priced");
    };
    assert_eq!(valuation.age, Duration::from_secs(2 * 60 * 60));
    assert_eq!(valuation.confidence, Confidence::Low);
}

#[test]
fn a_league_name_with_spaces_is_url_encoded() {
    let relay = serve(fixture_root());
    let cache = tempfile::tempdir().unwrap();
    let source = RelaySource::parse(&relay.url).unwrap();
    // The fixture has no such League; only the request matters.
    let _ = block_on(sync(
        &client(),
        &source,
        cache.path(),
        &League::new("Hardcore Standard"),
    ));
    assert_eq!(relay.take(), ["v1/Hardcore%20Standard/manifest.json"]);
}
