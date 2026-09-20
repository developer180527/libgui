//! Effect Controls: the property tree on the left, and the clip's own little
//! timeline on the right, the two scrolling as one.

use libgui::*;

use crate::state::Editor;
use crate::theme::REEL;
use crate::widgets::{icon_button, prop_row, value, Icon, Prop};

pub fn panel(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container(col, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        chips(ui, ed);
        // The tree and the clip's timeline sit side by side under one header.
        ui.container(Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame::none(), |ui| {
            ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame::none(), |ui| {
                tree(ui, ed);
            });
            clip_timeline(ui, ed);
        });
        footer(ui);
    });
}

/// "Source • Tikal.mp4" and "Sequence 01 • Tikal.mp4": which clip the panel is
/// showing, and from where.
fn chips(ui: &mut Ui, ed: &Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(30.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(6.0)
        .align(Align::Start, Align::Center);
    let name = ed
        .selected_clip
        .and_then(|i| ed.seq.clips.get(i))
        .map(|c| c.name.replace(" [V]", "").replace(" [A]", ""))
        .unwrap_or_else(|| "(no clip)".into());
    ui.container(row, Frame { fill: Color::hex(0x1f1f1f), ..Frame::none() }, |ui| {
        chip(ui, "src", Icon::Panel, &format!("Source • {name}"), false);
        chip(ui, "seq", Icon::Effects, &format!("Sequence 01 • {name}"), true);
        ui.flex();
        let _ = icon_button(ui, "split", Icon::Panel, 18.0, false);
        let _ = t;
    });
}

fn chip(ui: &mut Ui, key: &str, icon: Icon, label: &str, on: bool) {
    let t = ui.theme.clone();
    let id = ui.make_id(("chip", key));
    let r = ui.interact(id);
    if r.hovered {
        ui.cursor = Cursor::Pointer;
    }
    let hot = ui.animate_bool(id, 0, r.hovered);
    let size = t.metrics.font_size;
    let w = ui.fonts.measure(ui.font, size, label).x + 34.0;
    let text = ui.frame_text(label);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Fixed(22.0)), Vec2::ZERO, true, move |p, rect| {
        let fill = if on { Color::hex(0x2f2f2f) } else { Color::hex(0x272727) };
        p.rect_bordered(rect, fill.lerp(Color::hex(0x3a3a3a), hot), 3.0, 1.0, Color::hex(0x161616));
        let ic = Rect::new(rect.x + 4.0, rect.center().y - 7.0, 14.0, 14.0);
        crate::widgets::draw_icon(p, ic, icon, if on { t.palette.accent } else { t.palette.text_faint });
        p.text_left(rect.shrink(22.0, 0.0, 6.0, 0.0), size, if on { t.palette.text } else { t.palette.text_muted }, text);
    });
}

