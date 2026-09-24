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
