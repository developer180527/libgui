//! The demo driven the way a person drives it, through the real keymap.

use libgui::*;
use libgui_colors::{theme_for, Colors};

const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");
const SIZE: Vec2 = Vec2::new(1100.0, 780.0);

struct World {
    ui: Ui,
    app: Colors,
}

impl World {
    fn new() -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let platform = libgui_keymap::Platform::Windows;
        ui.set_key_bindings(libgui_keymap::ui_bindings(platform));
        ui.focus_policy = libgui_keymap::full_keyboard_access(platform);
        let mut w = Self { ui, app: Colors::default() };
        for _ in 0..4 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) -> FrameOutput<'_> {
        self.ui.begin_frame(FrameInfo { screen_size: SIZE, scale: 1.0, dt: 1.0 / 60.0 });
        self.app.ui(&mut self.ui);
        self.ui.end_frame()
    }

    fn step(&mut self) {
        drop(self.frame());
    }

    fn rect(&self, id: Id) -> Rect {
        self.ui.rect_of(id).expect("not built")
    }

    fn click(&mut self, at: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.step();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.step();
    }

    fn key(&mut self, k: Key) {
        self.ui.push(InputEvent::Key { key: k, pressed: true, repeat: false });
        if k == Key::Space {
            self.ui.push(InputEvent::Text(" ".into()));
        }
        self.step();
        self.ui.push(InputEvent::Key { key: k, pressed: false, repeat: false });
        self.step();
    }

    /// The inline picker's square: the only thing exactly 160 px tall with a
    /// white fill.
    fn square(&mut self) -> Rect {
        let out = self.frame();
        let r = out.draw.instances.iter().find(|i| i.rect[3] == 160.0 && i.color == Color::WHITE.to_array()).map(|i| i.rect);
        drop(out);
        let r = r.expect("no inline square");
        Rect::new(r[0], r[1], r[2], r[3])
    }
}

#[test]
fn the_theme_switch_changes_the_whole_window() {
    let mut w = World::new();
    let dark = w.ui.theme.palette.bg_app;
    w.app.theme = 2;
    w.step();
    assert_ne!(w.ui.theme.palette.bg_app.to_array(), dark.to_array(), "Light did not change the background");
    assert_eq!(w.ui.theme.name, "Light");
}

#[test]
fn a_picked_accent_rebuilds_the_theme_around_it() {
    let mut w = World::new();
    let stock = w.ui.theme.palette.accent;
    w.app.accent_theme = true;
    w.app.accent = Color::rgba(0.9, 0.2, 0.6, 1.0);
    w.step();
    assert_eq!(w.ui.theme.palette.accent.to_array(), [0.9, 0.2, 0.6, 1.0], "the accent did not reach the theme");
    // Not the palette alone: the widget styles built from it follow.
    assert_eq!(w.ui.theme, theme_for(0, Some(w.app.accent)), "the theme was not rebuilt from the palette");
    w.app.accent_theme = false;
    w.step();
    assert_eq!(w.ui.theme.palette.accent.to_array(), stock.to_array(), "switching it off did not restore the stock accent");
}

#[test]
fn one_drag_is_one_recent_colour() {
    let mut w = World::new();
    let sq = w.square();
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(sq.x + 10.0, sq.y + 10.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.step();
    for i in 0..12 {
        w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(sq.x + 20.0 + i as f32 * 12.0, sq.y + 20.0 + i as f32 * 6.0) });
        w.step();
    }
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    w.step();
    w.step();
    assert_eq!(w.app.recent.len(), 1, "a drag left {} recent colours, not one", w.app.recent.len());
    assert_eq!(w.app.recent[0].to_array(), w.app.accent.to_array());
}

#[test]
fn a_recent_colour_goes_back_to_the_picker() {
    let mut w = World::new();
    let sq = w.square();
    w.click(Vec2::new(sq.x + 5.0, sq.y + 5.0));
    let first = w.app.accent;
    w.click(Vec2::new(sq.right() - 5.0, sq.bottom() - 30.0));
    assert_ne!(w.app.accent.to_array(), first.to_array());
    // Find the first pick's swatch by its colour in the recent row.
    let row = w.rect(Id::new("recent"));
    let out = w.frame();
    let swatch = out
        .draw
        .instances
        .iter()
        .find(|i| i.rect[2] == 20.0 && i.rect[1] >= row.y && i.color == first.to_array())
        .map(|i| Vec2::new(i.rect[0] + 10.0, i.rect[1] + 10.0));
    drop(out);
    w.click(swatch.expect("the first pick is not in the recent row"));
    assert_eq!(w.app.accent.to_array(), first.to_array(), "clicking a recent colour did not restore it");
}

#[test]
fn a_layer_swatch_opens_from_the_keyboard_and_its_picker_is_adjusted_from_it() {
    let mut w = World::new();
    // Tab through the inline picker (square, hue, alpha, hex), the theme
    // switch's neighbours, to the first swatch. Count stops rather than
    // hard-code them: keep tabbing until a popup can be opened.
    let before = w.app.layers[0];
    let mut opened = false;
    for _ in 0..20 {
        w.key(Key::Tab);
        w.key(Key::Space);
        if w.ui.any_popup_open() {
            opened = true;
            break;
        }
    }
    assert!(opened, "no swatch could be opened from the keyboard");
    // Inside the popup: Tab to its square, and the arrows adjust the colour.
    w.key(Key::Tab);
    w.key(Key::ArrowDown);
    w.key(Key::ArrowDown);
    assert_ne!(w.app.layers[0].to_array(), before.to_array(), "the arrows did not adjust the swatch's colour");
    w.key(Key::Escape);
    assert!(!w.ui.any_popup_open(), "Escape did not close the swatch's picker");
}
