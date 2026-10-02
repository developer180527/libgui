//! The application bar: window buttons, the workspace tabs an editor names
//! after the job (Import / Edit / Export), the project title, and the right
//! side's utilities.

use libgui::*;

use crate::state::Editor;
use crate::widgets::{icon_button, Icon};

pub fn bar(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(38.0))
        .padding(Insets::xy(10.0, 0.0))
        .gap(6.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame { fill: Color::hex(0x1c1c1c), ..Frame::none() }, |ui| {
        traffic_lights(ui);
        ui.space(10.0);
        let _ = icon_button(ui, "home", Icon::Home, 24.0, false);
        ui.space(8.0);
        for (i, name) in ["Import", "Edit", "Export"].iter().enumerate() {
            workspace_tab(ui, i, name, &mut ed.top_tab);
        }
        ui.flex();
        title(ui, ed);
        ui.flex();
        let _ = icon_button(ui, "prog", Icon::Panel, 24.0, false);
        let _ = icon_button(ui, "share", Icon::Share, 24.0, false);
        let _ = icon_button(ui, "menu", Icon::Menu, 24.0, false);
        let _ = icon_button(ui, "full", Icon::Expand, 24.0, false);
        let _ = t;
    });
}

fn traffic_lights(ui: &mut Ui) {
    let id = ui.make_id("lights");
    ui.add_leaf(id, Layout::leaf(Size::Fixed(56.0), Size::Fixed(14.0)), Vec2::ZERO, false, |p, r| {
        for (i, c) in [0xff5f57u32, 0xfebc2e, 0x28c840].iter().enumerate() {
            let x = r.x + i as f32 * 20.0;
            p.rect(Rect::new(x, r.center().y - 6.0, 12.0, 12.0), Color::hex(*c), 6.0);
        }
    });
}

/// One of the workspace names. The active one is white with an underline; the
/// rest are muted, as the reference has them.
fn workspace_tab(ui: &mut Ui, i: usize, name: &str, selected: &mut usize) {
    let t = ui.theme.clone();
    let id = ui.make_id(("ws", i));
    let r = ui.interact(id);
    if r.clicked {
        *selected = i;
    }
    if r.hovered {
        ui.cursor = Cursor::Pointer;
    }
    let on = *selected == i;
    let hot = ui.animate_bool(id, 0, r.hovered);
    let size = 13.0;
    let w = ui.fonts.measure(ui.font, size, name).x + 20.0;
    let text = ui.frame_text(name);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Fixed(30.0)), Vec2::ZERO, true, move |p, rect| {
        let c = if on { Color::hex(0xf2f2f2) } else { t.palette.text_muted.lerp(Color::hex(0xd8d8d8), hot) };
        p.text_centered(rect, size, c, text);
        if on {
            let tw = p.measure(size, text).x;
            p.rect(Rect::new(rect.center().x - tw * 0.5, rect.bottom() - 5.0, tw, 2.0), Color::hex(0xf2f2f2), 1.0);
        }
    });
}

fn title(ui: &mut Ui, ed: &Editor) {
    let t = ui.theme.clone();
    let id = ui.make_id("title");
    let name = ui.frame_text(ed.project);
    let state = ui.frame_text(" - Edited");
    let size = 14.0;
    ui.add_leaf(id, Layout::leaf(Size::Fit, Size::Fixed(20.0)), Vec2::new(150.0, 20.0), false, move |p, rect| {
        let w = p.measure(size, name).x;
        p.text_left(rect, size, Color::hex(0xe8e8e8), name);
        p.text_left(rect.shrink(w, 0.0, 0.0, 0.0), size, t.palette.text_muted, state);
    });
}
