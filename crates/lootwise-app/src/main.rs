//! Lootwise: a tray app whose Ctrl+D runs a Price Check of the item under the pointer in
//! Path of Exile and shows its Valuation in an overlay beside the pointer.
//!
//! Configuration, from the environment:
//! - `LOOTWISE_RELAY` (required): the price relay's base URL, or a local mirror folder made by
//!   `pnpm dev:mirror` (ADR 0004).
//! - `LOOTWISE_LEAGUE`: the League to price in, as poe.ninja names it; `Standard` by default.
//! - `LOOTWISE_CACHE`: where downloaded prices are kept; the platform data dir by default.

mod price_check;

use std::path::PathBuf;
use std::time::SystemTime;

use lootwise_core::{Clock, Engine, League};
use lootwise_input::EnigoInput;
use lootwise_sources::{RelayFeed, RelaySource, default_cache_dir};
use price_check::PriceCheckOverlay;
use rune::{
    App, AppSpec, Cx, DisplayTarget, El, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions,
    OverlayPlacement, OverlaySpec, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec, Tx, el,
};

const QUIT: &str = "quit";

/// The windowless app's state: the way to the overlay.
struct Lootwise {
    overlay: OverlayHandle<PriceCheckOverlay>,
}

fn no_window(_: &Lootwise, _: &mut Cx<Lootwise>) -> El {
    el()
}

fn price_check(s: &mut Lootwise, _: &mut Tx) {
    s.overlay.update(price_check::shortcut);
}

/// A gold coin, so the tray entry is recognisable.
fn coin_icon() -> TrayIcon {
    const SIZE: u32 = 32;
    let centre = (SIZE as f32 - 1.0) / 2.0;
    let pixels = (0..SIZE * SIZE)
        .flat_map(|i| {
            let (x, y) = ((i % SIZE) as f32 - centre, (i / SIZE) as f32 - centre);
            let d = (x * x + y * y).sqrt();
            if d > 15.0 {
                [0, 0, 0, 0]
            } else if d > 12.0 {
                [0xb0, 0x7d, 0x1a, 0xff]
            } else {
                [0xf2, 0xc1, 0x4e, 0xff]
            }
        })
        .collect();
    TrayIcon::rgba(SIZE, SIZE, pixels)
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The engine bound to the configured League and relay, or why it can't be built.
fn pricer() -> Result<price_check::Pricer, String> {
    let relay = std::env::var("LOOTWISE_RELAY").map_err(|_| {
        "set LOOTWISE_RELAY to the price relay's URL or a `pnpm dev:mirror` folder".to_owned()
    })?;
    let source = RelaySource::parse(&relay).map_err(|e| e.to_string())?;
    let league =
        League::new(std::env::var("LOOTWISE_LEAGUE").unwrap_or_else(|_| "Standard".into()));
    let cache = std::env::var_os("LOOTWISE_CACHE")
        .map(PathBuf::from)
        .or_else(default_cache_dir)
        .ok_or("no data dir for the price cache; set LOOTWISE_CACHE")?;
    eprintln!(
        "lootwise: pricing in {} from {source}, cached in {}",
        league.name(),
        cache.display()
    );
    let feed = RelayFeed::start(source, cache, league.clone())
        .map_err(|e| format!("can't start the price feed: {e}"))?;
    let engine = Engine::new(feed, SystemClock);
    Ok(Box::new(move |text| engine.price_check(&league, text)))
}

fn main() {
    let pricer = pricer().unwrap_or_else(|e| {
        eprintln!("lootwise: {e}");
        std::process::exit(2);
    });
    let input = || Box::new(EnigoInput::new());
    let mut app = App::new(AppSpec::new(
        "Lootwise",
        (320.0, 200.0),
        || unreachable!("built by App::init"),
        no_window,
    ))
    .windowless();
    let overlay = app.overlay(
        OverlaySpec::new(
            "lootwise-price-check",
            price_check::CARD_SIZE,
            price_check::view,
        )
        .options(OverlayOptions {
            placement: Some(OverlayPlacement::FullScreen),
            display: DisplayTarget::UnderPointer,
            keys: KeyPolicy::OnDemand,
            input: OverlayInput::Regions,
            ..OverlayOptions::default()
        })
        .poll(price_check::poll)
        .on_key(price_check::on_key),
        PriceCheckOverlay::new(input(), input(), pricer),
    );
    let surface = overlay.clone();
    overlay.update(move |s, _| s.attach(surface));
    let app = app
        .init(move || Lootwise { overlay })
        .global_shortcut("ctrl+d", price_check);

    let quit = app.handle();
    let tray = Tray::new(
        TraySpec::new("lootwise", "Lootwise", coin_icon())
            .tooltip("Lootwise: Ctrl+D over an item")
            .menu(vec![TrayItem::action(QUIT, "Quit")]),
        move |event| {
            if event == TrayEvent::Menu(QUIT.into()) {
                quit.quit();
            }
        },
    );
    if let Err(e) = &tray {
        eprintln!("lootwise: no tray icon ({e}); quit with Ctrl+C in the terminal");
    }
    app.run();
    drop(tray);
}
