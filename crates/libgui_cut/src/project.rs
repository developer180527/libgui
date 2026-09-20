//! The project bin: a path bar, a search field, and the thumbnails.

use libgui::*;

use crate::state::{Editor, Media};
use crate::widgets::{divider, icon_button, Icon};

pub fn panel(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container(col, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        path_bar(ui, ed);
        search_row(ui, ed);
        grid(ui, ed);
        footer(ui, ed);
    });
}

fn path_bar(ui: &mut Ui, ed: &Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(22.0))
        .padding(Insets::xy(6.0, 0.0))
        .gap(6.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame::none(), |ui| {
        let _ = icon_button(ui, "bin", Icon::Folder, 16.0, false);
        ui.text_with(&format!("{}.prproj", ed.project), t.metrics.font_size, t.palette.text);
    });
}

fn search_row(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(28.0))
        .padding(Insets::xy(6.0, 0.0))
        .gap(6.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame::none(), |ui| {
        ui.container(
            Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(20.0)).gap(4.0).align(Align::Start, Align::Center),
            Frame::none(),
            |ui| {
                let _ = icon_button(ui, "find", Icon::Search, 16.0, false);
                let mut q = String::new();
                ui.text_input("search", &mut q, "");
            },
        );
        let _ = icon_button(ui, "sort", Icon::Sort, 18.0, false);
        let _ = t;
        let _ = ed;
    });
}

/// The thumbnails. A real bin virtualises this; ten assets do not need it, and
/// the demo's outliner already shows that trick.
fn grid(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let selected = ed.selected_asset;
    let count = ed.assets.len();
    let mut picked = None;
    ui.container(
        Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(18.0)).padding(Insets::xy(8.0, 0.0)).align(Align::Start, Align::Center),
        Frame::none(),
        |ui| {
            ui.text_with(&format!("1 of {count} items selected"), t.metrics.font_size, t.palette.text_muted);
        },
    );
    let opts = ScrollOptions { padding: Insets::all(8.0), gap: 8.0, ..ScrollOptions::new(Size::Grow(1.0)) };
    ui.scroll_area_with("bin", opts, |ui| {
        // Two across, as the reference's panel width gives.
        for chunk in 0..count.div_ceil(2) {
            ui.container(
                Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(126.0)).gap(8.0),
                Frame::none(),
                |ui| {
                    for i in (chunk * 2)..(chunk * 2 + 2).min(count) {
                        if thumbnail(ui, ed, i, selected == i) {
                            picked = Some(i);
                        }
                    }
                },
            );
        }
    });
    if let Some(i) = picked {
        ed.selected_asset = i;
    }
}

