//! The Price Check overlay: its state, its view, and the Ctrl+D sequence that fills it.
//!
//! The overlay covers the display under the pointer, transparent and click-through except for
//! its card ([`OverlayInput::Regions`]), so the game keeps focus. The card sits next to where the
//! pointer was when the player pressed Ctrl+D; Ctrl+D again prices the item under the pointer
//! then and replaces the card. It takes keys only once the player clicks it
//! ([`KeyPolicy::OnDemand`]), and then hears Esc alone. Since the game keeps focus, the overlay
//! can't hear the player leave the card; instead it samples the pointer while the card is shown
//! and closes the card on a mouse button press outside it or once the pointer is far from it.
//!
//! The pointer and the overlay measure in different spaces: the pointer in global desktop
//! pixels (XWayland's root on Linux), the overlay in points that, on Wayland, start at `(0, 0)`
//! on every display. So both placement and leave detection work in the card's display's local
//! points: the pointer less its display's origin, both as the pointer sees them, and the card's
//! rect less the overlay's.
//!
//! [`OverlayInput::Regions`]: rune::OverlayInput::Regions
//! [`KeyPolicy::OnDemand`]: rune::KeyPolicy::OnDemand

use std::time::Duration;

use lootwise_core::{Confidence, Item, PriceCheck, Rarity, Valuation};
use lootwise_input::{ClipboardCopy, KeySender, PointerDisplay, PointerLocator, Tick};
use rune::{
    ClipboardContent, Color, Cx, Display, DisplayTarget, El, Key, KeyEvent, OverlayHandle, Tx, col,
    el, text,
};
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
/// Seconds between pointer samples while the card is shown: shorter than a quick click, which
/// a sample has to land in to be seen. A sample is one XWayland round trip, a few microseconds.
const SAMPLE_EVERY: f64 = 0.02;
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

/// Where a card goes: beside where the pointer was when the player pressed Ctrl+D, on the
/// display it was on.
#[derive(Clone, Debug, PartialEq)]
struct CardPlace {
    /// The pointer, in global desktop pixels; `None` when the platform couldn't say.
    pointer: Option<(i32, i32)>,
    /// The top-left of the pointer's display, in the same pixels (when the pointer's display
    /// is unknown, the chosen display's position as Rune lists it).
    origin: (i32, i32),
    /// The display the overlay opens on.
    display: DisplayTarget,
}

impl CardPlace {
    /// `at`, in global desktop pixels, as points from the display's top-left at its `scale`
    /// (1 when unknown).
    fn local(&self, (x, y): (i32, i32), scale: f32) -> (f32, f32) {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let (dx, dy) = (
            i64::from(x) - i64::from(self.origin.0),
            i64::from(y) - i64::from(self.origin.1),
        );
        (dx as f32 / scale, dy as f32 / scale)
    }
}

/// Where the card is: its rect in the overlay's local points, as last laid out, the display's
/// scale then, and the place it was laid out for. Bounds laid out for another place are a
/// previous card's.
#[derive(Clone, Debug, PartialEq)]
struct CardBounds {
    rect: [f32; 4],
    scale: f32,
    place: Option<CardPlace>,
}

/// The overlay's state.
pub struct PriceCheckOverlay {
    keys: Box<dyn KeySender>,
    pointer: Box<dyn PointerLocator>,
    copy: ClipboardCopy<ClipboardContent>,
    pricer: Pricer,
    /// The displays the overlay can open on, as Rune lists them at startup.
    displays: Vec<Display>,
    card: Option<CardContent>,
    /// Where the card shown goes.
    place: Option<CardPlace>,
    /// The same for the Price Check running, until its card replaces the shown one.
    checking: Option<CardPlace>,
    /// The overlay's rect in points, from `on_screen_layout`: global on X11 and Windows,
    /// starting at `(0, 0)` on Wayland.
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
            displays: Vec::new(),
            card: None,
            place: None,
            checking: None,
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

    /// The displays a card can open on.
    pub fn set_displays(&mut self, displays: Vec<Display>) {
        self.displays = displays;
    }

