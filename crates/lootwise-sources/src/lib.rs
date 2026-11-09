//! Lootwise price sources. [`RelayFeed`] is the `PriceFeed` backed by the Lootwise price relay
//! (ADR 0004): it downloads the relayed poe.ninja files into a disk cache, refreshing at most
//! every [`REFRESH_INTERVAL`], and prices Commodity Items from the cache. Lootwise never calls
//! poe.ninja itself.

mod book;
mod relay;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use lootwise_core::{CommodityItem, League, PriceEntry, PriceFeed};

pub use book::{PriceBook, SOURCE, is_priced_category};
pub use relay::{RelaySource, SyncError, SyncReport, load, sync};

/// How often the feed asks the relay for new prices; the relay itself refreshes this often.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// The cache folder in the platform data dir (`~/.local/share/lootwise/relay` on Linux).
pub fn default_cache_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("lootwise").join("relay"))
}

/// The relay-backed `PriceFeed` for one League.
///
/// Prices come from the disk cache, so they are available at once from an earlier run. A
/// background thread with its own tokio runtime syncs the cache from the relay on start and
/// every [`REFRESH_INTERVAL`]. When the relay is unreachable the cached prices stay, keeping
/// their original fetch time, so their Valuations age into low confidence.
#[derive(Clone)]
pub struct RelayFeed {
    league: League,
    book: Arc<RwLock<PriceBook>>,
}

impl RelayFeed {
    /// Loads `league` from `cache` and starts refreshing it from `source` in the background.
    pub fn start(source: RelaySource, cache: PathBuf, league: League) -> std::io::Result<Self> {
        let feed = Self {
            book: Arc::new(RwLock::new(load(&cache, &league))),
            league,
        };
        let background = feed.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        std::thread::Builder::new()
            .name("lootwise-relay".into())
            .spawn(move || runtime.block_on(background.refresh_forever(source, cache)))?;
        Ok(feed)
    }

    async fn refresh_forever(self, source: RelaySource, cache: PathBuf) {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Lootwise/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/hawasman/lootwise)"
            ))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("the HTTP client's TLS backend is built in");
        loop {
            match sync(&client, &source, &cache, &self.league).await {
                Ok(report) => {
                    for category in &report.failed {
                        eprintln!(
                            "lootwise: relay {source}: {category} failed, keeping the cached copy"
                        );
                    }
                    if !report.downloaded.is_empty() {
                        let book = load(&cache, &self.league);
                        *self.book.write().unwrap_or_else(|e| e.into_inner()) = book;
                    }
                }
                Err(e) => eprintln!("lootwise: relay unreachable, using cached prices: {e}"),
            }
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    }

    /// Whether the feed has any prices yet.
    pub fn has_prices(&self) -> bool {
        !self
            .book
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }
}

impl PriceFeed for RelayFeed {
    fn price(&self, league: &League, item: &CommodityItem) -> Option<PriceEntry> {
        if *league != self.league {
            return None;
        }
        self.book
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .price(item)
    }

    fn divine_rate(&self, league: &League) -> Option<f64> {
        if *league != self.league {
            return None;
        }
        self.book
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .divine_rate()
    }
}
