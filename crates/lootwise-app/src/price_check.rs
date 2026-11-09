//! The Price Check overlay: its state, its view, and the Ctrl+D sequence that fills it.
//!
//! The overlay covers the display under the pointer, transparent and click-through except for
//! its card ([`OverlayInput::Regions`]), so the game keeps focus. The card sits next to where the
//! pointer was when the player pressed Ctrl+D. It takes keys only once the player clicks it
//! ([`KeyPolicy::OnDemand`]), and then hears Esc alone; Ctrl+D closes it either way.

use lootwise_input::{ClipboardCopy, KeySender, PointerLocator, Tick};
use rune::{ClipboardContent, Color, Cx, El, Key, KeyEvent, OverlayHandle, Tx, col, el, text};
use rune_kit::Kit;

use crate::header::{ItemHeader, parse_header};

pub const NOT_AN_ITEM: &str = "Not a Path of Exile item";
/// The card's width, and the height kept clear for it at the bottom of the display, in points.
const CARD_SIZE: (f32, f32) = (320.0, 140.0);
/// The card's offset from the pointer, in points.
const POINTER_GAP: f32 = 16.0;

/// What the card shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Card {
    Item(ItemHeader),
    NotAnItem,
    /// The copy keystroke couldn't be sent.
    Failed(String),
}

impl Card {
    fn from_clipboard(text: &str) -> Self {
        parse_header(text).map_or(Card::NotAnItem, Card::Item)
    }
}

/// The overlay's state.
pub struct PriceCheck {
    keys: Box<dyn KeySender>,
    pointer: Box<dyn PointerLocator>,
    copy: ClipboardCopy<ClipboardContent>,
    card: Option<Card>,
    /// Global desktop pixels, read when the card opened.
    anchor: Option<(i32, i32)>,
    /// The overlay's rect in global points, from `on_screen_layout`.
    screen: [f32; 4],
    /// The native overlay, absent in headless runs.
    surface: Option<OverlayHandle<PriceCheck>>,
}

impl PriceCheck {
    pub fn new(keys: Box<dyn KeySender>, pointer: Box<dyn PointerLocator>) -> Self {
        Self {
            keys,
            pointer,
            copy: ClipboardCopy::new(),
            card: None,
            anchor: None,
            screen: [0.0; 4],
            surface: None,
        }
    }

    /// Lets the overlay show and hide its native surface.
    pub fn attach(&mut self, surface: OverlayHandle<PriceCheck>) {
        self.surface = Some(surface);
    }

    fn show(&mut self, card: Card) {
        self.anchor = self.pointer.pointer();
        self.card = Some(card);
        if let Some(surface) = &self.surface {
            surface.show();
        }
    }

    fn hide(&mut self) {
        self.card = None;
        if let Some(surface) = &self.surface {
            surface.hide();
        }
    }
}

/// Ctrl+D: close an open card, else start a Price Check of the item under the pointer.
pub fn shortcut(s: &mut PriceCheck, tx: &mut Tx) {
    if s.card.is_some() {
        s.hide();
        return;
    }
    if !s.copy.begin() {
        return;
    }
    tx.read_clipboard(|s: &mut PriceCheck, saved, tx| {
        if let Some(due) = s.copy.saved(saved.clone(), tx.clock()) {
            tx.copy("");
            tx.wake_at(due);
        }
    });
}

/// Before every frame and at each deadline the sequence asked for.
pub fn poll(s: &mut PriceCheck, clock: f64, tx: &mut Tx) -> bool {
    match s.copy.tick(clock, s.keys.as_mut()) {
        Tick::Idle => false,
        Tick::WakeAt(due) => {
            tx.wake_at(due);
            false
        }
        Tick::ReadClipboard => {
            tx.read_clipboard(|s: &mut PriceCheck, copied, tx| {
                let Some(saved) = s.copy.read() else { return };
                tx.copy_rich(saved);
                s.show(Card::from_clipboard(copied.text()));
            });
            false
        }
        Tick::Failed { restore, error } => {
            tx.copy_rich(restore);
            s.show(Card::Failed(error.to_string()));
            true
        }
    }
}

