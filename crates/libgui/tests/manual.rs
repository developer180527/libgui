//! The code in `MANUAL.md`, compiled and run.
//!
//! A manual whose examples do not compile is worse than no manual: it costs a
//! reader more to discover the lie than it would have cost them to read the
//! source. Four of these examples were wrong when they were first written —
//! `open_container` takes an id, `Budget::steady_frame` is
//! `testing::steady_frame`, `Renderer::new` takes the queue, `Restored` has no
//! `unplaced` — and only compiling them found it.
//!
//! Keep this in step with the document. If an example here changes, the
//! document changes with it.
use libgui::*;
const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

#[allow(dead_code)]
fn frame_loop(ui: &mut Ui, screen_size: Vec2, scale: f32, dt: f32) {
    ui.begin_frame(FrameInfo { screen_size, scale, dt });
    let _ = ui.end_frame();
}

#[allow(dead_code)]
fn containers(ui: &mut Ui, theme: &Theme) {
    ui.container_id(Id::new("sidebar"),
        Layout::column().width(Size::Fixed(240.0)).height(Size::Grow(1.0))
            .padding(Insets::all(8.0)).gap(6.0),
        Frame { fill: theme.palette.bg_panel, ..Frame::none() },
        |ui| { ui.label("x"); });
    ui.open_container(Id::new("row"), Layout::row(), Frame::none());
    ui.close_container();
}

#[allow(dead_code)]
fn nav(ui: &mut Ui, rows: &[String], selected: &mut usize) {
    let nav = ui.open_collection("hierarchy", rows.len());
    if nav.moved { *selected = nav.cursor; }
    for (i, row) in rows.iter().enumerate() {
        let r = ui.selectable_keyed(i, row, *selected == i);
        if r.clicked { *selected = i; ui.set_cursor(nav.id, i); }
        if nav.moved && nav.cursor == i { ui.scroll_to(r.id); }
    }
    ui.close_collection();
}

struct Param { source: String }
struct Doc;
impl Doc {
    fn check_expression(&self, _t: &str) -> Result<f64, String> { Ok(0.0) }
    fn reevaluate(&mut self) {}
}

#[allow(dead_code)]
fn validated(ui: &mut Ui, param: &mut Param, doc: &mut Doc, shown: String) {
    let r = ui.validated_input_with("height", &mut param.source, &ValidatedOptions {
        display: Some(&shown),
        ..Default::default()
    }, |text| doc.check_expression(text).map(|_| ()).map_err(|e| FieldError::new(e.to_string())));
    if r.committed { doc.reevaluate(); }
}

fn pick(_at: Vec2, _r: Rect) -> Color { Color::WHITE }

#[allow(dead_code)]
fn own_widget(ui: &mut Ui, key: &str, value: &mut Color) {
    let id = ui.make_id(("swatch", key));
    let r = ui.interact_focusable_drag(id, FocusKind::Control);
    if r.active { *value = pick(r.mouse_pos, r.rect); }
    let hot = ui.animate_bool(id, 0, r.hovered);
    let c = *value;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(24.0)), Vec2::ZERO, true,
        move |p, rect| p.rect_bordered(rect, c, 4.0, 1.0 + hot, p.theme.palette.border));
}

#[allow(dead_code)]
fn colour(ui: &mut Ui, layer: &mut Color, push_undo: impl FnOnce()) {
    if ui.color_picker("layer", layer).finished {
        push_undo();
    }
    ui.color_button("swatch", layer);
    let id = ui.make_id("g");
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(8.0)), Vec2::ZERO, false,
        |p, r| p.gradient(r, Color::WHITE, Color::BLACK, Axis::X));
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PartId(usize);
struct PartNode { name: String, kids: Vec<PartId> }
struct Assembly { top: Vec<PartId>, nodes: Vec<PartNode>, picked: std::collections::HashSet<PartId> }
impl std::ops::Index<PartId> for Assembly {
    type Output = PartNode;
    fn index(&self, i: PartId) -> &PartNode { &self.nodes[i.0] }
}
impl TreeSource for Assembly {
    type Key = PartId;
    fn roots(&self, out: &mut Vec<PartId>) { out.extend(&self.top) }
    fn children(&self, n: PartId, out: &mut Vec<PartId>) { out.extend(&self[n].kids) }
    fn has_children(&self, n: PartId) -> bool { !self[n].kids.is_empty() }
    fn label(&self, n: PartId) -> std::borrow::Cow<'_, str> { self[n].name.as_str().into() }
    fn selected(&self, n: PartId) -> bool { self.picked.contains(&n) }
}

