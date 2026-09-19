//! Drag and drop: the state machine is driven by pointer events alone, so it
//! can be tested frame by frame with no window and no GPU.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// Two lists side by side: rows in `left` are drag sources, `right` is a zone.
/// Returns what the zone received this frame.
struct World {
    left: Vec<&'static str>,
    right: Vec<&'static str>,
    /// Set on the frame the drag began.
    started: bool,
    /// The zone is highlighted (a drag it accepts is over it).
    hovered: bool,
    accepts: &'static [&'static str],
}

impl World {
    fn new() -> Self {
        Self { left: vec!["Kick", "Snare", "Hat"], right: vec![], started: false, hovered: false, accepts: &["clip"] }
    }

    fn frame(&mut self, ui: &mut Ui) {
        self.started = false;
        ui.begin_frame(FrameInfo::default());
        ui.container(Layout::row(), Frame::none(), |ui| {
            ui.container(Layout::column().width(Size::Fixed(300.0)), Frame::none(), |ui| {
                for name in self.left.clone() {
                    ui.with_key(name, |ui| {
                        let id = ui.make_id(name);
                        let r = ui.interact_drag(id);
                        let opts = LeafOptions { interactive: true, ..Default::default() };
                        ui.add_leaf_ex(
                            id,
                            Layout::leaf(Size::Grow(1.0), Size::Fixed(24.0)),
                            Vec2::ZERO,
                            opts,
                            |_, _| {},
                        );
                        let d = ui.drag_source_from(&r, || Payload::new("clip", name).with_label(name));
                        self.started |= d.started;
                    });
                }
            });
            ui.container_id(Id::new("right"), Layout::column().width(Size::Fixed(300.0)), Frame::none(), |ui| {
                let z = ui.drop_zone(self.accepts);
                self.hovered = z.hovered;
                if let Some(p) = z.dropped {
                    if let Ok(name) = p.take::<&'static str>() {
                        self.left.retain(|n| *n != name);
                        self.right.push(name);
                    }
                }
                ui.space(10.0);
            });
        });
        ui.drag_ghost();
        ui.end_frame();
    }
}

fn press(ui: &mut Ui, pos: Vec2) {
    ui.push(InputEvent::PointerMoved { pos });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
}

fn move_to(ui: &mut Ui, pos: Vec2) {
    ui.push(InputEvent::PointerMoved { pos });
}

fn release(ui: &mut Ui, pos: Vec2) {
    ui.push(InputEvent::PointerMoved { pos });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
}

/// Rects come from the previous frame, so every scenario needs a warm-up frame.
fn warm(w: &mut World, ui: &mut Ui) {
    w.frame(ui);
    w.frame(ui);
}