/// The property tree. Every row is the same shape: twirl, stopwatch, label,
/// value(s), reset.
fn tree(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let opts = ScrollOptions { gap: 0.0, ..ScrollOptions::new(Size::Grow(1.0)) };
    ui.scroll_area_with("fx", opts, |ui| {
        section(ui, "Video", true);
        let mut motion = ed.motion_open;
        group(ui, "motion", "Motion", &mut motion);
        ed.motion_open = motion;
        if ed.motion_open {
            let (mut x, mut y) = ed.position;
            prop_row(ui, "pos", Prop::new("Position"), |ui| {
                value(ui, "px", &mut x, 0.5, "");
                value(ui, "py", &mut y, 0.5, "");
            });
            ed.position = (x, y);

            let mut scale = ed.scale;
            prop_row(ui, "scale", Prop::new("Scale").twirl(&mut false.clone()), |ui| {
                value(ui, "s", &mut scale, 0.4, "");
            });
            ed.scale = scale;

            let mut sw = ed.scale_width;
            let uniform = ed.uniform_scale;
            let mut sub = false;
            prop_row(ui, "scalew", Prop::new("Scale Width").twirl(&mut sub).dim(uniform), |ui| {
                if uniform {
                    let c = t.palette.text_faint;
                    ui.text_with(&format!("{sw:.1}"), t.metrics.font_size, c);
                } else {
                    value(ui, "sw", &mut sw, 0.4, "");
                }
            });
            ed.scale_width = sw;

            let mut uni = ed.uniform_scale;
            prop_row(ui, "uniform", Prop::new("").plain(), |ui| {
                ui.checkbox("Uniform Scale", &mut uni);
                ui.flex();
            });
            ed.uniform_scale = uni;

            let mut rot = ed.rotation;
            prop_row(ui, "rot", Prop::new("Rotation").twirl(&mut false.clone()), |ui| {
                value(ui, "r", &mut rot, 0.5, "");
            });
            ed.rotation = rot;

            let (mut ax, mut ay) = ed.anchor;
            prop_row(ui, "anchor", Prop::new("Anchor Point"), |ui| {
                value(ui, "ax", &mut ax, 0.5, "");
                value(ui, "ay", &mut ay, 0.5, "");
            });
            ed.anchor = (ax, ay);

            let mut af = ed.anti_flicker;
            prop_row(ui, "af", Prop::new("Anti-flicker Filter").twirl(&mut false.clone()), |ui| {
                value(ui, "af", &mut af, 0.01, "");
            });
            ed.anti_flicker = af;

            for (i, name) in ["Crop Left", "Crop Top", "Crop Right", "Crop Bottom"].iter().enumerate() {
                let mut v = ed.crop[i];
                prop_row(ui, ("crop", i), Prop::new(name).twirl(&mut false.clone()), |ui| {
                    value(ui, ("cropv", i), &mut v, 0.2, "%");
                });
                ed.crop[i] = v.clamp(0.0, 100.0);
            }
        }
        let mut opacity = ed.opacity_open;
        group(ui, "opacity", "Opacity", &mut opacity);
        ed.opacity_open = opacity;
        let mut remap = false;
        group(ui, "remap", "Time Remapping", &mut remap);
        let mut twirl = ed.twirl_open;
        group(ui, "twirl", "Twirl", &mut twirl);
        ed.twirl_open = twirl;
        if ed.twirl_open {
            let mut angle = 0.0;
            prop_row(ui, "twirl-angle", Prop::new("Angle").twirl(&mut false.clone()), |ui| {
                value(ui, "ta", &mut angle, 0.5, "");
            });
            let mut radius = 32.0;
            prop_row(ui, "twirl-radius", Prop::new("Twirl Radius").twirl(&mut false.clone()), |ui| {
                value(ui, "tr", &mut radius, 0.5, "");
            });
        }
    });
}

/// A section header ("Video"), the darker strip with a twirl on the right.
fn section(ui: &mut Ui, label: &str, open: bool) {
    let t = ui.theme.clone();
    let id = ui.make_id(("section", label));
    let text = ui.frame_text(label);
    let size = t.metrics.font_size;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(20.0)), Vec2::ZERO, false, move |p, r| {
        p.rect(r, Color::hex(0x272727), 0.0);
        p.text_left(r.shrink(8.0, 0.0, 0.0, 0.0), size, t.palette.text, text);
        let ic = Rect::new(r.right() - 20.0, r.center().y - 6.0, 12.0, 12.0);
        crate::widgets::draw_icon(p, ic, if open { Icon::Chevron } else { Icon::ChevronRight }, t.palette.text_muted);
    });
}

/// An effect's header row: twirl, the `fx` badge, the name, a reset.
fn group(ui: &mut Ui, key: &str, label: &str, open: &mut bool) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(20.0))
        .padding(Insets::xy(4.0, 0.0))
        .gap(4.0)
        .align(Align::Start, Align::Center);
    let id = ui.make_id(("group", key));
    ui.container_id(id, row, Frame { fill: Color::hex(0x202020), ..Frame::none() }, |ui| {
        let r = icon_button(ui, (key, "tw"), if *open { Icon::Chevron } else { Icon::ChevronRight }, 12.0, false);
        if r.clicked {
            *open = !*open;
        }
        let fx_id = ui.make_id(("fx", key));
        ui.add_leaf(fx_id, Layout::leaf(Size::Fixed(14.0), Size::Fixed(14.0)), Vec2::ZERO, false, move |p, rect| {
            crate::widgets::draw_icon(p, rect, Icon::Effects, Color::hex(0x8f8f8f));
        });
        ui.text_with(label, t.metrics.font_size, t.palette.text);
        ui.flex();
        let _ = icon_button(ui, (key, "rst"), Icon::Reset, 14.0, false);
    });
}

