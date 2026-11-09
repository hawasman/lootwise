//! The Price Check overlay: its state, its view, and the Ctrl+D sequence that fills it.
//!
//! The overlay covers the display under the pointer, transparent and click-through except for
//! its card ([`OverlayInput::Regions`]), so the game keeps focus. The card sits next to where the
//! pointer was when the player pressed Ctrl+D; Ctrl+D again prices the item under the pointer
//! then and replaces the card. It takes keys only once the player clicks it
//! ([`KeyPolicy::OnDemand`]), and then hears Esc alone. Since the game keeps focus, the overlay
//! can't hear the player leave the card; instead it samples the pointer while the card is shown
//! and closes the card on a mouse button press outside it or once the pointer is far from it.

use std::time::Duration;

use lootwise_core::{Confidence, Item, PriceCheck, Rarity, Valuation};
use lootwise_input::{ClipboardCopy, KeySender, Pointer, PointerLocator, Tick};
use rune::{ClipboardContent, Color, Cx, El, Key, KeyEvent, OverlayHandle, Tx, col, el, text};
use rune_kit::Kit;

pub const NOT_AN_ITEM: &str = "Not a Path of Exile item";
pub const NOT_A_COMMODITY_ITEM: &str = "Not a Commodity Item: valuing it needs a Live Search";
pub const NO_PRICE: &str = "No price for this item in the League";
pub const PRICES_UNAVAILABLE: &str =
    "Prices unavailable for this League (check LOOTWISE_LEAGUE and the relay)";
pub const LOW_CONFIDENCE: &str = "Low confidence";
/// The card's width, and the height kept clear for it at the bottom of the display, in points.
/// Also the overlay's nominal size; its full-screen placement overrides it.
pub const CARD_SIZE: (f32, f32) = (320.0, 220.0);
/// The card's offset from the pointer, in points.
const POINTER_GAP: f32 = 16.0;
/// Seconds between pointer samples while the card is shown.
const SAMPLE_EVERY: f64 = 0.05;
/// How far from the card, in points, the pointer goes before the card closes: about one card
/// width, so moving to the next item nearby keeps it.
const LEAVE_DISTANCE: f32 = CARD_SIZE.0;

/// Runs a Price Check of clipboard text: the engine, bound to the player's League.
pub type Pricer = Box<dyn Fn(&str) -> PriceCheck>;

/// What the card shows.
#[derive(Clone, Debug, PartialEq)]
pub enum CardContent {
    Checked(PriceCheck),
    /// The copy keystroke couldn't be sent.
    Failed(String),
}

/// Where the card is: its rect in global points, as the overlay last laid it out, and the
/// display's scale then.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CardBounds {
    rect: [f32; 4],
    scale: f32,
}

/// The overlay's state.
pub struct PriceCheckOverlay {
    keys: Box<dyn KeySender>,
    pointer: Box<dyn PointerLocator>,
    copy: ClipboardCopy<ClipboardContent>,
    pricer: Pricer,
    card: Option<CardContent>,
    /// Global desktop pixels: where the pointer was when the player pressed Ctrl+D for the
    /// card shown.
    anchor: Option<(i32, i32)>,
    /// The same for the Price Check running, until its card replaces the shown one.
    checking_at: Option<(i32, i32)>,
    /// The overlay's rect in global points, from `on_screen_layout`.
    screen: [f32; 4],
    bounds: Option<CardBounds>,
    /// When the pointer is next sampled, while the card is shown.
    sample_due: Option<f64>,
    /// Whether a mouse button was held at the last sample, so a press is told from a hold.
    was_pressed: Option<bool>,
    /// The native overlay, absent in headless runs.
    surface: Option<OverlayHandle<PriceCheckOverlay>>,
}

impl PriceCheckOverlay {
    pub fn new(keys: Box<dyn KeySender>, pointer: Box<dyn PointerLocator>, pricer: Pricer) -> Self {
        Self {
            keys,
            pointer,
            copy: ClipboardCopy::new(),
            pricer,
            card: None,
            anchor: None,
            checking_at: None,
            screen: [0.0; 4],
            bounds: None,
            sample_due: None,
            was_pressed: None,
            surface: None,
        }
    }