#[test]
fn dragging_a_row_into_a_zone_moves_it() {
    let mut ui = ui();
    let mut w = World::new();
    warm(&mut w, &mut ui);

    let row = Vec2::new(40.0, 12.0);
    press(&mut ui, row);
    w.frame(&mut ui);
    assert!(!w.started, "a press alone is not a drag");
    assert_eq!(ui.dragging(), None);

    move_to(&mut ui, row + Vec2::new(60.0, 0.0));
    w.frame(&mut ui);
    assert!(w.started);
    assert_eq!(ui.dragging(), Some("clip"));

    // Over the right-hand column: the zone lights up but has taken nothing.
    move_to(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    w.frame(&mut ui);
    assert!(w.hovered);
    assert!(w.right.is_empty());

    release(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    assert_eq!(w.right, ["Kick"]);
    assert_eq!(w.left, ["Snare", "Hat"]);
    assert_eq!(ui.dragging(), None, "the drag ends when the payload is taken");
}

#[test]
fn a_press_without_movement_is_still_a_click() {
    let mut ui = ui();
    let mut w = World::new();
    warm(&mut w, &mut ui);

    let row = Vec2::new(40.0, 12.0);
    press(&mut ui, row);
    w.frame(&mut ui);
    // Well inside the threshold.
    move_to(&mut ui, row + Vec2::new(1.0, 1.0));
    w.frame(&mut ui);
    assert!(!w.started);
    release(&mut ui, row + Vec2::new(1.0, 1.0));
    w.frame(&mut ui);
    assert_eq!(ui.dragging(), None);
    assert_eq!(w.left.len(), 3, "nothing moved");
}

#[test]
fn releasing_outside_any_zone_drops_nothing() {
    let mut ui = ui();
    let mut w = World::new();
    warm(&mut w, &mut ui);

    press(&mut ui, Vec2::new(40.0, 12.0));
    w.frame(&mut ui);
    move_to(&mut ui, Vec2::new(40.0, 400.0));
    w.frame(&mut ui);
    assert_eq!(ui.dragging(), Some("clip"));

    release(&mut ui, Vec2::new(40.0, 400.0));
    w.frame(&mut ui);
    assert!(w.right.is_empty());
    assert_eq!(w.left.len(), 3);
    assert_eq!(ui.dragging(), None, "an unclaimed payload does not survive the frame");
}

#[test]
fn a_zone_that_rejects_the_kind_never_highlights_or_takes() {
    let mut ui = ui();
    let mut w = World::new();
    w.accepts = &["file"];
    warm(&mut w, &mut ui);

    press(&mut ui, Vec2::new(40.0, 12.0));
    w.frame(&mut ui);
    move_to(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    w.frame(&mut ui);
    assert!(!w.hovered);
    release(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    assert!(w.right.is_empty());
}

#[test]
fn escape_cancels_a_drag() {
    let mut ui = ui();
    let mut w = World::new();
    warm(&mut w, &mut ui);

    press(&mut ui, Vec2::new(40.0, 12.0));
    w.frame(&mut ui);
    move_to(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    assert_eq!(ui.dragging(), Some("clip"));

    ui.push(InputEvent::Key { key: Key::Escape, pressed: true, repeat: false });
    w.frame(&mut ui);
    assert_eq!(ui.dragging(), None);

    release(&mut ui, Vec2::new(400.0, 12.0));
    w.frame(&mut ui);
    assert!(w.right.is_empty(), "the release after a cancel drops nothing");
}

#[test]
fn an_external_drag_routes_like_any_other() {
    let mut ui = ui();
    let mut w = World::new();
    w.accepts = &["clip", "file"];
    warm(&mut w, &mut ui);

    // The host noticed an OS drag entering the window.
    move_to(&mut ui, Vec2::new(400.0, 12.0));
    ui.begin_external_drag(Payload::new("file", "Kick").with_label("kick.wav"));
    w.frame(&mut ui);
    w.frame(&mut ui);
    assert!(w.hovered);

    ui.end_external_drag(true);
    w.frame(&mut ui);
    assert_eq!(w.right, ["Kick"]);
}

#[test]
fn an_external_drag_leaving_the_window_drops_nothing() {
    let mut ui = ui();
    let mut w = World::new();
    w.accepts = &["file"];
    warm(&mut w, &mut ui);

    move_to(&mut ui, Vec2::new(400.0, 12.0));
    ui.begin_external_drag(Payload::new("file", "Kick"));
    w.frame(&mut ui);
    ui.end_external_drag(false);
    w.frame(&mut ui);
    assert_eq!(ui.dragging(), None);
    assert!(w.right.is_empty());
}

#[test]
fn the_innermost_accepting_zone_wins() {
    let mut ui = ui();
    let mut inner_took = false;
    let mut outer_took = false;
    let mut f = |ui: &mut Ui, drop: bool| {
        ui.begin_frame(FrameInfo::default());
        ui.container_id(Id::new("outer"), Layout::column(), Frame::none(), |ui| {
            if ui.drop_zone(&["thing"]).dropped.is_some() {
                outer_took = true;
            }
            ui.space(100.0);
            ui.container_id(
                Id::new("inner"),
                Layout::column().width(Size::Fixed(200.0)).height(Size::Fixed(200.0)),
                Frame::none(),
                |ui| {
                    if ui.drop_zone(&["thing"]).dropped.is_some() {
                        inner_took = true;
                    }
                    ui.space(10.0);
                },
            );
        });
        ui.end_frame();
        let _ = drop;
    };
    f(&mut ui, false);
    // Inside the inner box, which sits below 100px of spacer.
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(50.0, 150.0) });
    ui.begin_external_drag(Payload::new("thing", 1u32));
    f(&mut ui, false);
    f(&mut ui, false);
    ui.end_external_drag(true);
    f(&mut ui, true);
    assert!(inner_took);
    assert!(!outer_took);
}

#[test]
fn a_payload_of_the_wrong_type_comes_back_whole() {
    let p = Payload::new("thing", 7u32).with_label("seven");
    let p = p.take::<String>().expect_err("not a String");
    assert_eq!(p.kind(), "thing");
    assert_eq!(p.label(), "seven");
    assert_eq!(p.get::<u32>(), Some(&7));
    assert_eq!(p.take::<u32>().ok(), Some(7));
}