/// One asset: a painted frame, a name, a duration, and the badges an NLE puts
/// in the corners.
fn thumbnail(ui: &mut Ui, ed: &Editor, i: usize, selected: bool) -> bool {
    let t = ui.theme.clone();
    let a = &ed.assets[i];
    let id = ui.make_id(("asset", i));
    let r = ui.interact(id);
    if r.hovered {
        ui.cursor = Cursor::Pointer;
    }
    let hot = ui.animate_bool(id, 0, r.hovered);
    let (c0, c1) = a.tint;
    let (name, dur) = (ui.frame_text(a.name), ui.frame_text(a.duration));
    let media = a.media;
    let size = t.metrics.font_size_small;
    let accent = t.palette.accent;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| {
        let fill = if selected { Color::hex(0x3a3a3a) } else { Color::hex(0x2a2a2a).lerp(Color::hex(0x333333), hot) };
        p.rect_bordered(rect, fill, 3.0, 1.0, if selected { accent } else { Color::hex(0x191919) });
        let img = Rect::new(rect.x + 4.0, rect.y + 4.0, rect.w - 8.0, rect.h - 32.0);
        // The "frame": a graded sky over a ground, or a waveform for audio.
        match media {
            Media::Audio => {
                p.rect(img, Color::hex(0x1b3b49), 2.0);
                let mid = img.center().y;
                for k in 0..(img.w as usize / 3) {
                    let x = img.x + k as f32 * 3.0;
                    let h = ((k as f32 * 0.7).sin().abs() * 0.5 + (k as f32 * 0.23).cos().abs() * 0.5) * img.h * 0.4;
                    p.rect(Rect::new(x, mid - h * 0.5, 2.0, h.max(2.0)), c1, 0.0);
                }
            }
            _ => {
                let bands = 12;
                for b in 0..bands {
                    let f = b as f32 / bands as f32;
                    p.rect(Rect::new(img.x, img.y + img.h * f, img.w, img.h / bands as f32 + 1.0), c0.lerp(c1, f), 0.0);
                }
                p.rect(Rect::new(img.x, img.y + img.h * 0.62, img.w, img.h * 0.38), c0.lerp(Color::hex(0x2f5d33), 0.6), 0.0);
            }
        }
        // Corner badges: the film-strip mark and the "has audio" mark.
        let badge = Rect::new(img.right() - 24.0, img.bottom() - 12.0, 22.0, 10.0);
        p.rect(badge, Color::rgba(0.0, 0.0, 0.0, 0.55), 1.5);
        for k in 0..5 {
            p.rect(Rect::new(badge.x + 2.0 + k as f32 * 4.0, badge.y + 2.0, 2.0, 6.0), Color::hex(0xbfc7cf), 0.0);
        }
        p.text_left(Rect::new(rect.x + 6.0, rect.bottom() - 26.0, rect.w - 60.0, 14.0), size, Color::hex(0xdcdcdc), name);
        p.text_right(Rect::new(rect.x, rect.bottom() - 26.0, rect.w - 6.0, 14.0), size, Color::hex(0x9a9a9a), dur);
    });
    r.clicked
}

/// The bare zoom slider a bin wears: no label, no readout.
fn thumb_slider(ui: &mut Ui, ed: &mut Editor) {
    let id = ui.make_id("thumb");
    let r = ui.interact_drag(id);
    if r.active {
        let f = ((r.mouse_pos.x - r.rect.x) / r.rect.w.max(1.0)).clamp(0.0, 1.0);
        ed.thumb = f;
    }
    if r.hovered {
        ui.cursor = Cursor::Pointer;
    }
    let f = ed.thumb;
    ui.add_leaf(id, Layout::leaf(Size::Fixed(104.0), Size::Fixed(20.0)), Vec2::ZERO, true, move |p, rect| {
        let line = Rect::new(rect.x + 6.0, rect.center().y - 1.0, rect.w - 12.0, 2.0);
        p.rect(line, Color::hex(0x3d3d3d), 1.0);
        let x = line.x + line.w * f;
        p.rect(Rect::new(x - 5.0, rect.center().y - 5.0, 10.0, 10.0), Color::hex(0xb0b0b0), 5.0);
    });
}

/// The row of view modes and bin tools along the bottom.
fn footer(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(28.0))
        .padding(Insets::xy(6.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame { fill: Color::hex(0x1f1f1f), ..Frame::none() }, |ui| {
        let _ = icon_button(ui, "pen", Icon::Pen, 20.0, true);
        let _ = icon_button(ui, "list", Icon::List, 20.0, false);
        let _ = icon_button(ui, "grid", Icon::Grid, 20.0, true);
        let _ = icon_button(ui, "free", Icon::Freeform, 20.0, false);
        divider(ui, "p1", true, 16.0);
        thumb_slider(ui, ed);
        ui.flex();
        let _ = icon_button(ui, "sortbin", Icon::Sort, 20.0, false);
        let _ = icon_button(ui, "cols", Icon::Panel, 20.0, false);
        let _ = icon_button(ui, "zoomin", Icon::Zoom, 20.0, false);
        let _ = icon_button(ui, "newbin", Icon::NewBin, 20.0, false);
        let _ = icon_button(ui, "trash", Icon::Trash, 20.0, false);
        let _ = t;
    });
}