    /// Lets the overlay show and hide its native surface.
    pub fn attach(&mut self, surface: OverlayHandle<PriceCheckOverlay>) {
        self.surface = Some(surface);
    }

    fn show(&mut self, card: CardContent) {
        self.card = Some(card);
        self.anchor = self.checking_at.take();
        if let Some(surface) = &self.surface {
            surface.show();
        }
    }

    fn hide(&mut self) {
        self.card = None;
        self.sample_due = None;
        self.was_pressed = None;
        if let Some(surface) = &self.surface {
            surface.hide();
        }
    }
}

/// Ctrl+D: start a Price Check of the item under the pointer, whose card replaces any shown.
pub fn shortcut(s: &mut PriceCheckOverlay, tx: &mut Tx) {
    if !s.copy.begin() {
        return;
    }
    // Before the copy's delays, so a mouse moved meanwhile doesn't drag the card off the item.
    s.checking_at = s.pointer.pointer().map(|p| p.at);
    tx.read_clipboard(|s: &mut PriceCheckOverlay, saved, tx| {
        if let Some(due) = s.copy.saved(saved.clone(), tx.clock()) {
            tx.copy("");
            tx.wake_at(due);
        }
    });
}

/// Before every frame and at each deadline asked for: steps the copy sequence and, while the
/// card is shown, watches the pointer.
pub fn poll(s: &mut PriceCheckOverlay, clock: f64, tx: &mut Tx) -> bool {
    let copied = match s.copy.tick(clock, s.keys.as_mut()) {
        Tick::Idle => false,
        Tick::WakeAt(due) => {
            tx.wake_at(due);
            false
        }
        Tick::ReadClipboard => {
            tx.read_clipboard(|s: &mut PriceCheckOverlay, copied, tx| {
                let Some(saved) = s.copy.read() else { return };
                tx.copy_rich(saved);
                let check = (s.pricer)(copied.text());
                s.show(CardContent::Checked(check));
            });
            false
        }
        Tick::Failed { restore, error } => {
            tx.copy_rich(restore);
            s.show(CardContent::Failed(error.to_string()));
            true
        }
    };
    let left = watch_pointer(s, clock, tx);
    copied || left
}

/// While the card is shown, samples the pointer every [`SAMPLE_EVERY`] and closes the card
/// once the player leaves it. Says whether it closed it.
fn watch_pointer(s: &mut PriceCheckOverlay, clock: f64, tx: &mut Tx) -> bool {
    if s.card.is_none() {
        return false;
    }
    let due = *s.sample_due.get_or_insert(clock);
    if clock < due {
        tx.wake_at(due);
        return false;
    }
    s.sample_due = Some(clock + SAMPLE_EVERY);
    tx.wake_at(clock + SAMPLE_EVERY);
    let Some(pointer) = s.pointer.pointer() else {
        return false;
    };
    let pressed = pointer.pressed && s.was_pressed == Some(false);
    s.was_pressed = Some(pointer.pressed);
    if s.bounds
        .is_some_and(|card| player_left(card, pointer, pressed))
    {
        s.hide();
        return true;
    }
    false
}

/// Whether the player has left the card: pressed a mouse button outside it (`pressed`: a
/// button went down since the last sample), or moved the pointer beyond [`LEAVE_DISTANCE`].
fn player_left(card: CardBounds, pointer: Pointer, pressed: bool) -> bool {
    let scale = if card.scale > 0.0 { card.scale } else { 1.0 };
    let (px, py) = (pointer.at.0 as f32 / scale, pointer.at.1 as f32 / scale);
    let [x, y, w, h] = card.rect;
    let dx = (x - px).max(px - (x + w)).max(0.0);
    let dy = (y - py).max(py - (y + h)).max(0.0);
    let distance = dx.hypot(dy);
    distance > LEAVE_DISTANCE || (pressed && distance > 0.0)
}

