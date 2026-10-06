//! What does libgui cost an application, frame by frame, while a user uses it?
//!
//!   cargo run --release -p libgui_bench --bin app
//!
//! The widget benchmark (`main.rs`) times one widget kind in isolation on a
//! steady frame. That is the cheap case: nothing moved, every cache hits. A
//! user is not steady. They drag a splitter and the layout changes every
//! frame; they resize the window; they scroll a table; they sweep the pointer
//! over a list. This measures a CAD-shaped window — menu bar, toolbar, a tree,
//! a viewport, an inspector, a table, a status bar — under each of those, at
//! 1x and 1.5x, and reports the median, p99 and worst frame, because a frame
//! that is fine on average and late once a second is a visible hitch.
//!
//! It also times what a host does with the frame afterwards: expanding it to a
//! mesh, which a bgfx host does every frame.

use libgui::mesh::Mesh;
use libgui::*;
use std::time::{Duration, Instant};

const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");

struct App {
    left_w: f32,
    right_w: f32,
    bottom_h: f32,
    table: TableState,
    names: Vec<String>,
    values: Vec<f32>,
    flags: Vec<bool>,
    text: String,
    // Where things were last frame, for driving input at them.
    right_splitter: Rect,
    table_rect: Rect,
    field_rect: Rect,
}

impl App {
    fn new() -> Self {
        let cols = ["Name", "Type", "Material", "Mass", "Volume", "Layer"].map(|c| Column::new(c).width(120.0));
        Self {
            left_w: 260.0,
            right_w: 320.0,
            bottom_h: 220.0,
            table: TableState::new(cols),
            names: (0..5000).map(|i| format!("Body {i}")).collect(),
            values: vec![0.5; 64],
            flags: vec![true; 64],
            text: "Bracket".into(),
            right_splitter: Rect::default(),
            table_rect: Rect::default(),
            field_rect: Rect::default(),
        }
    }

