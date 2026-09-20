//! The program monitor: the picture, the scrub bar under it, and the transport.
//!
//! The picture is painted rather than decoded — a frame of the edit, in
//! rectangles. In a real host this rect is a `ui.viewport(..)` handed to the
//! decoder's texture, which is a one-line swap.

use libgui::*;

use crate::state::{timecode, Editor};
use crate::theme::REEL;
use crate::widgets::{divider, icon_button, Icon};

pub fn panel(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container(col, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        picture(ui, ed);
        readout(ui, ed);
        scrub(ui, ed);
        transport(ui, ed);
    });
}

/// The image area: letterboxed, with the frame painted inside it.
fn picture(ui: &mut Ui, ed: &mut Editor) {
    let id = ui.make_id("picture");
    let r = ui.interact(id);
    if r.clicked {
        ed.playing = !ed.playing;
    }
    let phase = ed.playhead;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| {
        p.rect(rect, Color::hex(0x141414), 0.0);
        // 16:9 inside the panel.
        let scale = (rect.w / 16.0).min(rect.h / 9.0);
        let (w, h) = (16.0 * scale, 9.0 * scale);
        let f = Rect::new(rect.center().x - w * 0.5, rect.center().y - h * 0.5, w, h);
        frame(p, f, phase);
    });
}

/// A jungle temple, the way the reference's clip looks: sky, clouds, a stepped
/// pyramid, lawn, and leaves hanging into the top corners.
fn frame(p: &mut Painter, f: Rect, phase: f32) {
    let horizon = f.y + f.h * 0.56;
    let sky_top = Color::hex(0x5d93c4);
    let sky_bot = Color::hex(0xd3e0e8);
    let bands = 30;
    for i in 0..bands {
        let t = i as f32 / bands as f32;
        let y = f.y + (horizon - f.y) * t;
        p.rect(Rect::new(f.x, y, f.w, (horizon - f.y) / bands as f32 + 1.0), sky_top.lerp(sky_bot, t), 0.0);
    }
    // Clouds: flat-bottomed piles, drifting with the playhead.
    let drift = (phase * 4.0) % (f.w * 0.5);
    for (cx, cy, cw) in [(0.08f32, 0.14f32, 0.26f32), (0.46, 0.08, 0.30), (0.80, 0.19, 0.22), (0.30, 0.26, 0.18)] {
        let x = f.x + f.w * cx - drift * 0.1;
        let w = f.w * cw;
        let h = f.h * 0.045;
        let c = Color::WHITE.with_alpha(0.82);
        p.rect(Rect::new(x, f.y + f.h * cy, w, h), c, h * 0.45);
        p.rect(Rect::new(x + w * 0.18, f.y + f.h * cy - h * 0.7, w * 0.42, h * 1.3), c, h * 0.6);
        p.rect(Rect::new(x + w * 0.55, f.y + f.h * cy - h * 0.35, w * 0.3, h), c, h * 0.5);
    }
    // Treeline along the horizon.
    for i in 0..30 {
        let t = i as f32 / 30.0;
        let x = f.x + f.w * t;
        let h = f.h * (0.07 + ((i * 13) % 7) as f32 * 0.009);
        let c = Color::hex(0x2c5530).lerp(Color::hex(0x437a41), ((i * 5) % 4) as f32 * 0.3);
        p.rect(Rect::new(x, horizon - h, f.w / 26.0, h + 2.0), c, f.w * 0.008);
    }
    // Ground: lawn, then a lighter strip of path.
    p.rect(Rect::new(f.x, horizon, f.w, f.h - (horizon - f.y)), Color::hex(0x5b8f44), 0.0);
    p.rect(Rect::new(f.x, horizon + f.h * 0.10, f.w, f.h * 0.34), Color::hex(0x6ba151), 0.0);
    p.rect(Rect::new(f.x + f.w * 0.30, horizon + f.h * 0.18, f.w * 0.42, f.h * 0.08), Color::hex(0x9aa06a), 0.0);

    // The stepped pyramid, right of centre.
    let base = Rect::new(f.x + f.w * 0.50, f.y + f.h * 0.10, f.w * 0.34, f.h * 0.48);
    let steps = 16;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let w = base.w * (0.30 + 0.70 * t);
        let y = base.y + base.h * t;
        let x = base.center().x - w * 0.5;
        let c = Color::hex(0x8d8578).lerp(Color::hex(0x6b6458), (i % 2) as f32 * 0.4);
        p.rect(Rect::new(x, y, w, base.h / steps as f32 + 1.0), c, 0.0);
        p.rect(Rect::new(x, y, w, 1.0), Color::hex(0xa39b8d), 0.0);
        // The stair up the middle, in shadow.
        p.rect(Rect::new(base.center().x - w * 0.10, y, w * 0.20, base.h / steps as f32 + 1.0), Color::hex(0x7b7468), 0.0);
    }
    // Temple house on top, with its doorway.
    let house = Rect::new(base.center().x - base.w * 0.15, base.y - base.h * 0.19, base.w * 0.30, base.h * 0.21);
    p.rect(house, Color::hex(0x8d8578), 0.0);
    p.rect(Rect::new(house.x, house.y - house.h * 0.25, house.w, house.h * 0.3), Color::hex(0x77705f), 0.0);
    p.rect(Rect::new(house.center().x - house.w * 0.15, house.y + house.h * 0.32, house.w * 0.3, house.h * 0.68), Color::hex(0x241f18), 0.0);

    // Thatched huts sitting on the grass.
    for (i, x) in [0.05f32, 0.15, 0.26].iter().enumerate() {
        let hh = f.h * (0.075 + i as f32 * 0.006);
        let hw = f.w * 0.075;
        let hy = horizon + f.h * (0.035 + i as f32 * 0.02);
        p.rect(Rect::new(f.x + f.w * x, hy, hw, hh), Color::hex(0x8a6a44), 0.0);
        p.rect(Rect::new(f.x + f.w * x - hw * 0.12, hy - hh * 0.42, hw * 1.24, hh * 0.46), Color::hex(0x5e4a2e), 0.0);
    }

    // Foreground leaves hanging into the corners.
    let leaf = Color::hex(0x1e3d1b);
    for (cx, cy, rw, rh, rot) in [
        (0.00f32, -0.06f32, 0.17f32, 0.15f32, 0.0f32),
        (0.10, -0.10, 0.12, 0.13, 0.0),
        (0.86, -0.08, 0.16, 0.17, 0.0),
        (0.72, -0.12, 0.13, 0.12, 0.0),
        (0.95, 0.10, 0.10, 0.12, 0.0),
    ] {
        let _ = rot;
        let r = Rect::new(f.x + f.w * cx, f.y + f.h * cy, f.w * rw, f.h * rh);
        p.rect(r, leaf, r.h * 0.45);
        p.rect(r.shrink(r.w * 0.2, r.h * 0.25, r.w * 0.2, r.h * 0.25), Color::hex(0x2c5a26), r.h * 0.3);
    }
}