    fn show(&mut self, card: CardContent) {
        let was_shown = self.card.is_some();
        self.card = Some(card);
        if let Some(place) = self.checking.take() {
            let moved = self.place.as_ref().map(|p| &p.display) != Some(&place.display);
            // A shown overlay moved to its own display again is remade on Wayland: a flicker.
            if (moved || !was_shown)
                && let Some(surface) = &self.surface
            {
                surface.set_display(place.display.clone());
            }
            self.place = Some(place);
        }
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
    s.checking = Some(card_place(&s.displays, s.pointer.as_mut()));
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
    let just_pressed = pointer.pressed && s.was_pressed == Some(false);
    s.was_pressed = Some(pointer.pressed);
    // During a Price Check its card is about to replace this one wherever the pointer went.
    if s.copy.is_busy() {
        return false;
    }
    let Some(card) = s.bounds.as_ref().filter(|b| b.place == s.place) else {
        return false;
    };
    let Some(place) = &s.place else { return false };
    let at = place.local(pointer.at, card.scale);
    if player_left(card.rect, at, just_pressed) {
        s.hide();
        return true;
    }
    false
}

/// Whether the player has left the card at `rect`, the pointer being `at`, both in local
/// points: pressed a mouse button outside it (`just_pressed`: a button went down since the last
/// sample), or moved beyond [`LEAVE_DISTANCE`].
fn player_left(rect: [f32; 4], (px, py): (f32, f32), just_pressed: bool) -> bool {
    let [x, y, w, h] = rect;
    let dx = (x - px).max(px - (x + w)).max(0.0);
    let dy = (y - py).max(py - (y + h)).max(0.0);
    let distance = dx.hypot(dy);
    distance > LEAVE_DISTANCE || (just_pressed && distance > 0.0)
}

/// The display a Price Check's card opens on, as an index into Rune's `displays`: the one
/// matching `under` (the pointer's display, as the pointer sees it) by connector name, else by
/// geometry; with no `under`, the one holding the pointer `at`; else the primary.
fn card_display(
    displays: &[Display],
    under: Option<&PointerDisplay>,
    at: Option<(i32, i32)>,
) -> Option<usize> {
    let by_name = || {
        let name = under?.name.as_ref()?;
        displays.iter().position(|d| d.name.as_ref() == Some(name))
    };
    let by_geometry = || {
        let under = under?;
        displays
            .iter()
            .position(|d| d.position == under.origin && d.size == under.size)
    };
    // Only without `under`: once the pointer's display matches none, its space isn't Rune's.
    let holding = || {
        let at = at.filter(|_| under.is_none())?;
        displays.iter().position(|d| {
            PointerDisplay {
                name: None,
                origin: d.position,
                size: d.size,
                primary: d.primary,
            }
            .contains(at)
        })
    };
    by_name()
        .or_else(by_geometry)
        .or_else(holding)
        .or_else(|| displays.iter().position(|d| d.primary))
}

/// Where a Price Check started now puts its card: beside the pointer, on its display.
fn card_place(displays: &[Display], pointer: &mut dyn PointerLocator) -> CardPlace {
    let at = pointer.pointer().map(|p| p.at);
    let under = at.and_then(|at| pointer.display(at));
    let chosen = card_display(displays, under.as_ref(), at).map(|i| &displays[i]);
    CardPlace {
        pointer: at,
        // On a display Rune doesn't list, the card goes to the primary at the same spot.
        origin: under
            .map(|d| d.origin)
            .or(chosen.map(|d| d.position))
            .unwrap_or_default(),
        display: chosen
            .and_then(|d| d.id.clone())
            .map_or(DisplayTarget::Primary, DisplayTarget::Id),
    }
}

/// The overlay's keys, once it has them: Esc closes the card, everything else passes.
pub fn on_key(s: &mut PriceCheckOverlay, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key != Key::Escape || s.card.is_none() {
        return false;
    }
    s.hide();
    true
}

/// Where the card's top-left goes in the overlay, in points: beside `pointer` (local points),
/// kept on the overlay of `size` points (zero before its first layout, when no clamping
/// happens).
fn card_origin(pointer: Option<(f32, f32)>, size: (f32, f32)) -> (f32, f32) {
    let Some((px, py)) = pointer else {
        return (POINTER_GAP, POINTER_GAP);
    };
    let clamp = |v: f32, room: f32| {
        if room > 0.0 {
            v.min(room).max(0.0)
        } else {
            v.max(0.0)
        }
    };
    (
        clamp(px + POINTER_GAP, size.0 - CARD_SIZE.0),
        clamp(py + POINTER_GAP, size.1 - CARD_SIZE.1),
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
    let place = s.place.clone();
    let pointer = place
        .as_ref()
        .and_then(|p| Some(p.local(p.pointer?, scale)));
    let (x, y) = card_origin(pointer, (s.screen[2], s.screen[3]));
    let card = rune_kit::Card::new().accent(accent).child(body).build(&k);
    root.child(
        el().key("card")
            .w(CARD_SIZE.0)
            .radius(k.radius.md)
            .fill(k.c.bg_base)
            .border(1.0, k.c.border_base)
            .absolute(x, y)
            .hit()
            // Where the player's pointer is measured from, to close the card once they leave:
            // less the overlay's origin, the card's rect in the display's local points.
            .on_screen_layout(move |s: &mut PriceCheckOverlay, [x, y, w, h]| {
                s.bounds = Some(CardBounds {
                    rect: [x - s.screen[0], y - s.screen[1], w, h],
                    scale,
                    place: place.clone(),
                });
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
    use lootwise_input::{InputError, Pointer};
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
        fn display(&mut self, _: (i32, i32)) -> Option<PointerDisplay> {
            None
        }
    }
    /// A pointer the test moves and presses, counting how often the overlay samples it, on
    /// `displays` as it sees them (none by default: the platform can't say).
    #[derive(Clone, Default)]
    struct Mouse {
        at: Rc<Cell<(i32, i32)>>,
        pressed: Rc<Cell<bool>>,
        samples: Rc<Cell<u32>>,
        displays: Vec<PointerDisplay>,
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
        fn display(&mut self, at: (i32, i32)) -> Option<PointerDisplay> {
            self.displays.iter().find(|d| d.contains(at)).cloned()
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
        watched_by(clipboard, copies, Mouse::at(400, 300))
    }

    /// The same with `mouse`.
    fn watched_by(
        clipboard: &MemoryClipboard,
        copies: &'static str,
        mouse: Mouse,
    ) -> (Runtime<PriceCheckOverlay>, Mouse, Rc<Cell<&'static str>>) {
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
        // The test's frames come every 1/60 s, so a sample lands up to one frame late.
        let fewest = (1.0 / (SAMPLE_EVERY + 1.0 / 60.0)).floor() as u32;
        let most = (1.0 / SAMPLE_EVERY).ceil() as u32 + 1;
        assert!(
            (fewest..=most).contains(&shown),
            "every {SAMPLE_EVERY} s while shown: {shown} in a second"
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
        let size = (1920.0, 1080.0);
        assert_eq!(card_origin(Some((400.0, 300.0)), size), (416.0, 316.0));
        assert_eq!(
            card_origin(Some((1910.0, 1070.0)), size),
            (1600.0, 860.0),
            "kept clear of the edges"
        );
        assert_eq!(card_origin(None, size), (16.0, 16.0), "pointer unknown");
    }

    #[test]
    fn the_pointer_is_measured_from_its_displays_top_left_in_points() {
        let place = CardPlace {
            pointer: None,
            origin: (1920, 0),
            display: DisplayTarget::Primary,
        };
        assert_eq!(place.local((2264, 606), 1.0), (344.0, 606.0));
        assert_eq!(
            place.local((2720, 600), 2.0),
            (400.0, 300.0),
            "pixels become points"
        );
        assert_eq!(
            place.local((1900, 10), 1.0),
            (-20.0, 10.0),
            "left of the display"
        );
    }

    /// The user's desktop: `HDMI-A-2` at `+0+0` and the primary `HDMI-A-1` at `+1920+0`, both
    /// 1920×1080 at scale 1, as the pointer (XWayland) sees them.
    fn pointer_displays() -> Vec<PointerDisplay> {
        let display = |name: &str, x, primary| PointerDisplay {
            name: Some(name.into()),
            origin: (x, 0),
            size: (1920, 1080),
            primary,
        };
        vec![
            display("HDMI-A-1", 1920, true),
            display("HDMI-A-2", 0, false),
        ]
    }

    /// The same as Rune lists them.
    fn overlay_displays() -> Vec<Display> {
        pointer_displays()
            .into_iter()
            .map(|d| Display {
                id: None,
                name: d.name,
                position: d.origin,
                size: d.size,
                scale: 1.0,
                primary: d.primary,
            })
            .collect()
    }

    /// The overlay on [`pointer_displays`], the [`Mouse`] at `at`. Like Rune on Wayland, the
    /// headless overlay's own origin reads `(0, 0)` whichever display it covers.
    fn on_two_displays(
        clipboard: &MemoryClipboard,
        at: (i32, i32),
    ) -> (Runtime<PriceCheckOverlay>, Mouse) {
        let mut mouse = Mouse::at(at.0, at.1);
        mouse.displays = pointer_displays();
        let (mut rt, mouse, _) = watched_by(clipboard, UNIQUE, mouse);
        rt.update(|s, _| s.set_displays(overlay_displays()));
        (rt, mouse)
    }

    #[test]
    fn on_the_primary_right_of_another_display_the_card_opens_beside_the_pointer_and_stays() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = on_two_displays(&clipboard, (2264, 606));
        press_ctrl_d(&mut rt);
        run(&mut rt, 1.0);
        assert!(rt.state.card.is_some(), "the first samples don't close it");
        assert_eq!(
            card_at(&rt),
            Some((360.0, 622.0)),
            "beside the pointer, 344 points into HDMI-A-1"
        );
        assert_eq!(rt.state.place.as_ref().map(|p| p.origin), Some((1920, 0)));
    }

    #[test]
    fn on_the_display_at_the_origin_the_card_opens_beside_the_pointer_and_stays() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, _) = on_two_displays(&clipboard, (500, 400));
        press_ctrl_d(&mut rt);
        run(&mut rt, 1.0);
        assert!(rt.state.card.is_some(), "the first samples don't close it");
        assert_eq!(card_at(&rt), Some((516.0, 416.0)));
        assert_eq!(rt.state.place.as_ref().map(|p| p.origin), Some((0, 0)));
    }

    #[test]
    fn on_another_display_moving_beyond_about_a_card_width_still_closes_the_card() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse) = on_two_displays(&clipboard, (2264, 606));
        press_ctrl_d(&mut rt);
        // The card spans x 360..680 in HDMI-A-1's points, 2280..2600 on the desktop.
        mouse.at.set((2864, 650));
        run(&mut rt, 0.2);
        assert!(rt.state.card.is_some(), "264 points away");
        mouse.at.set((2934, 650));
        run(&mut rt, 0.2);
        assert_eq!(rt.state.card, None, "334 points away");
    }

    #[test]
    fn on_another_display_a_press_outside_the_card_still_closes_it() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse) = on_two_displays(&clipboard, (2264, 606));
        press_ctrl_d(&mut rt);
        click(&mut rt, &mouse, (2400, 700));
        assert!(rt.state.card.is_some(), "a press inside keeps it");
        click(&mut rt, &mouse, (2200, 650));
        assert_eq!(rt.state.card, None);
    }

    #[test]
    fn the_card_opens_on_the_overlay_display_matching_the_pointers() {
        let overlay = overlay_displays();
        let pointer = pointer_displays();
        let (primary, left) = (Some(0), Some(1));
        assert_eq!(
            card_display(&overlay, Some(&pointer[0]), Some((2264, 606))),
            primary
        );
        assert_eq!(
            card_display(&overlay, Some(&pointer[1]), Some((500, 400))),
            left
        );

        // Rune places Wayland outputs in compositor space, which needn't be XWayland's.
        let mut moved = overlay.clone();
        moved[1].position = (-1920, 0);
        assert_eq!(
            card_display(&moved, Some(&pointer[1]), Some((500, 400))),
            left,
            "by connector name"
        );

        let mut unnamed = pointer[1].clone();
        unnamed.name = None;
        assert_eq!(
            card_display(&overlay, Some(&unnamed), Some((500, 400))),
            left,
            "by geometry"
        );
        assert_eq!(
            card_display(&moved, Some(&unnamed), Some((500, 400))),
            primary,
            "no match: the primary"
        );

        assert_eq!(
            card_display(&overlay, None, Some((500, 400))),
            left,
            "the pointer's display unknown: the one holding the pointer"
        );
        assert_eq!(card_display(&overlay, None, None), primary);
        assert_eq!(
            card_display(&[], Some(&pointer[0]), Some((2264, 606))),
            None
        );
    }

    #[test]
    fn a_card_far_from_the_last_one_is_not_closed_by_it() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, mouse, copies) = watched(&clipboard, UNIQUE);
        press_ctrl_d(&mut rt);

        // Straight to an item across the screen, before a sample sees the pointer leave.
        mouse.at.set((1500, 900));
        copies.set(DIVINE);
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Divine Orb"));

        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        run(&mut rt, 0.5);
        mouse.at.set((300, 200));
        copies.set(UNIQUE);
        press_ctrl_d(&mut rt);
        assert_eq!(
            text_of(&rt, "item-name").as_deref(),
            Some("Kaom's Heart"),
            "after the last card was closed far away"
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
