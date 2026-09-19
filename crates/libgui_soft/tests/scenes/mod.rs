//! Scenes for golden-image and GPU-parity tests. Shared by path:
//! `libgui_soft/tests/golden.rs` renders them on the CPU against checked-in
//! PNGs, and `libgui_wgpu/tests/parity.rs` renders them through the real shader
//! to check the CPU renderer against a GPU.
//!
//! A scene is deterministic by construction: libgui's clock is the sum of the
//! frames' `dt`, never the wall clock, and every run uses the same frames.
//! libgui has no platform-dependent behaviour to pin: shortcut hints are text
//! the app passes in.

#![allow(dead_code)] // each including test uses a different subset

use libgui::*;
use std::cell::Cell;

pub const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");

/// Every scene is rendered at each of these DPI scales. 1.5 is where
/// pixel-snapping bugs show.
pub const SCALES: [f32; 3] = [1.0, 1.5, 2.0];

pub type ThemeFn = fn() -> Theme;

pub const THEMES: [(&str, ThemeFn); 2] = [("dark", Theme::dark), ("light", Theme::light)];

/// What the pointer does to the scene's marked widget before the capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pointer {
    None,
    /// Rest over it.
    Hover,
    /// Press and hold on it.
    Press,
    /// Click it, then type into it (focuses a text field with its caret on).
    ClickAndType(&'static str),
    /// Rest over it and wheel by this many pixels.
    Wheel(f32),
}

pub struct Scene {
    pub name: &'static str,
    /// Logical size.
    pub size: (f32, f32),
    pub pointer: Pointer,
    pub build: fn(&mut Ui),
}

thread_local! {
    /// The widget a scene wants the pointer on, from its last build.
    static TARGET: Cell<Rect> = const { Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)) };
    /// The text field's contents in `text_focused`: app state, which must
    /// start empty on every run.
    static FIELD: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Mark `r` as the widget the scene's [`Pointer`] acts on.
pub fn target(r: Rect) {
    TARGET.with(|t| t.set(r));
}

impl Scene {
    /// Physical size at `scale`.
    pub fn pixels(&self, scale: f32) -> (u32, u32) {
        ((self.size.0 * scale).round() as u32, (self.size.1 * scale).round() as u32)
    }

    /// Run the scene's frames and hand the final one to `f`.
    pub fn run<R>(&self, theme: Theme, scale: f32, f: impl FnOnce(&FrameOutput, (u32, u32)) -> R) -> R {
        let mut ui = Ui::new(theme, FONT).expect("font");
        TARGET.with(|t| t.set(Rect::default()));
        FIELD.with(|t| t.borrow_mut().clear());
        // A whole second per frame, so every hover fade and ease has finished
        // by the time it is captured.
        let info = FrameInfo { screen_size: Vec2::new(self.size.0, self.size.1), scale, dt: 1.0 };
        let frame = |ui: &mut Ui| {
            ui.begin_frame(info);
            (self.build)(ui);
            let _ = ui.end_frame();
        };
        // Two frames so layout has settled and the target's rect is known.
        frame(&mut ui);
        frame(&mut ui);

        let at = TARGET.with(|t| t.get()).center();
        let press = |ui: &mut Ui, pressed| {
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed });
        };
        match self.pointer {
            Pointer::None => {}
            Pointer::Hover => ui.push(InputEvent::PointerMoved { pos: at }),
            Pointer::Press => {
                ui.push(InputEvent::PointerMoved { pos: at });
                press(&mut ui, true);
            }
            Pointer::ClickAndType(_) => {
                ui.push(InputEvent::PointerMoved { pos: at });
                press(&mut ui, true);
                press(&mut ui, false);
            }
            Pointer::Wheel(dy) => {
                ui.push(InputEvent::PointerMoved { pos: at });
                ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, dy), unit: WheelUnit::Pixel });
            }
        }
        for _ in 0..3 {
            frame(&mut ui);
        }
        if let Pointer::ClickAndType(text) = self.pointer {
            // Typing in the captured frame restarts the caret blink, so the
            // caret is on in the image.
            if !text.is_empty() {
                ui.push(InputEvent::Text(text.into()));
            }
        }
        ui.begin_frame(info);
        (self.build)(&mut ui);
        let out = ui.end_frame();
        f(&out, self.pixels(scale))
    }
}

