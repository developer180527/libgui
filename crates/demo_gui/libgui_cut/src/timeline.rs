//! The timeline: the widget no UI library ships.
//!
//! It is one interactive surface rather than a tree of widgets, because
//! everything in it is decided by the same two numbers — pixels per second and
//! the scroll offset — and a tree would spend its life keeping a ruler, a
//! header column and a thousand clips agreeing about them. So this takes a
//! single rect from libgui, hit-tests inside it, and paints.
//!
//! What it has to get right:
//!
//! - **Scroll in two directions.** Horizontally the ruler and the clips move
//!   together while the track headers stay; vertically the headers and the
//!   clips move together while the ruler stays.
//! - **Zoom about the pointer**, so Cmd-wheel keeps the frame under the cursor
//!   under the cursor.
//! - **Drag**: clips between tracks and along time, the playhead on the ruler,
//!   and both scrollbars.
//! - **Snapping** to clip edges and the playhead, which is what makes cutting
//!   feel solid.

use libgui::*;

use crate::state::{timecode, Drag, Editor, Media, TrackKind, BAR, HEADER_W, RULER_H};
use crate::theme::REEL;
use crate::widgets::{divider, icon_button, Icon};

const TOOLS: [(Icon, &str); 9] = [
    (Icon::Select, "Selection"),
    (Icon::TrackSelect, "Track Select Forward"),
    (Icon::Ripple, "Ripple Edit"),
    (Icon::Rolling, "Rolling Edit"),
    (Icon::Razor, "Razor"),
    (Icon::Slip, "Slip"),
    (Icon::Pen, "Pen"),
    (Icon::Rect, "Rectangle"),
    (Icon::Hand, "Hand"),
];

pub fn panel(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
    ui.container(col, Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() }, |ui| {
        toolbar(ui, ed);
        ui.container(Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame::none(), |ui| {
            tools(ui, ed);
            surface(ui, ed);
            meters(ui, ed);
        });
    });
}

/// Timecode and the sequence-wide toggles.
fn toolbar(ui: &mut Ui, ed: &mut Editor) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(30.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    ui.container(row, Frame { fill: Color::hex(0x1f1f1f), ..Frame::none() }, |ui| {
        ui.text_with(&timecode(ed.playhead), 12.0, REEL.timecode);
        ui.space(10.0);
        let _ = icon_button(ui, "seq-settings", Icon::Settings, 22.0, false);
        let snap = icon_button(ui, "snap", Icon::Snap, 22.0, ed.snap);
        if snap.clicked {
            ed.snap = !ed.snap;
        }
        let link = icon_button(ui, "link", Icon::LinkedSelection, 22.0, ed.linked);
        if link.clicked {
            ed.linked = !ed.linked;
        }
        let mk = icon_button(ui, "markers", Icon::Marker, 22.0, ed.markers);
        if mk.clicked {
            ed.markers = !ed.markers;
        }
        divider(ui, "tb", true, 18.0);
        let _ = icon_button(ui, "tl-wrench", Icon::Wrench, 22.0, false);
        let _ = icon_button(ui, "tl-cc", Icon::Captions, 22.0, false);
        ui.flex();
        let _ = t;
    });
}

/// The tool palette down the left edge.
fn tools(ui: &mut Ui, ed: &mut Editor) {
    let col = Layout::column()
        .width(Size::Fixed(30.0))
        .height(Size::Grow(1.0))
        .padding(Insets::xy(3.0, 6.0))
        .gap(2.0)
        .align(Align::Center, Align::Start);
    ui.container(col, Frame { fill: Color::hex(0x1c1c1c), ..Frame::none() }, |ui| {
        for (i, (icon, name)) in TOOLS.iter().enumerate() {
            let r = icon_button(ui, ("tool", i), *icon, 24.0, ed.tool == i);
            ui.tooltip(&r, name);
            if r.clicked {
                ed.tool = i;
            }
        }
    });
}