/// Timecode left, fit/quality dropdowns right — the strip under the picture.
fn readout(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(26.0))
        .padding(Insets::xy(10.0, 0.0))
        .gap(8.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame::none(), |ui| {
        ui.text_with(&timecode(ed.playhead), 12.0, REEL.timecode);
        ui.space(6.0);
        ui.combo("fit", &mut ed.fit, &["Fit", "100%", "50%", "25%"]);
        ui.flex();
        ui.combo("quality", &mut ed.quality, &["Full", "1/2", "1/4"]);
        let _ = icon_button(ui, "settings", Icon::Wrench, 20.0, false);
        ui.text_with(&timecode(ed.content_end()), 12.0, t.palette.text_muted);
    });
}

/// The scrub bar: click or drag anywhere on it to move the playhead.
fn scrub(ui: &mut Ui, ed: &mut Editor) {
    let id = ui.make_id("scrub");
    let r = ui.interact_drag(id);
    let end = ed.content_end();
    if r.active || r.pressed {
        let t = ((r.mouse_pos.x - r.rect.x) / r.rect.w.max(1.0)).clamp(0.0, 1.0);
        ed.playhead = t * end;
    }
    if r.hovered {
        ui.cursor = Cursor::ResizeHorizontal;
    }
    let frac = (ed.playhead / end).clamp(0.0, 1.0);
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(22.0)), Vec2::ZERO, true, move |p, rect| {
        let bar = Rect::new(rect.x + 6.0, rect.y + 8.0, rect.w - 12.0, 5.0);
        p.rect(bar, Color::hex(0x3a3a3a), 2.5);
        // Ticks every tenth, like the monitor's own scale.
        for i in 0..=10 {
            let x = bar.x + bar.w * (i as f32 / 10.0);
            p.rect(Rect::new(x, bar.bottom() + 2.0, 1.0, 4.0), Color::hex(0x4a4a4a), 0.0);
        }
        let x = bar.x + bar.w * frac;
        p.rect(Rect::new(bar.x, bar.y, (x - bar.x).max(0.0), bar.h), Color::hex(0x4a4a4a), 2.5);
        p.rect(Rect::new(x - 5.0, rect.y + 3.0, 10.0, 14.0), REEL.playhead, 2.0);
    });
}

fn transport(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(34.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(2.0)
        .align(Align::Center, Align::Center);
    ui.container(row, Frame { fill: Color::hex(0x1f1f1f), ..Frame::none() }, |ui| {
        let _ = icon_button(ui, "add-marker", Icon::Marker, 22.0, false);
        let _ = icon_button(ui, "in", Icon::MarkIn, 22.0, false);
        let _ = icon_button(ui, "out", Icon::MarkOut, 22.0, false);
        divider(ui, "t1", true, 18.0);
        let _ = icon_button(ui, "start", Icon::JumpStart, 22.0, false);
        let _ = icon_button(ui, "back", Icon::StepBack, 22.0, false);
        let play = icon_button(ui, "play", if ed.playing { Icon::Pause } else { Icon::Play }, 26.0, ed.playing);
        if play.clicked {
            ed.playing = !ed.playing;
        }
        let _ = icon_button(ui, "fwd", Icon::StepForward, 22.0, false);
        let _ = icon_button(ui, "end", Icon::JumpEnd, 22.0, false);
        divider(ui, "t2", true, 18.0);
        let _ = icon_button(ui, "lift", Icon::Insert, 22.0, false);
        let _ = icon_button(ui, "extract", Icon::Overwrite, 22.0, false);
        let _ = icon_button(ui, "snapshot", Icon::Camera, 22.0, false);
        let _ = icon_button(ui, "compare", Icon::Panel, 22.0, false);
        let _ = icon_button(ui, "captions", Icon::Captions, 22.0, false);
        ui.flex();
        let _ = icon_button(ui, "plus", Icon::AddTrack, 22.0, false);
        let _ = t;
    });
}
