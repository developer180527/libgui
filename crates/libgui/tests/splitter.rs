//! [`Ui::splitter`]: a draggable divider for a UI that is not docked.
//!
//! The dock has splitters of its own. An app that lays out three fixed panels
//! around a viewport — which is most professional tools before they adopt
//! docking, and some after — had no way to let the user move a boundary, so
//! its panel widths were constants.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(800.0, 600.0), scale: 1.0, dt: 1.0 / 60.0 };

/// A sidebar of `width`, a splitter, and the rest of the window.
struct World {
    ui: Ui,
    width: f32,
    opts: SplitterOptions,
    resp: Option<Response>,
}

impl World {
    fn new(width: f32, opts: SplitterOptions) -> Self {
        let mut w = Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), width, opts, resp: None };
        w.frame();
        w.frame();
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(INFO);
        let row = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        let (width, opts) = (self.width, self.opts);
        let mut out = None;
        let mut w = width;
        self.ui.container_id(Id::new("root"), row, Frame::none(), |ui| {
            let pane = Layout::column().width(Size::Fixed(width)).height(Size::Grow(1.0));
            ui.container_id(Id::new("pane"), pane, Frame::none(), |ui| ui.label("Inspector"));
            out = Some(ui.splitter("edge", &mut w, opts));
            let rest = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
            ui.container_id(Id::new("rest"), rest, Frame::none(), |ui| ui.label("Scene"));
        });
        self.width = w;
        self.resp = out;
        let _ = self.ui.end_frame();
    }

    /// Where the handle actually is. A `Response` carries the *previous*
    /// frame's rect, so settling first matters once the pane has been resized:
    /// aiming at where the handle used to be lands inside the pane.
    fn handle(&mut self) -> Rect {
        self.frame();
        self.resp.as_ref().expect("no splitter response").rect
    }

    /// Press on the handle, move by `d`, release. One frame per step, because
    /// a drag delta is the change since the previous frame.
    fn drag(&mut self, d: Vec2) {
        let at = self.handle().center();
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        self.ui.push(InputEvent::PointerMoved { pos: at + d });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
    }
}

/// Dragging the rule right widens the pane to its left.
#[test]
fn a_drag_moves_the_pane_it_divides() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(80.0, 600.0));
    w.drag(Vec2::new(60.0, 0.0));
    assert_eq!(w.width, 260.0);
    w.drag(Vec2::new(-100.0, 0.0));
    assert_eq!(w.width, 160.0);
}

/// **The one an app gets wrong first.** A pane docked to the right has its
/// handle on its *leading* edge, so dragging that edge left must make the pane
/// wider, not narrower. Without `inverted` every right-hand inspector in the
/// world resizes backwards.
#[test]
fn an_inverted_splitter_grows_the_pane_on_its_other_side() {
    let opts = SplitterOptions::vertical_rule(80.0, 600.0).inverted();
    let mut w = World::new(200.0, opts);
    w.drag(Vec2::new(-60.0, 0.0));
    assert_eq!(w.width, 260.0, "dragging a right-hand pane's edge left did not widen it");
    w.drag(Vec2::new(60.0, 0.0));
    assert_eq!(w.width, 200.0);
}

/// The range is a clamp, not a suggestion: a pane cannot be dragged shut or
/// dragged over the window.
#[test]
fn the_range_holds_however_far_the_pointer_goes() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(120.0, 300.0));
    w.drag(Vec2::new(-5000.0, 0.0));
    assert_eq!(w.width, 120.0);
    w.drag(Vec2::new(5000.0, 0.0));
    assert_eq!(w.width, 300.0);
}

/// A range given the wrong way round clamps to the same two numbers rather
/// than collapsing the pane to one of them. `clamp` panics if min > max, which
/// would turn a typo in a layout into a crash in the field.
#[test]
fn a_backwards_range_does_not_panic() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(300.0, 120.0));
    w.drag(Vec2::new(-5000.0, 0.0));
    assert_eq!(w.width, 120.0);
}