/// Everything that scrolls: headers, ruler, tracks, clips, scrollbars.
fn surface(ui: &mut Ui, ed: &mut Editor) {
    let id = ui.make_id("timeline");
    let r = ui.interact_drag(id);
    let rect = r.rect;
    ed.timeline_rect = rect;

    // Geometry for this frame. `rect` is last frame's on the first pass, which
    // is fine: it only decides how far things may scroll.
    let view_w = (rect.w - HEADER_W - BAR).max(1.0);
    let view_h = (rect.h - RULER_H - BAR).max(1.0);
    let end = ed.content_end() + 4.0;
    let content_w = end * ed.pps;
    let content_h = ed.tracks_height();
    let max_x = (content_w - view_w).max(0.0);
    let max_y = (content_h - view_h).max(0.0);

    let track_x = rect.x + HEADER_W;
    let tracks_y = rect.y + RULER_H;
    // A plain function rather than a closure over `ed`: the drag arms below
    // need `ed` mutably while they ask it where the pointer is in time.
    let time_at = |x: f32, scroll_x: f32, pps: f32| ((x - track_x + scroll_x) / pps).max(0.0);

    // --- input ---------------------------------------------------------
    let inside = r.hovered || r.active;
    if inside {
        let s = ui.input().scroll;
        let m = ui.input().modifiers;
        if m.ctrl || m.logo {
            // Zoom about the pointer: the frame under the cursor stays there.
            let anchor = time_at(r.mouse_pos.x, ed.scroll_x, ed.pps);
            let factor = (1.0 + s.y * 0.0015).clamp(0.5, 2.0);
            ed.pps = (ed.pps * factor).clamp(6.0, 900.0);
            ed.scroll_x = (anchor * ed.pps - (r.mouse_pos.x - track_x)).clamp(0.0, (end * ed.pps - view_w).max(0.0));
        } else if m.shift {
            ed.scroll_x = (ed.scroll_x - s.y - s.x).clamp(0.0, max_x);
        } else {
            ed.scroll_y = (ed.scroll_y - s.y).clamp(0.0, max_y);
            ed.scroll_x = (ed.scroll_x - s.x).clamp(0.0, max_x);
        }
    }

    let on_ruler = r.mouse_pos.y < tracks_y && r.mouse_pos.x >= track_x;
    let on_hbar = r.mouse_pos.y >= rect.bottom() - BAR;
    let on_vbar = r.mouse_pos.x >= rect.right() - BAR && !on_hbar;

    if r.pressed {
        ed.drag = if on_hbar && max_x > 0.0 {
            Some(Drag::ScrollX)
        } else if on_vbar && max_y > 0.0 {
            Some(Drag::ScrollY)
        } else if on_ruler {
            ed.playhead = time_at(r.mouse_pos.x, ed.scroll_x, ed.pps);
            Some(Drag::Playhead)
        } else if r.mouse_pos.x >= track_x {
            // A clip, or the empty track under the pointer.
            let y = r.mouse_pos.y - tracks_y + ed.scroll_y;
            let t = time_at(r.mouse_pos.x, ed.scroll_x, ed.pps);
            match track_at(ed, y).and_then(|tr| ed.clip_at(tr, t)) {
                Some(i) => {
                    ed.selected_clip = Some(i);
                    Some(Drag::Clip { index: i, grab: t - ed.seq.clips[i].start })
                }
                None => {
                    ed.selected_clip = None;
                    None
                }
            }
        } else {
            // The header column: target the track that was clicked.
            let y = r.mouse_pos.y - tracks_y + ed.scroll_y;
            if let Some(tr) = track_at(ed, y) {
                header_click(ed, tr, r.mouse_pos.x - rect.x);
            }
            None
        };
    }

    if r.active {
        match ed.drag {
            Some(Drag::Playhead) => ed.playhead = time_at(r.mouse_pos.x, ed.scroll_x, ed.pps),
            Some(Drag::ScrollX) => {
                let span = (view_w / content_w.max(1.0) * view_w).max(24.0);
                let travel = (view_w - span).max(1.0);
                ed.scroll_x = (ed.scroll_x + r.drag_delta.x * max_x / travel).clamp(0.0, max_x);
            }
            Some(Drag::ScrollY) => {
                let span = (view_h / content_h.max(1.0) * view_h).max(24.0);
                let travel = (view_h - span).max(1.0);
                ed.scroll_y = (ed.scroll_y + r.drag_delta.y * max_y / travel).clamp(0.0, max_y);
            }
            Some(Drag::Clip { index, grab }) => {
                let want = (time_at(r.mouse_pos.x, ed.scroll_x, ed.pps) - grab).max(0.0);
                let len = ed.seq.clips[index].len;
                let start = if ed.snap { snap(ed, index, want, len) } else { want };
                ed.seq.clips[index].start = start;
                // Dragging across tracks, onto a track of the same kind.
                let y = r.mouse_pos.y - tracks_y + ed.scroll_y;
                if let Some(tr) = track_at(ed, y) {
                    let media = ed.seq.clips[index].media;
                    let kind = ed.seq.tracks[tr].kind;
                    let ok = matches!(
                        (media, kind),
                        (Media::Audio, TrackKind::Audio) | (Media::Video, TrackKind::Video) | (Media::Title, TrackKind::Video)
                    );
                    if ok {
                        ed.seq.clips[index].track = tr;
                    }
                }
            }
            None => {}
        }
    }
    if !r.active && ed.drag.is_some() {
        ed.drag = None;
    }
    if r.hovered {
        ui.cursor = if on_ruler { Cursor::ResizeHorizontal } else { Cursor::Default };
    }

    // Keep the playhead in view while it plays, as a timeline does.
    if ed.playing {
        let x = ed.playhead * ed.pps - ed.scroll_x;
        if x > view_w * 0.9 || x < 0.0 {
            ed.scroll_x = (ed.playhead * ed.pps - view_w * 0.1).clamp(0.0, max_x);
        }
    }

    // --- paint ---------------------------------------------------------
    let shot = Shot::new(ui, ed, view_w, view_h, content_w, content_h);
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| shot.paint(p, rect));
}