/// The overlay's keys, once it has them: Esc closes the card, everything else passes.
pub fn on_key(s: &mut PriceCheckOverlay, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key != Key::Escape || s.card.is_none() {
        return false;
    }
    s.hide();
    true
}

/// Where the card's top-left goes in the overlay, in points: beside the pointer, kept on the
/// display. `pointer` is in global desktop pixels; `screen` is the overlay's global rect in
/// points (zero before its first layout, when no clamping happens).
fn card_origin(pointer: Option<(i32, i32)>, scale: f32, screen: [f32; 4]) -> (f32, f32) {
    let Some((px, py)) = pointer else {
        return (POINTER_GAP, POINTER_GAP);
    };
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let x = px as f32 / scale - screen[0] + POINTER_GAP;
    let y = py as f32 / scale - screen[1] + POINTER_GAP;
    let clamp = |v: f32, room: f32| {
        if room > 0.0 {
            v.min(room).max(0.0)
        } else {
            v.max(0.0)
        }
    };
    (
        clamp(x, screen[2] - CARD_SIZE.0),
        clamp(y, screen[3] - CARD_SIZE.1),
    )
}

/// Path of Exile's colours for each rarity's name.
fn rarity_color(rarity: Rarity, k: &Kit) -> Color {
    let rgb = |r: u8, g: u8, b: u8| Color {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    };
    match rarity {
        Rarity::Normal => rgb(0xc8, 0xc8, 0xc8),
        Rarity::Magic => rgb(0x88, 0x88, 0xff),
        Rarity::Rare => rgb(0xff, 0xff, 0x77),
        Rarity::Unique => rgb(0xaf, 0x60, 0x25),
        Rarity::Gem => rgb(0x1b, 0xa2, 0x9b),
        Rarity::Currency => rgb(0xaa, 0x9e, 0x82),
        Rarity::DivinationCard => rgb(0x0e, 0xba, 0xff),
        Rarity::Quest => k.c.text_base,
    }
}