/// A padded panel filling the window, as every scene's root.
fn panel(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    let t = ui.theme.clone();
    let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(12.0)).gap(6.0);
    ui.container(layout, Frame::panel(&t), body);
}

/// Every basic widget, at rest.
fn widgets(ui: &mut Ui) {
    panel(ui, |ui| {
        ui.heading("Heading");
        ui.label("A label, and a muted one below.");
        ui.label_muted("Muted label");
        ui.separator();
        ui.row(|ui| {
            let _ = ui.button("Button");
            let _ = ui.button_primary("Primary");
        });
        let (mut on, mut off) = (true, false);
        ui.toggle("Toggle on", &mut on);
        ui.toggle("Toggle off", &mut off);
        ui.checkbox("Checked", &mut on);
        ui.checkbox("Unchecked", &mut off);
        let mut choice = 1;
        ui.row(|ui| {
            ui.radio("One", &mut choice, 0);
            ui.radio("Two", &mut choice, 1);
        });
        let mut v = 0.35;
        ui.slider("Slider", &mut v, 0.0, 1.0);
        let mut d = 12.5;
        ui.drag_value("Drag value", &mut d, 0.1);
        ui.progress("Progress", Some(0.6));
        let mut sel = 0;
        ui.combo("Combo", &mut sel, &["Perspective", "Orthographic"]);
        let mut seg = 1;
        ui.segmented("seg", &mut seg, &["Move", "Rotate", "Scale"]);
        let _ = ui.selectable("Selectable", false);
        let _ = ui.selectable("Selected", true);
        let mut empty = String::new();
        let mut filled = String::from("Some text");
        ui.text_input("empty", &mut empty, "Placeholder…");
        ui.text_input("filled", &mut filled, "");
    });
}

/// Tree rows at several depths, one selected.
fn tree(ui: &mut Ui) {
    panel(ui, |ui| {
        ui.section("Outliner");
        let _ = ui.tree_row("scene", 0, Branch::Expanded, "Scene", false);
        let _ = ui.tree_row("cam", 1, Branch::Leaf, "Camera", false);
        let _ = ui.tree_row("lights", 1, Branch::Collapsed, "Lights", false);
        let _ = ui.tree_row("mesh", 1, Branch::Expanded, "Mesh", true);
        let _ = ui.tree_row("mat", 2, Branch::Leaf, "Material", false);
    });
}

/// The raw primitives: rounded rects, borders, shadows, lines and curves.
/// These exercise the rasteriser directly, independent of any widget style.
fn shapes(ui: &mut Ui) {
    let id = ui.make_id("shapes");
    let fill = Layout::leaf(Size::Grow(1.0), Size::Grow(1.0));
    ui.add_leaf(id, fill, Vec2::ZERO, false, |p, r| {
        let (x, y) = (r.x, r.y);
        let white = Color::hex(0xffffff);
        let accent = Color::hex(0x4c8dff);
        p.rect(Rect::new(x + 12.0, y + 12.0, 60.0, 40.0), accent, 0.0);
        p.rect(Rect::new(x + 84.0, y + 12.0, 60.0, 40.0), accent, 8.0);
        p.rect(Rect::new(x + 156.0, y + 12.0, 40.0, 40.0), accent, 20.0);
        p.rect_bordered(Rect::new(x + 208.0, y + 12.0, 60.0, 40.0), Color::hex(0x202020), 6.0, 2.0, white);
        // A fractional rect and radius, as zoom and odd DPIs produce.
        p.rect(Rect::new(x + 12.3, y + 64.6, 59.5, 30.25), white.with_alpha(0.6), 3.5);
        p.shadow(Rect::new(x + 96.0, y + 68.0, 60.0, 30.0), 6.0, 12.0, Color::rgba(0.0, 0.0, 0.0, 0.6));
        p.rect(Rect::new(x + 96.0, y + 68.0, 60.0, 30.0), Color::hex(0x2a2a2a), 6.0);
        p.shadow(Rect::new(x + 184.0, y + 68.0, 60.0, 30.0), 6.0, 4.0, accent.with_alpha(0.8));
        // Lines: axis-aligned, diagonal, thin, thick, and a curve.
        p.line(Vec2::new(x + 12.0, y + 120.0), Vec2::new(x + 120.0, y + 120.0), 1.0, white);
        p.line(Vec2::new(x + 12.0, y + 132.0), Vec2::new(x + 120.0, y + 180.0), 2.0, white);
        p.line(Vec2::new(x + 12.0, y + 190.0), Vec2::new(x + 120.0, y + 150.0), 6.0, accent.with_alpha(0.7));
        p.bezier(
            Vec2::new(x + 140.0, y + 190.0),
            Vec2::new(x + 200.0, y + 190.0),
            Vec2::new(x + 200.0, y + 120.0),
            Vec2::new(x + 268.0, y + 120.0),
            3.0,
            accent,
        );
    });
}