/// Which track a y inside the stack belongs to.
fn track_at(ed: &Editor, y: f32) -> Option<usize> {
    let mut top = 0.0;
    for (i, t) in ed.seq.tracks.iter().enumerate() {
        if y >= top && y < top + t.height + 1.0 {
            return Some(i);
        }
        top += t.height + 1.0;
    }
    None
}

/// The header column's buttons: lock, eye/mute, solo, target.
fn header_click(ed: &mut Editor, track: usize, x: f32) {
    let video = ed.seq.tracks[track].kind == TrackKind::Video;
    let t = &mut ed.seq.tracks[track];
    match x {
        x if x < 26.0 => t.locked = !t.locked,
        x if x < 56.0 => t.targeted = !t.targeted,
        x if x < 84.0 => {
            if video {
                t.visible = !t.visible
            } else {
                t.muted = !t.muted
            }
        }
        x if x < 110.0 && !video => t.solo = !t.solo,
        _ => {}
    }
}

/// Nearest edge worth snapping to, within a few pixels.
fn snap(ed: &Editor, index: usize, want: f32, len: f32) -> f32 {
    let tol = 7.0 / ed.pps;
    let mut best = want;
    let mut dist = tol;
    let mut consider = |edge: f32, at: f32| {
        let d = (at - edge).abs();
        if d < dist {
            dist = d;
            best = want + (edge - at);
        }
    };
    for (i, c) in ed.seq.clips.iter().enumerate() {
        if i == index {
            continue;
        }
        for edge in [c.start, c.end()] {
            consider(edge, want);
            consider(edge, want + len);
        }
    }
    consider(ed.playhead, want);
    consider(ed.playhead, want + len);
    consider(0.0, want);
    best.max(0.0)
}

/// Everything the paint closure needs, copied out of the app before it runs.
struct Shot {
    tracks: Vec<TrackShot>,
    clips: Vec<ClipShot>,
    ticks: Vec<(f32, FrameText)>,
    pps: f32,
    scroll_x: f32,
    scroll_y: f32,
    playhead: f32,
    content_w: f32,
    content_h: f32,
    view_w: f32,
    view_h: f32,
    selected: Option<usize>,
    text: Color,
    faint: Color,
    accent: Color,
    size: f32,
}

struct TrackShot {
    name: FrameText,
    top: f32,
    height: f32,
    video: bool,
    locked: bool,
    on: bool,
    solo: bool,
    targeted: bool,
}

struct ClipShot {
    name: FrameText,
    track: usize,
    start: f32,
    len: f32,
    media: Media,
    fx: bool,
    tint: Color,
}

