//! Downloading the relay layout (`v1/{league}/manifest.json` + category files, see
//! `relay/README.md`) into a disk cache with the same layout, and loading prices from it.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use lootwise_core::League;
use serde::{Deserialize, Serialize};

use crate::book::{PriceBook, is_priced_category};

/// Where the relay's files come from: its http(s) base URL, or a local mirror folder made
/// by `pnpm dev:mirror`. Never poe.ninja itself (ADR 0004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelaySource {
    Http(reqwest::Url),
    Folder(PathBuf),
}

impl RelaySource {
    /// An `http://` or `https://` URL, else a folder path.
    pub fn parse(value: &str) -> Result<Self, SyncError> {
        if value.starts_with("http://") || value.starts_with("https://") {
            reqwest::Url::parse(value)
                .map(Self::Http)
                .map_err(|e| SyncError(format!("relay URL {value}: {e}")))
        } else {
            Ok(Self::Folder(PathBuf::from(value)))
        }
    }

    async fn get(
        &self,
        client: &reqwest::Client,
        league: &League,
        file: &str,
    ) -> Result<Vec<u8>, SyncError> {
        match self {
            Self::Http(base) => {
                let mut url = base.clone();
                url.path_segments_mut()
                    .map_err(|()| SyncError(format!("relay URL {base} cannot be a base")))?
                    .pop_if_empty()
                    .extend(["v1", league.name(), file]);
                let response = client
                    .get(url.clone())
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| SyncError(format!("{url}: {e}")))?;
                let body = response
                    .bytes()
                    .await
                    .map_err(|e| SyncError(format!("{url}: {e}")))?;
                Ok(body.to_vec())
            }
            Self::Folder(dir) => {
                let path = league_dir(dir, league).join(file);
                std::fs::read(&path).map_err(|e| SyncError(format!("{}: {e}", path.display())))
            }
        }
    }
}

impl fmt::Display for RelaySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(url) => write!(f, "{url}"),
            Self::Folder(dir) => write!(f, "{}", dir.display()),
        }
    }
}

/// A relay sync that could not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError(pub String);

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

/// The manifest fields the feed uses; the cache keeps them in the same shape.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    categories: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    fetched_at: Option<String>,
    etag: Option<String>,
}

/// What a sync did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Categories downloaded because their etag changed or they weren't cached yet.
    pub downloaded: Vec<String>,
    /// Categories whose download failed; their previous cached copy is kept.
    pub failed: Vec<String>,
}

fn league_dir(root: &Path, league: &League) -> PathBuf {
    root.join("v1").join(league.name())
}

fn read_manifest(dir: &Path) -> Option<Manifest> {
    let bytes = std::fs::read(dir.join("manifest.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes through a temporary file so a crash never leaves half a file in the cache.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Brings `cache`'s copy of `league` up to date with `source`, downloading only the priced
/// categories whose manifest etag changed. On error the cache is left as it was.
pub async fn sync(
    client: &reqwest::Client,
    source: &RelaySource,
    cache: &Path,
    league: &League,
) -> Result<SyncReport, SyncError> {
    let remote_bytes = source.get(client, league, "manifest.json").await?;
    let remote: Manifest = serde_json::from_slice(&remote_bytes)
        .map_err(|e| SyncError(format!("{source}: bad manifest for {}: {e}", league.name())))?;
    let dir = league_dir(cache, league);
    std::fs::create_dir_all(&dir).map_err(|e| SyncError(format!("{}: {e}", dir.display())))?;
    let cached = read_manifest(&dir).unwrap_or_default();

    let mut report = SyncReport::default();
    let mut kept = Manifest::default();
    for (category, entry) in remote.categories {
        if !is_priced_category(&category) || entry.etag.is_none() {
            continue;
        }
        let file = format!("{category}.json");
        let previous = cached.categories.get(&category);
        let unchanged = previous.is_some_and(|p| p.etag == entry.etag) && dir.join(&file).is_file();
        if !unchanged {
            let fetched = source.get(client, league, &file).await;
            let stored = fetched.and_then(|bytes| {
                write_atomic(&dir.join(&file), &bytes)
                    .map_err(|e| SyncError(format!("{}: {e}", dir.display())))
            });
            if stored.is_err() {
                report.failed.push(category.clone());
                if let Some(previous) = previous {
                    kept.categories.insert(category, previous.clone());
                }
                continue;
            }
            report.downloaded.push(category.clone());
        }
        kept.categories.insert(category, entry);
    }
    let manifest = serde_json::to_vec_pretty(&kept).expect("a manifest always serialises");
    write_atomic(&dir.join("manifest.json"), &manifest)
        .map_err(|e| SyncError(format!("{}: {e}", dir.display())))?;
    Ok(report)
}

/// The prices of `league` in `cache`, each stamped with its category's `fetchedAt`.
/// Missing or unreadable files contribute nothing.
pub fn load(cache: &Path, league: &League) -> PriceBook {
    let dir = league_dir(cache, league);
    let mut book = PriceBook::default();
    let Some(manifest) = read_manifest(&dir) else {
        return book;
    };
    for (category, entry) in manifest.categories {
        let Some(fetched_at) = entry
            .fetched_at
            .as_deref()
            .and_then(|t| humantime::parse_rfc3339(t).ok())
        else {
            continue;
        };
        let Ok(bytes) = std::fs::read(dir.join(format!("{category}.json"))) else {
            continue;
        };
        if let Err(e) = book.add_category(&category, &bytes, fetched_at) {
            eprintln!(
                "lootwise: skipping cached {category} of {}: {e}",
                league.name()
            );
        }
    }
    book
}