/// A price with as many decimals as it needs to be readable: `1.94`, `17.5`, `602`, and two
/// significant digits below 1, so a chaos orb in divines reads `0.002`, not `0`.
fn amount(value: f64) -> String {
    let decimals = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else if value >= 1.0 || value <= 0.0 {
        2
    } else {
        (1 - value.log10().floor() as i32).clamp(2, 6) as usize
    };
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// How old the prices are, e.g. `just now`, `12 min ago`, `3 h ago`.
fn age(age: Duration) -> String {
    let minutes = age.as_secs() / 60;
    match minutes {
        0 => "just now".to_owned(),
        1..=119 => format!("{minutes} min ago"),
        _ => format!("{} h ago", minutes / 60),
    }
}

fn line(key: &'static str, value: String, k: &Kit) -> El {
    text(value).key(key).font(k.ty.body).color(k.c.text_base)
}

/// The item's name lines, in its rarity's colour.
fn item_name(item: &Item, k: &Kit) -> (Color, El) {
    let color = rarity_color(item.rarity, k);
    let mut name = col().gap(2.0).child(
        text(item.name.clone())
            .key("item-name")
            .font(k.ty.body_medium)
            .color(color),
    );
    if let Some(base) = &item.base_type {
        name = name.child(
            text(base.clone())
                .key("item-base")
                .font(k.ty.body_medium)
                .color(color),
        );
    }
    (color, name)
}

fn valuation_lines(valuation: &Valuation, k: &Kit) -> El {
    let mut body = col().gap(2.0).child(
        text(format!("{} chaos", amount(valuation.chaos_value)))
            .key("chaos")
            .font(k.ty.body_medium)
            .color(k.c.text_base),
    );
    if let Some(divine) = valuation.divine_value {
        body = body.child(line("divine", format!("{} divine", amount(divine)), k));
    }
    if let Some(listings) = valuation.listing_count {
        body = body.child(line("listings", format!("{listings} listings"), k));
    }
    body = body.child(
        text(format!("{} · {}", valuation.source, age(valuation.age)))
            .key("age")
            .font(k.ty.body)
            .color(k.c.text_muted),
    );
    if valuation.confidence == Confidence::Low {
        body = body.child(
            text(LOW_CONFIDENCE)
                .key("low-confidence")
                .font(k.ty.body_medium)
                .color(Color {
                    r: 0.95,
                    g: 0.6,
                    b: 0.2,
                    a: 1.0,
                }),
        );
    }
    body
}

fn card_body(card: &CardContent, k: &Kit) -> (Color, El) {
    let note = |key: &'static str, message: &str| {
        text(message.to_owned())
            .key(key)
            .font(k.ty.body)
            .color(k.c.text_base)
            .wrap()
            .w_full()
    };
    let with_item = |item: &Item, below: El| {
        let (color, name) = item_name(item, k);
        (color, col().gap(6.0).child(name).child(below))
    };
    match card {
        CardContent::Checked(PriceCheck::Valued { item, valuation }) => {
            with_item(item, valuation_lines(valuation, k))
        }
        CardContent::Checked(PriceCheck::PricesUnavailable { item, .. }) => {
            with_item(item, note("prices-unavailable", PRICES_UNAVAILABLE))
        }
        CardContent::Checked(PriceCheck::NoPrice { item, .. }) => {
            with_item(item, note("no-price", NO_PRICE))
        }
        CardContent::Checked(PriceCheck::NotACommodityItem { item }) => {
            with_item(item, note("not-a-commodity-item", NOT_A_COMMODITY_ITEM))
        }
        CardContent::Checked(PriceCheck::NotAnItem) => {
            (k.c.icon_muted, note("not-an-item", NOT_AN_ITEM))
        }
        CardContent::Failed(error) => (k.c.icon_muted, note("failed", error)),
    }
}

