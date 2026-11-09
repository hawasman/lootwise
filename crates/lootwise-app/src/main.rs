//! Lootwise: a tray app whose Ctrl+D runs a Price Check of the item under the pointer in
//! Path of Exile and shows its header in an overlay beside the pointer.

mod header;
mod price_check;

use lootwise_input::EnigoInput;
use price_check::PriceCheck;
use rune::{App, AppSpec, Cx, DisplayTarget, El, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec, Tray, TrayEvent, TrayIcon, TrayItem, TraySpec, Tx, el};

const QUIT: &str = "quit";

/// The windowless app's state: the way to the overlay.
struct Lootwise {
    overlay: OverlayHandle<PriceCheck>,
}

fn no_window(_: &Lootwise, _: &mut Cx<Lootwise>) -> El { el() }

fn price_check(s: &mut Lootwise, _: &mut Tx) { s.overlay.update(price_check::shortcut); }

/// A gold coin, so the tray entry is recognisable.
fn coin_icon() -> TrayIcon {
    const SIZE: u32 = 32;
    let centre = (SIZE as f32 - 1.0) / 2.0;
    let pixels = (0..SIZE * SIZE).flat_map(|i| {
        let (x, y) = ((i % SIZE) as f32 - centre, (i / SIZE) as f32 - centre);
        let d = (x * x + y * y).sqrt();
        if d > 15.0 { [0, 0, 0, 0] } else if d > 12.0 { [0xb0, 0x7d, 0x1a, 0xff] } else { [0xf2, 0xc1, 0x4e, 0xff] }
    }).collect();
    TrayIcon::rgba(SIZE, SIZE, pixels)
}

fn main() {
    let input = || Box::new(EnigoInput::new());
    let mut app = App::new(AppSpec::new("Lootwise", (320.0, 200.0), || unreachable!("built by App::init"), no_window)).windowless();
    let overlay = app.overlay(
        OverlaySpec::new("lootwise-price-check", (320.0, 140.0), price_check::view)
            .options(OverlayOptions {
                placement: Some(OverlayPlacement::FullScreen),
                display: DisplayTarget::UnderPointer,
                keys: KeyPolicy::OnDemand,
                input: OverlayInput::Regions,
                ..OverlayOptions::default()
            })
            .poll(price_check::poll)
            .on_key(price_check::on_key),
        PriceCheck::new(input(), input()),
    );
    let surface = overlay.clone();
    overlay.update(move |s, _| s.attach(surface));
    let app = app.init(move || Lootwise { overlay }).global_shortcut("ctrl+d", price_check);

    let quit = app.handle();
    let tray = Tray::new(
        TraySpec::new("lootwise", "Lootwise", coin_icon()).tooltip("Lootwise: Ctrl+D over an item").menu(vec![TrayItem::action(QUIT, "Quit")]),
        move |event| if event == TrayEvent::Menu(QUIT.into()) { quit.quit(); },
    );
    if let Err(e) = &tray { eprintln!("lootwise: no tray icon ({e}); quit with Ctrl+C in the terminal"); }
    app.run();
    drop(tray);
}