    fn build(&mut self, ui: &mut Ui) {
        let grow = |l: Layout| l.width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container(grow(Layout::column()), Frame::none(), |ui| {
            // Menu bar and toolbar.
            ui.container(Layout::row().width(Size::Grow(1.0)).gap(4.0), Frame::none(), |ui| {
                for m in ["File", "Edit", "View", "Sketch", "Model", "Help"] {
                    ui.button(m);
                }
            });
            ui.container(Layout::row().width(Size::Grow(1.0)).gap(2.0), Frame::none(), |ui| {
                for i in 0..24 {
                    ui.button_keyed(i, "◇");
                }
            });

            ui.container(grow(Layout::row()), Frame::none(), |ui| {
                // The model tree: a long virtual list.
                let left = Layout::column().width(Size::Fixed(self.left_w)).height(Size::Grow(1.0));
                ui.container(left, Frame::none(), |ui| {
                    ui.heading("Model");
                    let names = &self.names;
                    ui.virtual_list("tree", names.len(), 24.0, |ui, i| {
                        ui.selectable_keyed(i, &names[i], i == 3);
                    });
                });
                ui.splitter("left", &mut self.left_w, SplitterOptions::vertical_rule(120.0, 600.0));

                // The 3D view, with a sketch drawn over it.
                ui.container(grow(Layout::column()), Frame::none(), |ui| {
                    ui.viewport("scene", TextureId::User(1), |p, r| {
                        let ink = Color::rgba(0.4, 0.8, 1.0, 1.0);
                        for k in 0..40 {
                            let x = r.x + r.w * (k as f32 / 40.0);
                            p.line(Vec2::new(x, r.y), Vec2::new(x, r.bottom()), 1.0, ink.with_alpha(0.2));
                        }
                        p.bezier(Vec2::new(r.x, r.y), Vec2::new(r.x + r.w, r.y), Vec2::new(r.x, r.bottom()), Vec2::new(r.x + r.w, r.bottom()), 2.0, ink);
                    });
                });

                let s = ui.splitter("right", &mut self.right_w, SplitterOptions::vertical_rule(200.0, 700.0).inverted());
                self.right_splitter = s.rect;

                // The inspector.
                let right = Layout::column().width(Size::Fixed(self.right_w)).height(Size::Grow(1.0));
                ui.container(right, Frame::none(), |ui| {
                    ui.scroll_area("inspector", |ui| {
                        let r = ui.text_input("name", &mut self.text, "Name");
                        self.field_rect = r.response.rect;
                        for g in 0..8 {
                            ui.with_key(g, |ui| {
                                ui.section("Transform");
                                for i in 0..6 {
                                    let k = g * 8 + i;
                                    ui.slider_keyed(i, "Value", &mut self.values[k], 0.0, 1.0);
                                }
                                ui.checkbox("Visible", &mut self.flags[g * 8]);
                                ui.toggle("Locked", &mut self.flags[g * 8 + 1]);
                            });
                        }
                    });
                });
            });

            ui.splitter("bottom", &mut self.bottom_h, SplitterOptions::horizontal_rule(80.0, 600.0).inverted());
            let bottom = Layout::column().width(Size::Grow(1.0)).height(Size::Fixed(self.bottom_h));
            let bottom_id = Id::new("bottom_panel");
            if let Some(r) = ui.rect_of(bottom_id) {
                self.table_rect = r;
            }
            ui.container_id(bottom_id, bottom, Frame::none(), |ui| {
                let names = &self.names;
                ui.table("parts", &mut self.table, 2000, |ui, row, col| match col {
                    0 => ui.label(&names[row]),
                    1 => ui.label("Solid"),
                    2 => ui.label("Aluminium 6061"),
                    3 => ui.label("1.204 kg"),
                    4 => ui.label("446 cm³"),
                    _ => ui.label_muted("Default"),
                });
            });

            ui.container(Layout::row().width(Size::Grow(1.0)).gap(16.0), Frame::none(), |ui| {
                ui.label_muted("Ready");
                ui.label_muted("X 12.000  Y 40.500  Z 0.000");
                ui.flex();
                ui.label_muted("mm");
            });
        });
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Scenario {
    /// The host redraws every frame for its own reasons; nothing changed.
    Steady,
    /// The pointer sweeps down the tree, one row a frame.
    Hover,
    /// The viewport's right edge dragged back and forth by fractional pixels.
    DragEdge,
    /// The window resized every frame, as a live resize does.
    Resize,
    /// The table scrolled continuously.
    Scroll,
    /// A character typed into a field every frame.
    Typing,
}

struct Run {
    ui: Ui,
    app: App,
    size: Vec2,
    scale: f32,
    mesh: Mesh,
}

impl Run {
    fn new(scale: f32) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut b = KeyBindings::new();
        b.bind(Shortcut::plain(Key::Backspace), UiAction::Delete(Motion::Left));
        ui.set_key_bindings(b);
        Self { ui, app: App::new(), size: Vec2::new(1600.0, 1000.0), scale, mesh: Mesh::new() }
    }

    /// One frame: (build + end_frame, mesh expansion, instances).
    fn frame(&mut self) -> (Duration, Duration, usize) {
        let info = FrameInfo { screen_size: self.size, scale: self.scale, dt: 1.0 / 60.0 };
        let t0 = Instant::now();
        self.ui.begin_frame(info);
        self.app.build(&mut self.ui);
        let out = self.ui.end_frame();
        let t1 = Instant::now();
        self.mesh.build(out.draw);
        let t2 = Instant::now();
        (t1 - t0, t2 - t1, out.draw.instances.len())
    }

    fn input(&mut self, s: Scenario, i: usize) {
        match s {
            Scenario::Steady => {}
            Scenario::Hover => {
                let y = 120.0 + (i % 30) as f32 * 24.0;
                self.ui.push(InputEvent::PointerMoved { pos: Vec2::new(100.0, y) });
            }
            Scenario::DragEdge => {
                if i == 0 {
                    let c = self.app.right_splitter.center();
                    self.ui.push(InputEvent::PointerMoved { pos: c });
                    self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
                } else {
                    // Back and forth, never on a whole pixel.
                    let phase = (i / 40).is_multiple_of(2);
                    let dx = if phase { -3.37 } else { 3.37 };
                    let at = self.app.right_splitter.center() + Vec2::new(dx, 0.0);
                    self.ui.push(InputEvent::PointerMoved { pos: at });
                }
            }
            Scenario::Resize => {
                let phase = (i / 40).is_multiple_of(2);
                self.size.x += if phase { -7.0 } else { 7.0 };
                self.size.y += if phase { -3.0 } else { 3.0 };
            }
            Scenario::Scroll => {
                let c = self.app.table_rect.center();
                self.ui.push(InputEvent::PointerMoved { pos: c });
                let dy = if (i / 60).is_multiple_of(2) { -40.0 } else { 40.0 };
                self.ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, dy), unit: WheelUnit::Pixel });
            }
            Scenario::Typing => {
                if i == 0 {
                    let c = self.app.field_rect.center();
                    self.ui.push(InputEvent::PointerMoved { pos: c });
                    self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
                } else if i == 1 {
                    self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
                } else if i.is_multiple_of(2) {
                    self.ui.push(InputEvent::Text("a".into()));
                } else {
                    self.ui.push(InputEvent::Key { key: Key::Backspace, pressed: true, repeat: false });
                    self.ui.push(InputEvent::Key { key: Key::Backspace, pressed: false, repeat: false });
                }
            }
        }
    }
}

