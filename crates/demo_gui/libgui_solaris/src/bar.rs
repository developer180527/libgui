//! The three full-width strips: the menu bar, the shelf, and the timeline.

use crate::widgets::*;
use crate::Editor;
use libgui::*;

const TITLE: &str = "/media/alex/DWork/Grafik/3d_Szenen//24Bulb/24_bulb_v3.hiplc \
                     - Houdini Indie Limited-Commercial 19.0.589 - Python 3";

pub fn menu_bar(ui: &mut Ui, app: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(21.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(2.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("menubar"), row, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        for m in ["File", "Edit", "Render", "Assets", "Windows", "RenderMan", "Help"] {
            ui.menu_button(m, |ui| {
                for item in ["New", "Open…", "Save"] {
                    let _ = ui.menu_item(item);
                }
            });
        }
        ui.space(6.0);
        desk(ui, "desk1", "Solaris", &mut app.view_tab, 92.0);
        desk(ui, "desk2", "Main", &mut app.param_tab, 92.0);
        // The title sits in the middle of the bar, not after the menus.
        let title = ui.frame_text(TITLE);
        let size = t.metrics.font_size;
        let fg = t.palette.text_muted;
        let id = ui.make_id("title");
        ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
            p.text_centered(r, size, fg, title);
        });
        desk(ui, "desk3", "Main", &mut app.net_tab, 84.0);
    });
}

/// The desktop pickers: a boxed name with a pair of arrows.
fn desk(ui: &mut Ui, key: &str, label: &str, _sel: &mut usize, w: f32) {
    let t = ui.theme.clone();
    let f = Frame { fill: t.palette.bg_inset, border: t.palette.border_strong, border_width: 1.0, radius: 2.0, ..Frame::none() };
    let row = Layout::row()
        .width(Size::Fixed(w))
        .height(Size::Fixed(16.0))
        .padding(Insets::xy(5.0, 0.0))
        .align(Align::Start, Align::Center);
    let __id = ui.make_id(("desk", key));
        ui.container_id(__id, row, f, |ui| {
        swatch(ui, ("deskicon", key), t.palette.text_faint);
        ui.space(4.0);
        ui.label(label);
        ui.flex();
        let c = t.palette.text_faint;
        let id = ui.make_id(("deskarrow", key));
        ui.add_leaf(id, Layout::leaf(Size::Fixed(9.0), Size::Fixed(12.0)), Vec2::ZERO, false, move |p, r| {
            p.chevron(Rect::new(r.x, r.y - 2.0, r.w, r.h), 7.0, Chevron::Up, c);
            p.chevron(Rect::new(r.x, r.y + 2.0, r.w, r.h), 7.0, Chevron::Down, c);
        });
    });
}

const SHELF_TOOLS: [&str; 8] = [
    "Test Geometry C.",
    "Test Geometry P.",
    "Test Geometry R.",
    "Test Geometry S.",
    "Test Geometry S.",
    "Test Geometry T.",
    "Test Geometry T.",
    "Test Geometry T.",
];

const LIGHT_TOOLS: [&str; 7] =
    ["Camera", "Point Light", "Spot Light", "Area Light", "Geometry Light", "Distant Light", "Environment Light"];

pub fn shelf(ui: &mut Ui, app: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Fixed(66.0));
    ui.container_id(Id::new("shelf"), col, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        // Two tab strips: the shelf's own on the left, the light sets on the
        // right, each with its own `+`.
        let strip = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0));
        ui.container_id(Id::new("shelfstrips"), strip, Frame::none(), |ui| {
            let left = Layout::row().width(Size::Grow(0.55)).height(Size::Grow(1.0));
            ui.container_id(Id::new("shelfleft"), left, Frame::none(), |ui| {
                tabs(ui, "shelf", &mut app.shelf_tab, &["Test Geometry", "RenderMan 24"]);
            });
            let right = Layout::row().width(Size::Grow(0.45)).height(Size::Grow(1.0));
            ui.container_id(Id::new("shelfright"), right, Frame::none(), |ui| {
                let mut one = 0;
                tabs(ui, "lights", &mut one, &["LOP Lights and Cameras"]);
            });
        });
        let tools = Layout::row()
            .width(Size::Grow(1.0))
            .height(Size::Grow(1.0))
            .padding(Insets::xy(4.0, 2.0))
            .gap(2.0);
        ui.container_id(Id::new("shelftools"), tools, Frame::none(), |ui| {
            for (i, label) in SHELF_TOOLS.iter().enumerate() {
                shelf_tool(ui, ("geo", i), label, Color::hex(0xb04a3a), 56.0);
            }
            ui.flex();
            for (i, label) in LIGHT_TOOLS.iter().enumerate() {
                let c = if i == 0 { Color::hex(0x6f7f95) } else { Color::hex(0xd8a83c) };
                shelf_tool(ui, ("light", i), label, c, 56.0);
            }
            ui.space(8.0);
        });
    });
}