impl Shot {
    fn new(ui: &mut Ui, ed: &Editor, view_w: f32, view_h: f32, content_w: f32, content_h: f32) -> Self {
        let t = ui.theme.clone();
        // Ruler labels: a step that keeps them readable at any zoom.
        let step = tick_step(ed.pps);
        let first = (ed.scroll_x / ed.pps / step).floor() * step;
        let mut ticks = Vec::new();
        let mut time = first;
        while time * ed.pps - ed.scroll_x < view_w + 80.0 {
            ticks.push((time, ui.frame_text(&timecode(time))));
            time += step;
        }
        let tracks = ed
            .seq
            .tracks
            .iter()
            .enumerate()
            .map(|(i, tr)| TrackShot {
                name: ui.frame_text(tr.name),
                top: ed.track_top(i),
                height: tr.height,
                video: tr.kind == TrackKind::Video,
                locked: tr.locked,
                on: if tr.kind == TrackKind::Video { tr.visible } else { !tr.muted },
                solo: tr.solo,
                targeted: tr.targeted,
            })
            .collect();
        let clips = ed
            .seq
            .clips
            .iter()
            .map(|c| ClipShot {
                name: ui.frame_text(&c.name),
                track: c.track,
                start: c.start,
                len: c.len,
                media: c.media,
                fx: c.fx,
                tint: c.tint,
            })
            .collect();
        Self {
            tracks,
            clips,
            ticks,
            pps: ed.pps,
            scroll_x: ed.scroll_x,
            scroll_y: ed.scroll_y,
            playhead: ed.playhead,
            content_w,
            content_h,
            view_w,
            view_h,
            selected: ed.selected_clip,
            text: t.palette.text,
            faint: t.palette.text_faint,
            accent: t.palette.accent,
            size: t.metrics.font_size,
        }
    }

    fn paint(&self, p: &mut Painter, rect: Rect) {
        let track_x = rect.x + HEADER_W;
        let tracks_y = rect.y + RULER_H;
        let view = Rect::new(track_x, tracks_y, self.view_w, self.view_h);
        p.rect(rect, REEL.track_bg, 0.0);

        self.ruler(p, rect, track_x);
        self.tracks(p, rect, view, track_x, tracks_y);
        self.headers(p, rect, tracks_y);
        self.playhead(p, rect, track_x, tracks_y);
        self.scrollbars(p, rect);
    }

    fn ruler(&self, p: &mut Painter, rect: Rect, track_x: f32) {
        let r = Rect::new(rect.x, rect.y, rect.w, RULER_H);
        p.rect(r, REEL.ruler_bg, 0.0);
        p.rect(Rect::new(rect.x, r.bottom() - 1.0, rect.w, 1.0), Color::hex(0x101010), 0.0);
        if let Some(clip) = p.draw.clip().intersect(&Rect::new(track_x, r.y, self.view_w, r.h)) {
            p.draw.push_clip(clip);
            // The work area bar along the top, as the reference has.
            p.rect(Rect::new(track_x - self.scroll_x, r.y, self.content_w, 3.0), Color::hex(0xd8c552).with_alpha(0.85), 0.0);
            for (time, label) in &self.ticks {
                let x = (track_x + time * self.pps - self.scroll_x).round();
                p.rect(Rect::new(x, r.y + 6.0, 1.0, RULER_H - 7.0), Color::hex(0x4d4d4d), 0.0);
                p.text_left(Rect::new(x + 4.0, r.y + 5.0, 90.0, 14.0), self.size - 1.0, self.faint, *label);
            }
            p.draw.pop_clip();
        }
    }

