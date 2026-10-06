//! The colour picker, driven the way a user drives it.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const W: f32 = 240.0;
const SQ: f32 = 160.0;

fn bindings() -> KeyBindings {
    let mut b = KeyBindings::new();
    b.bind(Shortcut::plain(Key::Enter), UiAction::InsertNewline).bind(Shortcut::plain(Key::Escape), UiAction::Cancel);
    b
}

struct World {
    ui: Ui,
    color: Color,
    opts: ColorPickerOptions,
    last: ColorPickerResponse,
    finished: usize,
}

impl World {
    fn new(color: Color) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        let mut w = Self { ui, color, opts: ColorPickerOptions::default(), last: Default::default(), finished: 0 };
        for _ in 0..3 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) -> ColorPickerResponse {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        let col = Layout::column().width(Size::Fixed(W)).height(Size::Fit);
        let mut r = ColorPickerResponse::default();
        let (color, opts) = (&mut self.color, self.opts);
        self.ui.container(col, Frame::none(), |ui| r = ui.color_picker_with("c", color, opts));
        let _ = self.ui.end_frame();
        self.finished += r.finished as usize;
        self.last = r;
        r
    }

    fn gap(&self) -> f32 {
        self.ui.theme.metrics.space
    }

    /// Where the strips are, from the picker's own layout rules.
    fn hue_y(&self) -> f32 {
        SQ + self.gap() + 7.0
    }
    fn alpha_y(&self) -> f32 {
        SQ + self.gap() + 14.0 + self.gap() + 7.0
    }

    /// Press at `from`, drag through `to`, release.
    fn drag(&mut self, from: Vec2, to: &[Vec2]) {
        self.ui.push(InputEvent::PointerMoved { pos: from });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        for &p in to {
            self.ui.push(InputEvent::PointerMoved { pos: p });
            self.frame();
        }
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
    }

    fn click(&mut self, at: Vec2) {
        self.drag(at, &[]);
    }
}

fn close(a: Color, b: Color) -> bool {
    a.to_array().iter().zip(b.to_array()).all(|(x, y)| (x - y).abs() < 0.02)
}

const RED: Color = Color::rgba(1.0, 0.0, 0.0, 1.0);

#[test]
fn the_square_picks_saturation_across_and_value_down() {
    let mut w = World::new(RED);
    w.click(Vec2::new(1.0, 1.0));
    assert!(close(w.color, Color::WHITE), "the top-left is not white: {:?}", w.color);
    w.click(Vec2::new(W - 1.0, 1.0));
    assert!(close(w.color, RED), "the top-right is not the hue: {:?}", w.color);
    w.click(Vec2::new(W * 0.5, SQ - 1.0));
    assert!(close(w.color, Color::rgba(0.0, 0.0, 0.0, 1.0)), "the bottom is not black: {:?}", w.color);
    w.click(Vec2::new(W * 0.5, SQ * 0.5));
    assert!(close(w.color, Color::rgba(0.5, 0.25, 0.25, 1.0)), "the middle is not half of half red: {:?}", w.color);
}

/// The reason the picker keeps its own state: grey has no hue and black has
/// no saturation, and a round trip through either must not lose them.
#[test]
fn the_hue_survives_a_trip_through_grey_and_black() {
    let mut w = World::new(Color::rgba(0.0, 0.4, 1.0, 1.0)); // a blue
    // To black — past the bottom, so value is exactly zero — and back up.
    w.drag(Vec2::new(W - 1.0, 1.0), &[Vec2::new(W - 1.0, SQ + 40.0), Vec2::new(W - 1.0, 1.0)]);
    let (r, g, b, _) = (w.color.r, w.color.g, w.color.b, 0);
    assert!(b > 0.9 && r < 0.05 && g > 0.3, "the hue was lost passing through black: {:?}", w.color);
    // To grey — past the left edge, saturation exactly zero — and back.
    w.drag(Vec2::new(W - 1.0, 1.0), &[Vec2::new(-40.0, 1.0), Vec2::new(W - 1.0, 1.0)]);
    assert!(w.color.b > 0.9 && w.color.r < 0.05, "the hue was lost passing through grey: {:?}", w.color);
}