pub fn timeline(ui: &mut Ui, app: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(44.0))
        .padding(Insets::xy(4.0, 2.0))
        .gap(4.0)
        .align(Align::Start, Align::Center);
    ui.container_id(Id::new("timeline"), row, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).gap(2.0);
        ui.container_id(Id::new("tlcol"), col, Frame::none(), |ui| {
            let top = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0)).gap(3.0).align(Align::Start, Align::Center);
            ui.container_id(Id::new("tltop"), top, Frame::none(), |ui| {
                tool_row(ui, "transport", 6, 16.0);
                ui.space(4.0);
                frame_field(ui, "frame", app.frame);
                ui.space(6.0);
                ruler(ui, app);
                ui.space(8.0);
                let w = Layout::row().width(Size::Fixed(122.0)).height(Size::Grow(1.0)).align(Align::Start, Align::Center);
                let __id = ui.make_id("keycount");
                ui.container_id(__id, w, Frame::none(), |ui| ui.label_muted("0 keys, 0/0 channels"));
            });
            let bottom =
                Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(16.0)).gap(3.0).align(Align::Start, Align::Center);
            ui.container_id(Id::new("tlbottom"), bottom, Frame::none(), |ui| {
                tool_row(ui, "tlb", 4, 14.0);
                ui.space(4.0);
                frame_field(ui, "start", 1.0);
                ui.flex();
                frame_field(ui, "end", 240.0);
                ui.space(6.0);
                ui.label_muted("Key Selected");
                ui.space(4.0);
                tool_row(ui, "tlc", 2, 14.0);
            });
        });
    });
}

fn frame_field(ui: &mut Ui, key: &str, value: f32) {
    let t = ui.theme.clone();
    let f = Frame { fill: t.palette.bg_inset, border: t.palette.border_strong, border_width: 1.0, radius: 2.0, ..Frame::none() };
    let row = Layout::row()
        .width(Size::Fixed(38.0))
        .height(Size::Fixed(15.0))
        .padding(Insets::xy(4.0, 0.0))
        .align(Align::Start, Align::Center);
    let __id = ui.make_id(("framefield", key));
        ui.container_id(__id, row, f, |ui| {
        ui.label(&format!("{value:.0}"));
    });
}

/// The frame ruler: ticks every 24 frames, a playhead at the current one.
fn ruler(ui: &mut Ui, app: &Editor) {
    let t = ui.theme.clone();
    let id = ui.make_id("ruler");
    let size = t.metrics.font_size_small;
    let (fg, line, bg) = (t.palette.text_faint, t.palette.border_strong, t.palette.bg_inset);
    let accent = t.palette.accent;
    let frame = app.frame;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(16.0)), Vec2::ZERO, true, move |p, r| {
        p.rect(r, bg, 2.0);
        let total = 240.0f32;
        for n in (24..=240).step_by(24) {
            let x = r.x + r.w * (n as f32 / total);
            let tick = p.hairline(x, r.y + r.h - 4.0, 1.0, 4.0);
            p.rect(tick, line, 0.0);
            let label = Rect::new(x - 14.0, r.y, 28.0, r.h - 4.0);
            p.text_centered(label, size, fg, format!("{n}"));
        }
        let x = r.x + r.w * (frame / total);
        p.rect(p.hairline(x, r.y, 1.0, r.h), accent, 0.0);
    });
}