/// The clip's own timeline, right of the tree: a ruler, the clip's bar, and a
/// playhead at the sequence time. Dragging anywhere in it scrubs.
fn clip_timeline(ui: &mut Ui, ed: &mut Editor) {
    let id = ui.make_id("fx-timeline");
    let r = ui.interact_drag(id);
    let clip = ed.selected_clip.and_then(|i| ed.seq.clips.get(i));
    let (start, len) = clip.map(|c| (c.start, c.len)).unwrap_or((0.0, 1.0));
    // Show the clip and a little air either side.
    let span = (len * 1.6).max(1.0);
    let origin = (start - len * 0.3).max(0.0);
    if r.active || r.pressed {
        let f = ((r.mouse_pos.x - r.rect.x) / r.rect.w.max(1.0)).clamp(0.0, 1.0);
        ed.playhead = origin + f * span;
    }
    if r.hovered {
        ui.cursor = Cursor::ResizeHorizontal;
    }
    let head = ((ed.playhead - origin) / span).clamp(0.0, 1.0);
    let clip_x = ((start - origin) / span).clamp(0.0, 1.0);
    let clip_w = (len / span).clamp(0.0, 1.0);
    let name = clip.map(|c| c.name.clone()).unwrap_or_default();
    let name = ui.frame_text(&name);
    // Four labels, spaced so they cannot collide: a `Painter` cannot measure
    // and re-layout, so the count is decided here.
    let ticks: Vec<FrameText> = (0..4).map(|i| ui.frame_text(&crate::state::timecode(origin + span * i as f32 / 4.0))).collect();
    let size = ui.theme.metrics.font_size_small;
    let fill = REEL.video_fill;
    let head_c = REEL.video_head;
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::new(200.0, 60.0), true, move |p, rect| {
        p.rect(rect, Color::hex(0x1d1d1d), 0.0);
        let ruler = Rect::new(rect.x, rect.y, rect.w, 18.0);
        p.rect(ruler, Color::hex(0x252525), 0.0);
        p.rect(Rect::new(rect.x, ruler.bottom(), rect.w, 1.0), Color::hex(0x101010), 0.0);
        for (i, label) in ticks.iter().enumerate() {
            let x = (rect.x + rect.w * (i as f32 / 4.0)).round();
            p.rect(Rect::new(x, ruler.y + 5.0, 1.0, 13.0), Color::hex(0x484848), 0.0);
            p.text_left(Rect::new(x + 3.0, ruler.y + 2.0, 72.0, 14.0), size - 1.0, Color::hex(0x7a7a7a), *label);
        }
        // The clip's bar, under the ruler.
        let bar = Rect::new(rect.x + rect.w * clip_x, ruler.bottom() + 4.0, (rect.w * clip_w).max(4.0), 20.0);
        p.rect(bar, fill, 2.0);
        p.rect(Rect::new(bar.x, bar.y, bar.w, 8.0), head_c, 2.0);
        if bar.w > 30.0 {
            p.text_left(bar.shrink(4.0, 8.0, 4.0, 0.0), size - 1.0, Color::hex(0xe8f1f8), name);
        }
        // Keyframe lanes, empty but ruled, as the panel shows them.
        let mut y = bar.bottom() + 6.0;
        while y < rect.bottom() - 14.0 {
            p.rect(Rect::new(rect.x + 4.0, y, rect.w - 8.0, 1.0), Color::hex(0x262626), 0.0);
            y += 20.0;
        }
        // A scrollbar along the bottom, as the reference has.
        let track = Rect::new(rect.x + 4.0, rect.bottom() - 10.0, rect.w - 8.0, 7.0);
        p.rect(track, Color::hex(0x161616), 3.5);
        p.rect(Rect::new(track.x + track.w * clip_x, track.y, (track.w * clip_w).max(20.0), track.h), Color::hex(0x4a4a4a), 3.5);
        // Playhead.
        let x = (rect.x + rect.w * head).round();
        p.rect(Rect::new(x - 0.5, rect.y, 1.0, rect.h - 12.0), REEL.playhead, 0.0);
        p.rect(Rect::new(x - 5.0, rect.y, 10.0, 9.0), REEL.playhead, 1.5);
    });
}

/// The panel's own bottom strip.
fn footer(ui: &mut Ui) {
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(26.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(4.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame { fill: Color::hex(0x1f1f1f), ..Frame::none() }, |ui| {
        ui.text_with(&crate::state::timecode(0.29), 11.0, REEL.timecode);
        ui.flex();
        let _ = icon_button(ui, "filter", Icon::Sort, 20.0, false);
        let _ = icon_button(ui, "audio", Icon::Speaker, 20.0, false);
        let _ = icon_button(ui, "export-fx", Icon::Export, 20.0, false);
    });
}