/// A picker that goes away forgets what it was showing, so a long session
/// does not keep the state of every picker it ever opened. A grey colour has
/// no hue of its own, so a picker that comes back fresh starts from red.
#[test]
fn a_picker_that_goes_away_forgets() {
    let mut w = World::new(Color::rgba(0.0, 0.4, 1.0, 1.0));
    w.drag(Vec2::new(W - 1.0, 1.0), &[Vec2::new(-40.0, 1.0)]); // to grey, remembering blue
    w.ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
    let _ = w.ui.end_frame(); // a frame without it
    w.frame();
    w.click(Vec2::new(W - 1.0, 1.0));
    assert!(w.color.r > 0.9 && w.color.b < 0.05, "a picker that went away still remembered its hue: {:?}", w.color);
}

/// Changed from outside *to* grey or black — an undo, a reset — the picker
/// keeps the hue and saturation it was showing rather than jumping to red, so
/// dragging back out returns to where the user was.
#[test]
fn an_outside_grey_or_black_keeps_what_the_picker_was_showing() {
    let mut w = World::new(Color::rgba(0.0, 0.4, 1.0, 1.0));
    w.color = Color::rgba(0.5, 0.5, 0.5, 1.0);
    w.frame();
    w.click(Vec2::new(W - 1.0, 1.0));
    assert!(w.color.b > 0.9 && w.color.r < 0.05, "an outside grey threw the hue away: {:?}", w.color);

    // Black has no saturation, so what keeping it changes is where the handle
    // is drawn: it stays where the user left it instead of jumping to the
    // left edge. Read from the draw output — the handle is the 12 px ring
    // with a white border.
    let mut w = World::new(Color::rgba(1.0, 0.5, 0.5, 1.0)); // saturation 0.5
    w.color = Color::rgba(0.0, 0.0, 0.0, 1.0);
    w.ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
    let col = Layout::column().width(Size::Fixed(W)).height(Size::Fit);
    let color = &mut w.color;
    w.ui.container(col, Frame::none(), |ui| {
        ui.color_picker("c", color);
    });
    let out = w.ui.end_frame();
    let ring = out
        .draw
        .instances
        .iter()
        .find(|i| i.rect[2] == 12.0 && i.rect[3] == 12.0 && i.border_color == Color::WHITE.to_array())
        .map(|i| i.rect[0] + 6.0)
        .expect("no handle drawn");
    assert!((ring - W * 0.5).abs() < 1.5, "an outside black moved the handle to {ring}, not {}", W * 0.5);
}

#[test]
fn a_colour_changed_outside_is_what_the_picker_edits() {
    let mut w = World::new(RED);
    w.color = Color::rgba(0.0, 1.0, 0.0, 1.0); // an undo, another panel
    w.frame();
    // Halving the saturation must keep green.
    w.click(Vec2::new(W * 0.5, 1.0));
    assert!(close(w.color, Color::rgba(0.5, 1.0, 0.5, 1.0)), "the outside change was not picked up: {:?}", w.color);
}

#[test]
fn a_still_picker_never_writes_the_colour() {
    // A colour HSV cannot reproduce bit for bit must not drift by being looked at.
    let start = Color::rgba(0.123, 0.456, 0.789, 0.5);
    let mut w = World::new(start);
    for _ in 0..30 {
        let r = w.frame();
        assert!(!r.changed);
    }
    assert_eq!(w.color.to_array(), start.to_array(), "an untouched picker changed the colour");
}

#[test]
fn a_drag_changes_every_frame_and_finishes_once() {
    let mut w = World::new(RED);
    let mut changes = 0;
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(10.0, 10.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    changes += w.frame().changed as usize;
    for i in 0..10 {
        w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0 + i as f32 * 15.0, 20.0 + i as f32 * 10.0) });
        let r = w.frame();
        assert!(r.dragging);
        assert!(!r.finished, "the drag finished before it was let go");
        changes += r.changed as usize;
    }
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    w.frame();
    w.frame();
    assert!(changes >= 10, "a drag reported {changes} changes over 11 frames of movement");
    assert_eq!(w.finished, 1, "one drag, one undo step");
}