pub fn view(s: &PriceCheckOverlay, cx: &mut Cx<PriceCheckOverlay>) -> El {
    let root = el()
        .key("price-check")
        .w_full()
        .h_full()
        .on_screen_layout(|s: &mut PriceCheckOverlay, rect| s.screen = rect);
    let Some(card) = &s.card else { return root };
    let k = Kit::neutral(true);
    let (accent, body) = card_body(card, &k);
    let scale = cx.env().scale;
    let (x, y) = card_origin(s.anchor, scale, s.screen);
    let card = rune_kit::Card::new().accent(accent).child(body).build(&k);
    root.child(
        el().key("card")
            .w(CARD_SIZE.0)
            .radius(k.radius.md)
            .fill(k.c.bg_base)
            .border(1.0, k.c.border_base)
            .absolute(x, y)
            .hit()
            // Where the player's pointer is measured from, to close the card once they leave.
            .on_screen_layout(move |s: &mut PriceCheckOverlay, rect| {
                s.bounds = Some(CardBounds { rect, scale });
            })
            // A click hands the card the keyboard, so Esc reaches it.
            .on_press(|s: &mut PriceCheckOverlay| {
                if let Some(surface) = &s.surface {
                    surface.request_keys();
                }
            })
            .child(card),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lootwise_input::InputError;
    use rune::clipboard::MemoryClipboard;
    use rune::{Env, Mods, Runtime};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::SystemTime;

    use lootwise_core::{Clock, CommodityItem, Engine, League, PriceEntry, PriceFeed};

    const SAMPLE: &str = "Item Class: Rings\nRarity: Rare\nDoom Loop\nTwo-Stone Ring\n--------\nRequirements:\nLevel: 32\n--------\nItem Level: 84\n";

    /// Stands in for the game: Ctrl+C puts what `copies` holds then on the clipboard.
    struct FakeGame {
        clipboard: MemoryClipboard,
        copies: Rc<Cell<&'static str>>,
        presses: Rc<Cell<u32>>,
    }
    impl KeySender for FakeGame {
        fn send_copy(&mut self) -> Result<(), InputError> {
            self.presses.set(self.presses.get() + 1);
            self.clipboard.set_text(self.copies.get());
            Ok(())
        }
    }
    struct FakePointer;
    impl PointerLocator for FakePointer {
        fn pointer(&mut self) -> Option<Pointer> {
            Some(Pointer {
                at: (400, 300),
                pressed: false,
            })
        }
    }
    /// A pointer the test moves and presses, counting how often the overlay samples it.
    #[derive(Clone, Default)]
    struct Mouse {
        at: Rc<Cell<(i32, i32)>>,
        pressed: Rc<Cell<bool>>,
        samples: Rc<Cell<u32>>,
    }
    impl Mouse {
        fn at(x: i32, y: i32) -> Self {
            let mouse = Self::default();
            mouse.at.set((x, y));
            mouse
        }
    }
    impl PointerLocator for Mouse {
        fn pointer(&mut self) -> Option<Pointer> {
            self.samples.set(self.samples.get() + 1);
            Some(Pointer {
                at: self.at.get(),
                pressed: self.pressed.get(),
            })
        }
    }

    const UNIQUE: &str = "Item Class: Body Armours\nRarity: Unique\nKaom's Heart\nGlorious Plate\n--------\nArmour: 553\n--------\nItem Level: 80\n";
    const DIVINE: &str = "Item Class: Stackable Currency\nRarity: Currency\nDivine Orb\n--------\nStack Size: 3/20\n--------\n";
    const MIRROR: &str = "Item Class: Stackable Currency\nRarity: Currency\nMirror of Kalandra\n--------\nStack Size: 1/10\n--------\n";

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    /// Prices Kaom's Heart from 5 minutes ago and a thinly listed Divine Orb from 2 hours ago,
    /// at 200 chaos per divine.
    struct FakeFeed;
    impl PriceFeed for FakeFeed {
        fn covers(&self, league: &League) -> bool {
            league.name() == "Standard"
        }
        fn price(&self, league: &League, item: &CommodityItem) -> Option<PriceEntry> {
            if league.name() != "Standard" {
                return None;
            }
            let (chaos_value, listing_count, age) = match item {
                CommodityItem::Unique { name, .. } if name == "Kaom's Heart" => {
                    (600.0, Some(245), 5)
                }
                CommodityItem::Currency { name } if name == "Divine Orb" => (200.0, Some(3), 120),
                _ => return None,
            };
            Some(PriceEntry {
                chaos_value,
                listing_count,
                fetched_at: now() - Duration::from_secs(age * 60),
                source: "poe.ninja".into(),
            })
        }
        fn divine_rate(&self, _: &League) -> Option<f64> {
            Some(200.0)
        }
    }
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            now()
        }
    }

    /// The engine pricing in `league` against [`FakeFeed`], which only covers `Standard`.
    fn pricer_in(league: &str) -> Pricer {
        let engine = Engine::new(FakeFeed, FixedClock);
        let league = League::new(league);
        Box::new(move |text| engine.price_check(&league, text))
    }

    fn pricer() -> Pricer {
        pricer_in("Standard")
    }

    /// The overlay as its host runs it, with `keys` in place of the game's input.
    fn runtime(
        clipboard: &MemoryClipboard,
        keys: Box<dyn KeySender>,
        pricer: Pricer,
    ) -> Runtime<PriceCheckOverlay> {
        runtime_with(clipboard, keys, Box::new(FakePointer), pricer)
    }

    /// The same with `pointer` in place of the player's mouse.
    fn runtime_with(
        clipboard: &MemoryClipboard,
        keys: Box<dyn KeySender>,
        pointer: Box<dyn PointerLocator>,
        pricer: Pricer,
    ) -> Runtime<PriceCheckOverlay> {
        let mut rt = Runtime::new(
            PriceCheckOverlay::new(keys, pointer, pricer),
            view,
            (1920.0, 1080.0),
            Env::new(true, 1.0),
        );
        rt.on_key = Some(on_key);
        rt.use_clipboard(clipboard);
        rt.frame(0.0);
        rt
    }

    fn game(clipboard: &MemoryClipboard, copies: &'static str) -> (Box<FakeGame>, Rc<Cell<u32>>) {
        let presses = Rc::new(Cell::new(0));
        let game = FakeGame {
            clipboard: clipboard.clone(),
            copies: Rc::new(Cell::new(copies)),
            presses: presses.clone(),
        };
        (Box::new(game), presses)
    }

    fn overlay(
        clipboard: &MemoryClipboard,
        copies: &'static str,
    ) -> (Runtime<PriceCheckOverlay>, Rc<Cell<u32>>) {
        let (game, presses) = game(clipboard, copies);
        (runtime(clipboard, game, pricer()), presses)
    }

    /// Frames for `seconds`, polling before each as the overlay host does.
    fn run(rt: &mut Runtime<PriceCheckOverlay>, seconds: f64) {
        for _ in 0..(seconds * 60.0).ceil() as u32 {
            rt.update_if(|s, tx| poll(s, tx.clock(), tx));
            rt.frame(1.0 / 60.0);
        }
    }

    fn press_ctrl_d(rt: &mut Runtime<PriceCheckOverlay>) {
        rt.update(shortcut);
        run(rt, 0.5);
    }

    fn text_of(rt: &Runtime<PriceCheckOverlay>, key: &str) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .and_then(|e| e.text)
    }

    #[test]
    fn ctrl_d_over_a_rare_says_it_is_not_a_commodity_item_and_esc_hides_it() {
        let clipboard = rune::clipboard::memory();
        clipboard.set_text("the player's own clipboard");
        let (mut rt, presses) = overlay(&clipboard, SAMPLE);

        press_ctrl_d(&mut rt);
        assert_eq!(presses.get(), 1, "one Ctrl+C reached the game");
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Doom Loop"));
        assert_eq!(text_of(&rt, "item-base").as_deref(), Some("Two-Stone Ring"));
        assert_eq!(
            text_of(&rt, "not-a-commodity-item").as_deref(),
            Some(NOT_A_COMMODITY_ITEM)
        );

        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        run(&mut rt, 0.5);
        assert_eq!(rt.state.card, None);
        assert_eq!(text_of(&rt, "item-name"), None, "the card is gone");
    }

    #[test]
    fn ctrl_d_over_a_unique_shows_its_valuation() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = overlay(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Kaom's Heart"));
        assert_eq!(text_of(&rt, "item-base").as_deref(), Some("Glorious Plate"));
        assert_eq!(text_of(&rt, "chaos").as_deref(), Some("600 chaos"));
        assert_eq!(text_of(&rt, "divine").as_deref(), Some("3 divine"));
        assert_eq!(text_of(&rt, "listings").as_deref(), Some("245 listings"));
        assert_eq!(
            text_of(&rt, "age").as_deref(),
            Some("poe.ninja · 5 min ago")
        );
        assert_eq!(text_of(&rt, "low-confidence"), None);
    }

    #[test]
    fn a_low_confidence_valuation_is_marked() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = overlay(&clipboard, DIVINE);
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "chaos").as_deref(), Some("200 chaos"));
        assert_eq!(text_of(&rt, "divine").as_deref(), Some("1 divine"));
        assert_eq!(text_of(&rt, "listings").as_deref(), Some("3 listings"));
        assert_eq!(text_of(&rt, "age").as_deref(), Some("poe.ninja · 2 h ago"));
        assert_eq!(
            text_of(&rt, "low-confidence").as_deref(),
            Some(LOW_CONFIDENCE)
        );
    }

    #[test]
    fn a_commodity_item_without_a_price_says_so() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = overlay(&clipboard, MIRROR);
        press_ctrl_d(&mut rt);
        assert_eq!(
            text_of(&rt, "item-name").as_deref(),
            Some("Mirror of Kalandra")
        );
        assert_eq!(text_of(&rt, "no-price").as_deref(), Some(NO_PRICE));
        assert_eq!(text_of(&rt, "chaos"), None);
    }

    #[test]
    fn a_league_without_prices_says_prices_are_unavailable() {
        let clipboard = rune::clipboard::memory();
        let (game, _) = game(&clipboard, DIVINE);
        let mut rt = runtime(&clipboard, game, pricer_in("Standrad"));
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Divine Orb"));
        assert_eq!(
            text_of(&rt, "prices-unavailable").as_deref(),
            Some(PRICES_UNAVAILABLE)
        );
        assert_eq!(text_of(&rt, "no-price"), None);
        assert_eq!(text_of(&rt, "chaos"), None);
    }

    #[test]
    fn amounts_keep_only_the_decimals_they_need() {
        assert_eq!(amount(1.94), "1.94");
        assert_eq!(amount(0.333), "0.33");
        assert_eq!(
            amount(1.0 / 498.2),
            "0.002",
            "a chaos orb in divines is not 0"
        );
        assert_eq!(amount(36.03 / 498.2), "0.072");
        assert_eq!(amount(2.0), "2");
        assert_eq!(amount(17.5), "17.5");
        assert_eq!(amount(602.6), "603");
    }

    #[test]
    fn the_players_clipboard_is_restored_after_a_price_check() {
        let clipboard = rune::clipboard::memory();
        let own = ClipboardContent::new("league notes").with_html("<b>league</b> notes");
        clipboard.set_content(own.clone());
        let (mut rt, _) = overlay(&clipboard, SAMPLE);

        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Doom Loop"));
        let after = clipboard
            .content()
            .map(|c| (c.text().to_owned(), c.html().map(str::to_owned)));
        assert_eq!(
            after,
            Some((
                "league notes".to_owned(),
                Some("<b>league</b> notes".to_owned())
            ))
        );
    }

    #[test]
    fn text_that_isnt_an_item_says_so() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = overlay(&clipboard, "just some chat text");
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "not-an-item").as_deref(), Some(NOT_AN_ITEM));
    }

    #[test]
    fn a_game_that_copies_nothing_leaves_no_stale_item() {
        // The player's clipboard holds an item copied earlier; the pointer is over nothing now.
        let clipboard = rune::clipboard::memory();
        clipboard.set_text(SAMPLE);
        struct Nothing;
        impl KeySender for Nothing {
            fn send_copy(&mut self) -> Result<(), InputError> {
                Ok(())
            }
        }
        let mut rt = runtime(&clipboard, Box::new(Nothing), pricer());
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "not-an-item").as_deref(), Some(NOT_AN_ITEM));
        assert_eq!(
            clipboard.text().as_deref().map(String::as_str),
            Some(SAMPLE),
            "and the old item is back on the clipboard"
        );
    }

    /// The overlay with a [`Mouse`] at (400, 300), and the game copying what the returned cell
    /// holds.
    fn watched(
        clipboard: &MemoryClipboard,
        copies: &'static str,
    ) -> (Runtime<PriceCheckOverlay>, Mouse, Rc<Cell<&'static str>>) {
        let mouse = Mouse::at(400, 300);
        let copies = Rc::new(Cell::new(copies));
        let game = FakeGame {
            clipboard: clipboard.clone(),
            copies: copies.clone(),
            presses: Rc::default(),
        };
        let rt = runtime_with(clipboard, Box::new(game), Box::new(mouse.clone()), pricer());
        (rt, mouse, copies)
    }

    /// Where the card is drawn, in points: beside (400, 300), so its left edge is at 416.
    fn card_at(rt: &Runtime<PriceCheckOverlay>) -> Option<(f32, f32)> {
        rt.presented("card").map(|(rect, _)| (rect[0], rect[1]))
    }

    fn click(rt: &mut Runtime<PriceCheckOverlay>, mouse: &Mouse, at: (i32, i32)) {
        mouse.at.set(at);
        run(rt, 0.1);
        mouse.pressed.set(true);
        run(rt, 0.1);
        mouse.pressed.set(false);
        run(rt, 0.1);
    }

    #[test]
    fn ctrl_d_while_a_card_is_shown_replaces_it_with_the_new_items_valuation() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, copies) = watched(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Kaom's Heart"));

        mouse.at.set((450, 320));
        copies.set(DIVINE);
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Divine Orb"));
        assert_eq!(text_of(&rt, "chaos").as_deref(), Some("200 chaos"));
        run(&mut rt, 1.0);
        assert_eq!(
            card_at(&rt),
            Some((466.0, 336.0)),
            "moved beside the new item"
        );
    }

    #[test]
    fn a_press_outside_the_card_closes_it() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);
        assert!(rt.state.card.is_some());
        // Left of the card, nearer than the distance that closes it on its own.
        click(&mut rt, &mouse, (300, 330));
        assert_eq!(rt.state.card, None);
        assert_eq!(text_of(&rt, "item-name"), None, "the card is gone");
    }

    #[test]
    fn a_press_inside_the_card_keeps_it() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);
        click(&mut rt, &mouse, (500, 350));
        assert!(rt.state.card.is_some());
    }

    #[test]
    fn a_button_held_since_before_the_card_opened_is_not_a_press() {
        // The player keeps the left button down to walk while pressing Ctrl+D.
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, UNIQUE);
        mouse.pressed.set(true);
        press_ctrl_d(&mut rt);
        mouse.at.set((300, 330));
        run(&mut rt, 0.2);
        assert!(rt.state.card.is_some());
    }

    #[test]
    fn moving_beyond_about_a_card_width_from_the_card_closes_it() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);
        // The card spans x 416..736.
        mouse.at.set((1000, 330));
        run(&mut rt, 0.2);
        assert!(rt.state.card.is_some(), "264 points away");
        mouse.at.set((1070, 330));
        run(&mut rt, 0.2);
        assert_eq!(rt.state.card, None, "334 points away");
    }

    #[test]
    fn the_pointer_is_sampled_only_while_the_card_is_shown() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, UNIQUE);
        run(&mut rt, 1.0);
        assert_eq!(mouse.samples.get(), 0, "before any card");

        press_ctrl_d(&mut rt);
        let before = mouse.samples.get();
        run(&mut rt, 1.0);
        let shown = mouse.samples.get() - before;
        assert!(
            (15..=25).contains(&shown),
            "about every 50 ms while shown: {shown} in a second"
        );

        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        run(&mut rt, 0.1);
        let hidden = mouse.samples.get();
        run(&mut rt, 1.0);
        assert_eq!(mouse.samples.get(), hidden, "none once hidden");
    }

    #[test]
    fn the_card_opens_beside_the_pointer_and_stays_on_the_display() {
        let screen = [0.0, 0.0, 1920.0, 1080.0];
        assert_eq!(card_origin(Some((400, 300)), 1.0, screen), (416.0, 316.0));
        assert_eq!(
            card_origin(Some((800, 600)), 2.0, screen),
            (416.0, 316.0),
            "pixels become points"
        );
        assert_eq!(
            card_origin(Some((1910, 1070)), 1.0, screen),
            (1600.0, 860.0),
            "kept clear of the edges"
        );
        assert_eq!(
            card_origin(Some((2320, 300)), 1.0, [1920.0, 0.0, 1920.0, 1080.0]),
            (416.0, 316.0),
            "on a display right of the primary"
        );
    }

    #[test]
    fn the_card_stays_where_ctrl_d_was_pressed_when_the_mouse_moves_during_the_copy() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, _) = watched(&clipboard, SAMPLE);
        rt.update(shortcut);
        mouse.at.set((900, 700));
        run(&mut rt, 0.5);
        assert_eq!(card_at(&rt), Some((416.0, 316.0)));
    }
}