fn pct(v: &mut [Duration], p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn ms(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64() * 1e3)
}

fn main() {
    println!(
        "libgui application benchmark ({})\n",
        if cfg!(debug_assertions) { "DEBUG - numbers are meaningless, use --release" } else { "release" }
    );
    println!(
        "{:<10} {:>5} {:>9} {:>9} {:>9} {:>11} {:>10} {:>10}",
        "scenario", "scale", "median", "p99", "worst", "mesh median", "instances", "% of 60Hz"
    );
    println!("{}", "-".repeat(82));
    let frames = 600;
    for scale in [1.0, 1.5] {
        for s in [
            Scenario::Steady,
            Scenario::Hover,
            Scenario::DragEdge,
            Scenario::Resize,
            Scenario::Scroll,
            Scenario::Typing,
        ] {
            let mut run = Run::new(scale);
            // Warm: layout settles, glyphs rasterise, rects exist to aim at.
            for _ in 0..60 {
                run.frame();
            }
            let mut times = Vec::with_capacity(frames);
            let mut meshes = Vec::with_capacity(frames);
            let mut inst = 0;
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let mut changed = 0usize;
            let mut prev_digest = 0u64;
            for i in 0..frames {
                run.input(s, i);
                let (t, m, n) = run.frame();
                times.push(t);
                meshes.push(m);
                inst = n;
                lo = lo.min(run.app.right_w);
                hi = hi.max(run.app.right_w);
                // Did the picture change? A cheap digest of the instance rects.
                let d = run.mesh.vertices.iter().fold(0u64, |h, v| h.wrapping_mul(31).wrapping_add(v.pos[0].to_bits() as u64 ^ (v.pos[1].to_bits() as u64) << 1));
                changed += (d != prev_digest) as usize;
                prev_digest = d;
            }
            if std::env::var("VERIFY").is_ok() {
                eprintln!("  {s:?}@{scale}: right_w {lo:.2}..{hi:.2}, frames that changed {changed}/{frames}, text {:?}", run.app.text.len());
            }
            let med = pct(&mut times, 0.5);
            let p99 = pct(&mut times, 0.99);
            let worst = *times.iter().max().unwrap();
            let mesh = pct(&mut meshes, 0.5);
            let share = (med + mesh).as_secs_f64() / (1.0 / 60.0) * 100.0;
            println!(
                "{:<10} {:>5} {:>9} {:>9} {:>9} {:>11} {:>10} {:>9.2}%",
                format!("{s:?}"),
                scale,
                ms(med),
                ms(p99),
                ms(worst),
                ms(mesh),
                inst,
                share
            );
        }
        println!();
    }
    // Transitions: the frames a steady benchmark never sees.
    let mut run = Run::new(1.0);
    let (first, _, _) = run.frame();
    let (second, _, _) = run.frame();
    for _ in 0..30 {
        run.frame();
    }
    run.scale = 1.5; // the window dragged to a denser monitor
    let (rescale, _, _) = run.frame();
    let (after, _, _) = run.frame();
    println!("first frame {} ms, second {} ms; DPI change frame {} ms, the one after {} ms", ms(first), ms(second), ms(rescale), ms(after));
    println!();
    println!("times in ms. '% of 60Hz' is median build + mesh as a share of a 16.7 ms frame.");
}