/// The axis names the rule, not the direction it travels: a vertical rule
/// ignores vertical motion, so a shaky hand does not drift the layout.
#[test]
fn a_vertical_rule_ignores_vertical_motion() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(80.0, 600.0));
    w.drag(Vec2::new(0.0, 120.0));
    assert_eq!(w.width, 200.0);
}

/// A horizontal rule is the other way round: it is dragged vertically, which
/// is what a console docked under a viewport needs.
#[test]
fn a_horizontal_rule_is_dragged_vertically() {
    let mut w = World::new(200.0, SplitterOptions::horizontal_rule(80.0, 600.0));
    w.drag(Vec2::new(0.0, 50.0));
    assert_eq!(w.width, 250.0);
    w.drag(Vec2::new(90.0, 0.0));
    assert_eq!(w.width, 250.0, "a horizontal rule moved sideways");
}

/// The pointer becomes a resize cursor over the handle, and while dragging it
/// — including when the pointer has been dragged off the handle, which is most
/// of any real drag.
#[test]
fn the_cursor_says_the_handle_is_draggable() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(80.0, 600.0));
    assert_eq!(w.ui.cursor, Cursor::Default);

    let at = w.handle().center();
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.frame();
    assert_eq!(w.ui.cursor, Cursor::ResizeHorizontal);

    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerMoved { pos: at + Vec2::new(0.0, 400.0) });
    w.frame();
    assert_eq!(w.ui.cursor, Cursor::ResizeHorizontal, "the cursor reverted mid-drag");

    let mut v = World::new(200.0, SplitterOptions::horizontal_rule(80.0, 600.0));
    let at = v.handle().center();
    v.ui.push(InputEvent::PointerMoved { pos: at });
    v.frame();
    assert_eq!(v.ui.cursor, Cursor::ResizeVertical);
}

/// The grab area is wider than the drawn rule. A 4 px line is a hard target,
/// and `hit_pad` is what makes it forgiving — so a press just off the rule
/// still starts the drag.
#[test]
fn the_grab_area_is_wider_than_the_line() {
    let opts = SplitterOptions::vertical_rule(80.0, 600.0).hit_pad(6.0);
    let mut w = World::new(200.0, opts);
    let r = w.handle();
    // Just outside the drawn rule, inside the pad.
    let at = Vec2::new(r.x + r.w + 4.0, r.center().y);
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerMoved { pos: at + Vec2::new(40.0, 0.0) });
    w.frame();
    assert_eq!(w.width, 240.0, "a press {} px off the rule missed it", 4.0);
}

/// The handle draws nothing at rest and a rule when it is hot, so a splitter
/// costs no ink in a screenshot of an idle window.
#[test]
fn the_rule_appears_only_under_the_pointer() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(80.0, 600.0));
    let ink = |w: &mut World| {
        w.ui.begin_frame(INFO);
        let row = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        let (width, opts) = (w.width, w.opts);
        let mut v = width;
        w.ui.container_id(Id::new("root"), row, Frame::none(), |ui| {
            let pane = Layout::column().width(Size::Fixed(width)).height(Size::Grow(1.0));
            ui.container_id(Id::new("pane"), pane, Frame::none(), |ui| ui.label("Inspector"));
            ui.splitter("edge", &mut v, opts);
        });
        w.ui.end_frame().draw.instances.len()
    };
    let at_rest = ink(&mut w);

    let at = w.handle().center();
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.frame();
    // Let the hover animation settle.
    for _ in 0..30 {
        w.frame();
    }
    assert!(ink(&mut w) > at_rest, "the rule never appeared under the pointer");
}

/// A double click is reported rather than acted on: only the app knows what
/// the pane's default width is, and a widget that restored a number it made up
/// would be policy in the core.
#[test]
fn a_double_click_is_reported_for_the_app_to_act_on() {
    let mut w = World::new(200.0, SplitterOptions::vertical_rule(80.0, 600.0));
    let at = w.handle().center();
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.frame();
    for _ in 0..2 {
        w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        w.frame();
        w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        w.frame();
    }
    assert!(w.resp.as_ref().unwrap().double_clicked, "no double click reached the app");
    assert_eq!(w.width, 200.0, "the splitter reset the width by itself");
}