#[allow(dead_code)]
fn tree(ui: &mut Ui, model: &Assembly, tree_state: &mut TreeState<PartId>) {
    let r = ui.tree_view("assembly", tree_state, model);
    let _ = r.expanded;
}

#[allow(dead_code)]
fn custom_draw(ui: &mut Ui, color: Color, ink: Color) {
    let id = ui.make_id("custom");
    let text = ui.frame_text("hello");
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(40.0)), Vec2::ZERO, true,
        move |p: &mut Painter, r: Rect| {
            p.rect(r, color, 4.0);
            p.text_left(r, 13.0, ink, text);
        });
}

#[allow(dead_code)]
fn styling(ui: &mut Ui) {
    ui.with_style(|t| { t.button.radius = 0.0; }, |ui| { ui.button("Square"); });
}

#[allow(dead_code)]
fn shortcuts(ui: &mut Ui) -> bool {
    ui.consume_shortcut(Shortcut::plain(Key::S).logo())
}

#[allow(dead_code)]
fn fonts() -> Result<(Ui, Ui), FontError> {
    let a = Ui::new(Theme::dark(), FONT)?;
    let b = Ui::with_fallbacks(Theme::dark(), &[FONT, FONT])?;
    Ok((a, b))
}

#[derive(Clone)]
struct MyTab(&'static str);
struct MyViewer;
impl TabViewer for MyViewer {
    type Tab = MyTab;
    fn title(&self, t: &MyTab) -> String { t.0.into() }
    fn id(&self, t: &MyTab) -> u64 { Id::from_name(t.0).0 }
    fn ui(&mut self, ui: &mut Ui, t: &mut MyTab) { ui.label(t.0); }
    fn scroll(&self, _t: &MyTab) -> bool { true }
    fn padding(&self, _t: &MyTab) -> Insets { Insets::all(8.0) }
}

#[test]
fn the_manuals_dock_example_works() {
    let mut dock = DockState::new();
    let left = dock.leaf(vec![MyTab("Hierarchy")]);
    let right = dock.leaf(vec![MyTab("Scene"), MyTab("Game")]);
    let root = dock.split(Axis::X, 0.25, left, right);
    dock.set_root(SurfaceId::MAIN, root);
    dock.config.floating_mode = FloatingMode::InApp;

    dock.set_pointer(Vec2::new(10.0, 10.0), false);
    dock.set_surface_frame(SurfaceId::MAIN, Vec2::ZERO, 1.0);
    dock.update();
    assert!(!dock.surfaces().is_empty());

    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut viewer = MyViewer;
    for _ in 0..2 {
        ui.begin_frame(FrameInfo::default());
        dock.show(&mut ui, SurfaceId::MAIN, &mut viewer);
        let _ = ui.end_frame();
    }

    // Persistence, exactly as the manual writes it.
    let layout = dock.layout(&viewer);
    let toml = layout.to_toml().expect("to_toml");
    let back = DockLayout::from_toml(&toml).expect("from_toml");
    let restored = dock.restore(&back, |id| {
        ["Hierarchy", "Scene", "Game"].iter().find(|n| Id::from_name(n).0 == id).map(|n| MyTab(n))
    }).expect("restore");
    assert!(!restored.placed.is_empty());
    let all = ["Hierarchy", "Scene", "Game"].iter().map(|n| Id::from_name(n).0);
    assert!(restored.missing_from(all).is_empty(), "everything was placed");
}

#[test]
fn the_manuals_testing_example_works() {
    use libgui::testing::{steady_frame, Budget};
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let cost = steady_frame(&mut ui, FrameInfo::default(), |ui| { ui.label("steady"); });
    Budget::steady(120).assert(&cost);

    // And the plain form the manual shows first.
    ui.begin_frame(FrameInfo::default());
    ui.label("x");
    let out = ui.end_frame();
    assert!(!out.draw.instances.is_empty());
}

/// §5.8: a drawn icon, kept by the app and filled each frame; and a loaded
/// one with its alpha counted.
#[allow(dead_code)]
fn icons(ui: &mut Ui, id: Id, ink: Color, tex: TextureId) {
    use std::rc::Rc;
    let play = Rc::new(
        Path::new(24.0, 24.0)
            .move_to(Vec2::new(6.0, 4.0))
            .line_to(Vec2::new(20.0, 12.0))
            .line_to(Vec2::new(6.0, 20.0))
            .close(),
    );
    let icon = play.clone();
    ui.add_leaf(id, Layout::leaf(Size::Fixed(16.0), Size::Fixed(16.0)), Vec2::ZERO, true, move |p, r| p.fill_path(&icon, r, ink));
    ui.add_leaf(id.with("png"), Layout::leaf(Size::Fixed(16.0), Size::Fixed(16.0)), Vec2::ZERO, true, move |p, r| {
        p.image_with_alpha(r, tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE, ImageAlpha::Straight)
    });
}

/// §5.9: springs, and handing a throw over.
#[allow(dead_code)]
fn springs(ui: &mut Ui, id: Id, drawer_open: bool, target_x: f32, x: f32, r: Response) {
    let _open = ui.animate_spring(id, 0, if drawer_open { 1.0 } else { 0.0 });
    let _x = ui.animate_spring_with(id, 1, target_x, Spring::new(0.4, 0.6));
    if r.released {
        let v = ui.pointer_velocity().x;
        ui.set_spring(id, 0, x, v);
    } else if r.active {
        ui.set_spring(id, 0, x, 0.0);
    }
    let _ = Spring::SNAPPY.value_at(0.1);
}

#[allow(dead_code)]
fn virtual_pair(ui: &mut Ui, names: &[String]) {
    for i in ui.open_virtual_list("objects", names.len(), ListOptions::new(24.0)) {
        ui.open_virtual_row(i);
        ui.label(&names[i]);
        ui.close_virtual_row();
    }
    ui.close_virtual_list();
}

#[allow(dead_code)]
fn dashes(p: &mut Painter, a: Vec2, b: Vec2, outline: &[Vec2], ink: Color, t: f32) {
    p.dashed_line(a, b, 1.0, ink, Dash::even(4.0));                  // construction line
    p.dashed_line(a, b, 2.0, ink, Dash::dotted(2.0));                // dotted
    p.dashed_polyline(outline, 1.0, ink, Dash::new(6.0, 3.0));       // hidden edge
    p.dashed_line(a, b, 1.0, ink, Dash::even(4.0).phase(t * 20.0));  // marching ants
}

fn angle_of(a: Vec2, b: Vec2) -> f32 { (b.y - a.y).atan2(b.x - a.x) }

#[allow(dead_code, clippy::too_many_arguments)]
fn turned(p: &mut Painter, r: Rect, knob_tex: TextureId, axis_mid: Vec2, a: Vec2, b: Vec2, ink: Color, angle: f32) {
    use std::f32::consts::FRAC_PI_2;
    p.image_rotated(r, knob_tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE, ImageAlpha::Straight, angle);
    p.text_rotated(axis_mid, 12.0, ink, "Height (mm)", -FRAC_PI_2);   // reads bottom to top
    p.text_rotated((a + b) * 0.5, 11.0, ink, "42.0 mm", angle_of(a, b)); // along a dimension line
}

#[allow(dead_code)]
fn scopes(ui: &mut Ui, ms: &[f32], head: usize, left: &[f32], right: &[f32], status: &mut String) {
    // A streaming history in a fixed ring buffer: nothing is shifted or copied.
    ui.scope("frame time", &[ScopeTrace::new(Trace::ring(ms, head)).filled().label("ms")],
        &ScopeOptions { range: Some((0.0, 33.3)), ..Default::default() });

    // Several traces, the range fitted to the data.
    let r = ui.scope("signals", &[ScopeTrace::new(left), ScopeTrace::new(right)], &ScopeOptions::default());
    if let Some(i) = r.index(left.len()) { *status = format!("sample {i}"); }
}

#[allow(dead_code)]
fn meters(ui: &mut Ui, left_db: f32, right_db: f32, load_now: f32, load_1s: f32) {
    let opts = MeterOptions::audio_db();            // -60..0 dB, zones at -18 and -6, LEDs, clip light
    ui.row(|ui| {
        ui.meter("L", left_db, &opts);
        ui.meter("R", right_db, &opts);
    });
    ui.meter_with_average("cpu", load_now, load_1s, &MeterOptions { zones: Some((0.7, 0.9)), ..Default::default() });
}