    fn tracks(&self, p: &mut Painter, rect: Rect, view: Rect, track_x: f32, tracks_y: f32) {
        let Some(clip) = p.draw.clip().intersect(&view) else { return };
        p.draw.push_clip(clip);
        for (i, tr) in self.tracks.iter().enumerate() {
            let y = tracks_y + tr.top - self.scroll_y;
            let bg = if i % 2 == 0 { REEL.track_bg } else { REEL.track_bg_alt };
            p.rect(Rect::new(track_x, y, self.view_w, tr.height), bg, 0.0);
            p.rect(Rect::new(track_x, y + tr.height, self.view_w, 1.0), Color::hex(0x141414), 0.0);
        }
        // Clips on top of the lanes.
        for (i, c) in self.clips.iter().enumerate() {
            let Some(tr) = self.tracks.get(c.track) else { continue };
            let x = track_x + c.start * self.pps - self.scroll_x;
            let w = c.len * self.pps;
            if x + w < view.x || x > view.right() {
                continue;
            }
            let y = tracks_y + tr.top - self.scroll_y;
            self.clip(p, Rect::new(x, y, w, tr.height - 1.0), c, self.selected == Some(i));
        }
        p.draw.pop_clip();
        let _ = rect;
    }

    fn clip(&self, p: &mut Painter, r: Rect, c: &ClipShot, selected: bool) {
        let (fill, head) = match c.media {
            Media::Video => (REEL.video_fill, REEL.video_head),
            Media::Audio => (REEL.audio_fill, REEL.audio_head),
            Media::Title => (REEL.title_fill, REEL.title_head),
        };
        p.rect(r, fill, 3.0);
        // Name strip along the top.
        let strip = Rect::new(r.x, r.y, r.w, 15.0_f32.min(r.h));
        p.rect(strip, head, 3.0);
        p.rect(Rect::new(r.x, strip.bottom() - 3.0, r.w, 3.0), head, 0.0);
        if r.w > 26.0 {
            p.text_left(strip.shrink(5.0, 0.0, 18.0, 0.0), self.size - 1.0, REEL.clip_text, c.name);
        }
        if c.fx && r.w > 40.0 {
            // The fx badge an effected clip wears.
            let b = Rect::new(r.right() - 16.0, r.y + 2.0, 13.0, 11.0);
            p.rect(b, Color::rgba(0.0, 0.0, 0.0, 0.35), 2.0);
            crate::widgets::draw_icon(p, b, Icon::Effects, Color::hex(0xc9e2f5));
        }
        let body = Rect::new(r.x, strip.bottom(), r.w, (r.bottom() - strip.bottom()).max(0.0));
        if body.h > 4.0 {
            match c.media {
                Media::Audio => waveform(p, body, c.len),
                Media::Title => {
                    p.rect(body.shrink(4.0, 3.0, 4.0, 3.0), Color::WHITE.with_alpha(0.10), 2.0);
                }
                Media::Video => filmstrip(p, body, c.tint, c.len),
            }
        }
        let border = if selected { REEL.selected } else { REEL.clip_border };
        p.rect_bordered(r, Color::TRANSPARENT, 3.0, if selected { 2.0 } else { 1.0 }, border);
    }

    /// The header column: name, and the row of per-track buttons.
    fn headers(&self, p: &mut Painter, rect: Rect, tracks_y: f32) {
        let col = Rect::new(rect.x, tracks_y, HEADER_W, self.view_h);
        let Some(clip) = p.draw.clip().intersect(&col) else { return };
        p.draw.push_clip(clip);
        p.rect(col, Color::hex(0x1a1a1a), 0.0);
        for tr in &self.tracks {
            let y = tracks_y + tr.top - self.scroll_y;
            let r = Rect::new(col.x, y, HEADER_W, tr.height);
            p.rect(r, REEL.track_head, 0.0);
            p.rect(Rect::new(r.x, r.bottom(), r.w, 1.0), Color::hex(0x141414), 0.0);
            let mid = r.y + 9.0;
            let icon = |p: &mut Painter, x: f32, icon: Icon, on: bool| {
                let b = Rect::new(r.x + x, mid, 16.0, 16.0);
                if on {
                    p.rect(b, Color::hex(0x3d3d3d), 2.0);
                }
                crate::widgets::draw_icon(p, b.shrink(3.0, 3.0, 3.0, 3.0), icon, if on { Color::hex(0xe8e8e8) } else { Color::hex(0x7a7a7a) });
            };
            icon(p, 5.0, Icon::Lock, tr.locked);
            // The source patch: V1 / A1, highlighted when targeted.
            let patch = Rect::new(r.x + 28.0, mid, 18.0, 16.0);
            p.rect(patch, if tr.targeted { self.accent } else { Color::hex(0x2f2f2f) }, 2.0);
            p.text_centered(patch, self.size - 1.0, if tr.targeted { Color::WHITE } else { self.faint }, tr.name);
            p.text_left(Rect::new(r.x + 52.0, mid, 40.0, 16.0), self.size, self.text, tr.name);
            if tr.video {
                icon(p, 86.0, Icon::Eye, tr.on);
            } else {
                icon(p, 86.0, Icon::Speaker, tr.on);
                icon(p, 106.0, Icon::Mic, tr.solo);
            }
        }
        p.draw.pop_clip();
    }