#[test]
fn the_hue_strip_moves_the_hue_and_keeps_the_rest() {
    let mut w = World::new(Color::rgba(1.0, 0.5, 0.5, 1.0)); // s 0.5, v 1
    let y = w.hue_y();
    w.click(Vec2::new(W * 0.5, y));
    assert!(close(w.color, Color::rgba(0.5, 1.0, 1.0, 1.0)), "half way round is not cyan at the same s and v: {:?}", w.color);
}

#[test]
fn the_alpha_strip_sets_alpha_and_can_be_left_out() {
    let mut w = World::new(RED);
    let y = w.alpha_y();
    w.click(Vec2::new(W * 0.25, y));
    assert!((w.color.a - 0.25).abs() < 0.02, "alpha is {}", w.color.a);
    assert!(close(Color::rgba(w.color.r, w.color.g, w.color.b, 1.0), RED), "alpha changed the colour");

    let mut w = World::new(RED);
    w.opts.alpha = false;
    w.frame();
    w.frame();
    w.click(Vec2::new(W * 0.25, y)); // where the strip would be: now the hex field or nothing
    assert_eq!(w.color.a, 1.0, "a picker without alpha changed alpha");
}

#[test]
fn the_hex_field_commits_and_refuses() {
    let mut w = World::new(RED);
    // The field is the last thing in the column.
    let y = w.alpha_y() + 7.0 + w.gap() + 12.0;
    let type_and_enter = |w: &mut World, t: &str| {
        w.click(Vec2::new(W * 0.5, y));
        w.ui.push(InputEvent::Text(t.into()));
        w.frame();
        w.ui.push(InputEvent::Key { key: Key::Enter, pressed: true, repeat: false });
        let r = w.frame();
        w.ui.push(InputEvent::Key { key: Key::Enter, pressed: false, repeat: false });
        w.frame();
        r
    };
    let r = type_and_enter(&mut w, "#00ff00");
    assert!(close(w.color, Color::rgba(0.0, 1.0, 0.0, 1.0)), "#00ff00 gave {:?}", w.color);
    assert!(r.finished, "a committed hex value is an edit");
    type_and_enter(&mut w, "0000ff"); // no '#': people type that too
    assert!(close(w.color, Color::rgba(0.0, 0.0, 1.0, 1.0)), "0000ff gave {:?}", w.color);
    let before = w.color;
    type_and_enter(&mut w, "nope");
    assert_eq!(w.color.to_array(), before.to_array(), "refused text changed the colour");
}

#[test]
fn a_swatch_opens_a_picker_that_edits_the_colour() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut color = RED;
    let frame = |ui: &mut Ui, color: &mut Color| {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 500.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.color_button("layer", color);
        let _ = ui.end_frame();
        (r, ui.any_popup_open())
    };
    for _ in 0..3 {
        frame(&mut ui, &mut color);
    }
    // The swatch is at the top-left.
    let at = Vec2::new(10.0, 10.0);
    ui.push(InputEvent::PointerMoved { pos: at });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui, &mut color);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let (_, open) = frame(&mut ui, &mut color);
    assert!(open, "clicking the swatch did not open the picker");
    for _ in 0..3 {
        frame(&mut ui, &mut color);
    }
    // Find the square the popup drew: the white fill under its first
    // gradient, the only thing exactly as tall as the square.
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 500.0), scale: 1.0, dt: 1.0 / 60.0 });
    ui.color_button("layer", &mut color);
    let out = ui.end_frame();
    let sq = out
        .draw
        .instances
        .iter()
        .find(|i| i.rect[3] == ColorPickerOptions::default().square_height && i.color == Color::WHITE.to_array())
        .map(|i| i.rect)
        .expect("the popup drew no square");
    drop(out);
    let corner = Vec2::new(sq[0] + 1.0, sq[1] + 1.0);
    ui.push(InputEvent::PointerMoved { pos: corner });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    frame(&mut ui, &mut color);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    frame(&mut ui, &mut color);
    assert!(close(color, Color::WHITE), "the picker in the popup did not edit the colour: {color:?}");
}