/// Text at several sizes and colours, including glyphs with descenders,
/// accents and punctuation.
fn text(ui: &mut Ui) {
    panel(ui, |ui| {
        let c = ui.theme.palette.text;
        for size in [10.0, 12.0, 13.0, 16.0, 20.0, 28.0] {
            ui.text_with(&format!("{size}px Quick brown fox — Ágjy, 0123"), size, c);
        }
        let m = ui.theme.palette.text_muted;
        ui.text_with("Muted text in a smaller size", 12.0, m);
    });
}

fn button_target(ui: &mut Ui) {
    panel(ui, |ui| {
        let r = ui.button("Target button");
        target(r.rect);
        let _ = ui.button("Other");
    });
}

fn tooltip_target(ui: &mut Ui) {
    panel(ui, |ui| {
        let r = ui.button("Hover for tip");
        target(r.rect);
        ui.tooltip(&r, "A tooltip, drawn above everything");
    });
}

fn text_input_target(ui: &mut Ui) {
    panel(ui, |ui| {
        FIELD.with(|t| {
            let r = ui.text_input("field", &mut t.borrow_mut(), "Type here");
            target(r.response.rect);
        });
    });
}

fn combo_target(ui: &mut Ui) {
    panel(ui, |ui| {
        let mut sel = 1;
        let r = ui.combo("Projection", &mut sel, &["Perspective", "Orthographic", "Isometric"]);
        target(r.rect);
    });
}

/// An open menu: plain items, a shortcut, a separator and a submenu arrow.
fn menu_target(ui: &mut Ui) {
    panel(ui, |ui| {
        // menu_button returns no response; it is the panel's first child, so
        // its rect starts at the panel padding. Aim a little inside it.
        target(Rect::new(12.0, 12.0, 24.0, 20.0));
        ui.menu_button("File", |ui| {
            let _ = ui.menu_item("New");
            let _ = ui.menu_item_shortcut("Open…", "⌘O");
            ui.menu_separator();
            ui.submenu("Recent", |ui| {
                let _ = ui.menu_item("scene.lvl");
            });
        });
    });
}

fn scroll_target(ui: &mut Ui) {
    panel(ui, |ui| {
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(150.0)), |ui| {
            for i in 0..30 {
                let r = ui.selectable_keyed(i, &format!("Row {i}"), i == 3);
                if i == 0 {
                    target(r.rect);
                }
            }
        });
    });
}

pub const SCENES: &[Scene] = &[
    Scene { name: "widgets", size: (320.0, 640.0), pointer: Pointer::None, build: widgets },
    Scene { name: "tree", size: (240.0, 230.0), pointer: Pointer::None, build: tree },
    Scene { name: "shapes", size: (280.0, 200.0), pointer: Pointer::None, build: shapes },
    Scene { name: "text", size: (360.0, 200.0), pointer: Pointer::None, build: text },
    Scene { name: "button_hover", size: (240.0, 90.0), pointer: Pointer::Hover, build: button_target },
    Scene { name: "button_pressed", size: (240.0, 90.0), pointer: Pointer::Press, build: button_target },
    Scene { name: "tooltip", size: (300.0, 110.0), pointer: Pointer::Hover, build: tooltip_target },
    Scene { name: "text_focused", size: (240.0, 60.0), pointer: Pointer::ClickAndType("Hello"), build: text_input_target },
    Scene { name: "menu_open", size: (260.0, 190.0), pointer: Pointer::ClickAndType(""), build: menu_target },
    Scene { name: "combo_open", size: (260.0, 160.0), pointer: Pointer::ClickAndType(""), build: combo_target },
    // 37px: not a multiple of anything, so a fractional offset would show.
    Scene { name: "scroll_mid", size: (240.0, 180.0), pointer: Pointer::Wheel(-37.0), build: scroll_target },
];

pub fn scene(name: &str) -> &'static Scene {
    SCENES.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no scene {name}"))
}