    fn playhead(&self, p: &mut Painter, rect: Rect, track_x: f32, tracks_y: f32) {
        let x = (track_x + self.playhead * self.pps - self.scroll_x).round();
        if x < track_x || x > track_x + self.view_w {
            return;
        }
        let area = Rect::new(track_x, rect.y, self.view_w, rect.h - BAR);
        let Some(clip) = p.draw.clip().intersect(&area) else { return };
        p.draw.push_clip(clip);
        p.rect(Rect::new(x - 0.5, rect.y, 1.0, area.h), REEL.playhead, 0.0);
        // The head itself, sitting in the ruler.
        p.rect(Rect::new(x - 6.0, rect.y, 12.0, 11.0), REEL.playhead, 2.0);
        p.draw.pop_clip();
        let _ = tracks_y;
    }

    fn scrollbars(&self, p: &mut Painter, rect: Rect) {
        // Horizontal, under the tracks.
        let track = Rect::new(rect.x + HEADER_W, rect.bottom() - BAR, self.view_w, BAR);
        p.rect(Rect::new(rect.x, track.y, rect.w, BAR), Color::hex(0x1a1a1a), 0.0);
        if self.content_w > self.view_w {
            let span = (self.view_w / self.content_w * self.view_w).max(28.0);
            let travel = (self.view_w - span).max(1.0);
            let t = (self.scroll_x / (self.content_w - self.view_w).max(1.0)).clamp(0.0, 1.0);
            let thumb = Rect::new(track.x + travel * t, track.y + 2.0, span, BAR - 4.0);
            p.rect(thumb, Color::hex(0x4d4d4d), (BAR - 4.0) * 0.5);
        }
        // Vertical, right of the tracks.
        let vt = Rect::new(rect.right() - BAR, rect.y + RULER_H, BAR, self.view_h);
        p.rect(vt, Color::hex(0x1a1a1a), 0.0);
        if self.content_h > self.view_h {
            let span = (self.view_h / self.content_h * self.view_h).max(28.0);
            let travel = (self.view_h - span).max(1.0);
            let t = (self.scroll_y / (self.content_h - self.view_h).max(1.0)).clamp(0.0, 1.0);
            let thumb = Rect::new(vt.x + 2.0, vt.y + travel * t, BAR - 4.0, span);
            p.rect(thumb, Color::hex(0x4d4d4d), (BAR - 4.0) * 0.5);
        }
    }
}

/// A tick every 1, 2, 5, 10… seconds, whichever keeps labels apart at this zoom.
fn tick_step(pps: f32) -> f32 {
    for step in [1.0 / 24.0, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 300.0] {
        if step * pps >= 76.0 {
            return step;
        }
    }
    600.0
}

/// Bars standing in for a decoded audio waveform.
fn waveform(p: &mut Painter, r: Rect, seed: f32) {
    let mid = r.center().y;
    p.rect(Rect::new(r.x, mid - 0.5, r.w, 1.0), REEL.wave.with_alpha(0.35), 0.0);
    let step = 3.0;
    let mut x = r.x;
    let mut k = seed * 10.0;
    while x < r.right() {
        k += 1.0;
        let a = (k * 0.7).sin() * 0.5 + (k * 0.23).cos() * 0.5;
        let h = (a.abs() * 0.85 + 0.08) * r.h;
        p.rect(Rect::new(x, mid - h * 0.5, 2.0, h), REEL.wave.with_alpha(0.75), 0.0);
        x += step;
    }
}