/// The overlay's keys, once it has them: Esc closes the card, everything else passes.
pub fn on_key(s: &mut PriceCheck, ev: &KeyEvent, _: &mut Tx) -> bool {
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
fn rarity_color(rarity: &str, k: &Kit) -> Color {
    let rgb = |r: u8, g: u8, b: u8| Color {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    };
    match rarity {
        "Normal" => rgb(0xc8, 0xc8, 0xc8),
        "Magic" => rgb(0x88, 0x88, 0xff),
        "Rare" => rgb(0xff, 0xff, 0x77),
        "Unique" => rgb(0xaf, 0x60, 0x25),
        "Gem" => rgb(0x1b, 0xa2, 0x9b),
        "Currency" => rgb(0xaa, 0x9e, 0x82),
        "Divination Card" => rgb(0x0e, 0xba, 0xff),
        _ => k.c.text_base,
    }
}

fn card_body(card: &Card, k: &Kit) -> (Color, El) {
    match card {
        Card::Item(item) => {
            let color = rarity_color(&item.rarity, k);
            let mut body = col().gap(4.0).child(
                text(item.rarity.clone())
                    .key("rarity")
                    .font(k.ty.body)
                    .color(k.c.text_muted),
            );
            for (i, line) in item.name.iter().enumerate() {
                let key = if i == 0 { "item-name" } else { "item-base" };
                body = body.child(
                    text(line.clone())
                        .key(key)
                        .font(k.ty.body_medium)
                        .color(color),
                );
            }
            (color, body)
        }
        Card::NotAnItem => (
            k.c.icon_muted,
            text(NOT_AN_ITEM)
                .key("not-an-item")
                .font(k.ty.body)
                .color(k.c.text_base),
        ),
        Card::Failed(error) => (
            k.c.icon_muted,
            text(error.clone())
                .key("failed")
                .font(k.ty.body)
                .color(k.c.text_base)
                .wrap()
                .w_full(),
        ),
    }
}

pub fn view(s: &PriceCheck, cx: &mut Cx<PriceCheck>) -> El {
    let root = el()
        .key("price-check")
        .w_full()
        .h_full()
        .on_screen_layout(|s: &mut PriceCheck, rect| s.screen = rect);
    let Some(card) = &s.card else { return root };
    let k = Kit::neutral(true);
    let (accent, body) = card_body(card, &k);
    let (x, y) = card_origin(s.anchor, cx.env().scale, s.screen);
    let card = rune_kit::Card::new().accent(accent).child(body).build(&k);
    root.child(
        el().key("card")
            .w(CARD_SIZE.0)
            .radius(k.radius.md)
            .fill(k.c.bg_base)
            .border(1.0, k.c.border_base)
            .absolute(x, y)
            .hit()
            // A click hands the card the keyboard, so Esc reaches it.
            .on_press(|s: &mut PriceCheck| {
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

    const SAMPLE: &str = "Item Class: Rings\nRarity: Rare\nDoom Loop\nTwo-Stone Ring\n--------\nRequirements:\nLevel: 32\n--------\nItem Level: 84\n";

    /// Stands in for the game: Ctrl+C puts `copies` on the clipboard.
    struct FakeGame {
        clipboard: MemoryClipboard,
        copies: &'static str,
        presses: Rc<Cell<u32>>,
    }
    impl KeySender for FakeGame {
        fn send_copy(&mut self) -> Result<(), InputError> {
            self.presses.set(self.presses.get() + 1);
            self.clipboard.set_text(self.copies);
            Ok(())
        }
    }
    struct FakePointer;
    impl PointerLocator for FakePointer {
        fn pointer(&mut self) -> Option<(i32, i32)> {
            Some((400, 300))
        }
    }

    /// The overlay as its host runs it, with `keys` in place of the game's input.
    fn runtime(clipboard: &MemoryClipboard, keys: Box<dyn KeySender>) -> Runtime<PriceCheck> {
        let mut rt = Runtime::new(
            PriceCheck::new(keys, Box::new(FakePointer)),
            view,
            (1920.0, 1080.0),
            Env::new(true, 1.0),
        );
        rt.on_key = Some(on_key);
        rt.use_clipboard(clipboard);
        rt.frame(0.0);
        rt
    }

    fn overlay(
        clipboard: &MemoryClipboard,
        copies: &'static str,
    ) -> (Runtime<PriceCheck>, Rc<Cell<u32>>) {
        let presses = Rc::new(Cell::new(0));
        let game = FakeGame {
            clipboard: clipboard.clone(),
            copies,
            presses: presses.clone(),
        };
        (runtime(clipboard, Box::new(game)), presses)
    }

    /// Frames for `seconds`, polling before each as the overlay host does.
    fn run(rt: &mut Runtime<PriceCheck>, seconds: f64) {
        for _ in 0..(seconds * 60.0).ceil() as u32 {
            rt.update_if(|s, tx| poll(s, tx.clock(), tx));
            rt.frame(1.0 / 60.0);
        }
    }

    fn press_ctrl_d(rt: &mut Runtime<PriceCheck>) {
        rt.update(shortcut);
        run(rt, 0.5);
    }

    fn text_of(rt: &Runtime<PriceCheck>, key: &str) -> Option<String> {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
            .and_then(|e| e.text)
    }

    #[test]
    fn ctrl_d_over_an_item_shows_its_name_and_esc_hides_it() {
        let clipboard = rune::clipboard::memory();
        clipboard.set_text("the player's own clipboard");
        let (mut rt, presses) = overlay(&clipboard, SAMPLE);

        press_ctrl_d(&mut rt);
        assert_eq!(presses.get(), 1, "one Ctrl+C reached the game");
        assert_eq!(text_of(&rt, "rarity").as_deref(), Some("Rare"));
        assert_eq!(text_of(&rt, "item-name").as_deref(), Some("Doom Loop"));
        assert_eq!(text_of(&rt, "item-base").as_deref(), Some("Two-Stone Ring"));

        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        run(&mut rt, 0.5);
        assert_eq!(rt.state.card, None);
        assert_eq!(text_of(&rt, "item-name"), None, "the card is gone");
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
        let mut rt = runtime(&clipboard, Box::new(Nothing));
        press_ctrl_d(&mut rt);
        assert_eq!(text_of(&rt, "not-an-item").as_deref(), Some(NOT_AN_ITEM));
        assert_eq!(
            clipboard.text().as_deref().map(String::as_str),
            Some(SAMPLE),
            "and the old item is back on the clipboard"
        );
    }

    #[test]
    fn ctrl_d_again_closes_the_card() {
        let clipboard = rune::clipboard::memory();
        let (mut rt, presses) = overlay(&clipboard, SAMPLE);
        press_ctrl_d(&mut rt);
        assert!(rt.state.card.is_some());
        press_ctrl_d(&mut rt);
        assert_eq!(rt.state.card, None);
        assert_eq!(presses.get(), 1, "closing sends no Ctrl+C");
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
            (1600.0, 940.0),
            "kept clear of the edges"
        );
        assert_eq!(
            card_origin(Some((2320, 300)), 1.0, [1920.0, 0.0, 1920.0, 1080.0]),
            (416.0, 316.0),
            "on a display right of the primary"
        );
    }
}