/// Thumbnails along a video clip: the first frame, then repeats, as an NLE
/// draws when a clip is tall enough.
fn filmstrip(p: &mut Painter, r: Rect, tint: Color, seed: f32) {
    let w = r.h * 1.6;
    if w < 6.0 {
        return;
    }
    let mut x = r.x;
    let mut i = 0;
    while x < r.right() {
        let cell = Rect::new(x, r.y, w.min(r.right() - x), r.h);
        let sky = tint.lerp(Color::hex(0xbcd4e8), 0.55);
        let ground = tint.lerp(Color::hex(0x1e3a22), 0.35);
        p.rect(Rect::new(cell.x, cell.y, cell.w, cell.h * 0.6), sky, 0.0);
        p.rect(Rect::new(cell.x, cell.y + cell.h * 0.6, cell.w, cell.h * 0.4), ground, 0.0);
        if cell.w > 8.0 {
            let m = cell.center();
            p.rect(Rect::new(m.x - cell.w * 0.12, cell.y + cell.h * 0.3, cell.w * 0.24, cell.h * 0.32), tint.lerp(Color::BLACK, 0.25), 0.0);
        }
        p.rect(Rect::new(cell.right() - 1.0, cell.y, 1.0, cell.h), Color::rgba(0.0, 0.0, 0.0, 0.25), 0.0);
        x += w;
        i += 1;
        let _ = (i, seed);
    }
}

/// The audio meters down the right edge: two channels, a dB scale, and the
/// solo buttons under them.
fn meters(ui: &mut Ui, ed: &Editor) {
    let levels = ed.meters;
    let id = ui.make_id("meters");
    let size = ui.theme.metrics.font_size_small;
    let marks = [0, -6, -12, -18, -24, -30, -36, -42, -48];
    let labels: Vec<FrameText> = marks.iter().map(|d| ui.frame_text(&d.to_string())).collect();
    let db = ui.frame_text("dB");
    let ss = ui.frame_text("S");
    ui.add_leaf(id, Layout::leaf(Size::Fixed(62.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
        p.rect(r, Color::hex(0x1c1c1c), 0.0);
        p.rect(Rect::new(r.x, r.y, 1.0, r.h), Color::hex(0x101010), 0.0);
        let top = r.y + 8.0;
        let bottom = r.bottom() - 30.0;
        let h = (bottom - top).max(10.0);
        // Scale down the right-hand side.
        for (i, label) in labels.iter().enumerate() {
            let y = top + h * (i as f32 / (labels.len() - 1) as f32);
            p.text_right(Rect::new(r.x + 24.0, y - 6.0, 34.0, 12.0), size - 1.0, Color::hex(0x8a8a8a), *label);
            p.rect(Rect::new(r.x + 20.0, y, 3.0, 1.0), Color::hex(0x3d3d3d), 0.0);
        }
        p.text_right(Rect::new(r.x + 24.0, bottom + 4.0, 34.0, 12.0), size - 1.0, Color::hex(0x8a8a8a), db);
        // Two channels.
        for (i, level) in levels.iter().enumerate() {
            let bar = Rect::new(r.x + 5.0 + i as f32 * 8.0, top, 6.0, h);
            p.rect(bar, Color::hex(0x0d0d0d), 1.0);
            let lit_h = h * level;
            let lit = Rect::new(bar.x, bar.bottom() - lit_h, bar.w, lit_h);
            p.rect(lit, REEL.meter_lo, 1.0);
            // Yellow near the top, red at the very top, as a meter warns.
            let warn = h * 0.25;
            if lit_h > h - warn {
                let hy = Rect::new(bar.x, bar.y + warn * 0.4, bar.w, lit_h - (h - warn));
                p.rect(hy, Color::hex(0xd8c552), 1.0);
            }
            if lit_h > h * 0.94 {
                p.rect(Rect::new(bar.x, bar.y, bar.w, lit_h - h * 0.94), REEL.meter_hi, 1.0);
            }
        }
        // The solo buttons an NLE puts under its meters.
        for i in 0..2 {
            let b = Rect::new(r.x + 5.0 + i as f32 * 8.0, r.bottom() - 16.0, 6.0, 10.0);
            p.rect(b, Color::hex(0x2a2a2a), 1.0);
            p.text_centered(b, size - 2.0, Color::hex(0x8a8a8a), ss);
        }
    });
}
