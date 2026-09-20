//! The timeline, driven the way a user drives it.
//!
//! A timeline that renders correctly and does not *behave* is a picture, so
//! these press, drag and wheel at real coordinates and check the edit changed.

use libgui::*;
use libgui_cut::{App, Editor};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const W: f32 = 1800.0;
const H: f32 = 1000.0;

struct Harness {
    ui: Ui,
    app: App,
    size: Vec2,
}

impl Harness {
    fn new() -> Self {
        Self::sized(W, H)
    }

    /// A window of a given size: the timeline only scrolls vertically when the
    /// tracks are taller than the panel, which a tall window hides.
    fn sized(w: f32, h: f32) -> Self {
        let mut h = Self { ui: Ui::new(libgui_cut::theme(), FONT).expect("font"), app: App::default(), size: Vec2::new(w, h) };
        // Enough frames for the dock to lay out and the timeline to learn its
        // rect, which is what every coordinate below is relative to.
        for _ in 0..4 {
            h.frame();
        }
        h
    }

    fn frame(&mut self) {
        self.ui.begin_frame(FrameInfo { screen_size: self.size, scale: 1.0, dt: 1.0 / 60.0 });
        self.app.ui(&mut self.ui);
        let _ = self.ui.end_frame();
    }

    fn ed(&self) -> &Editor {
        &self.app.ed
    }

    fn move_to(&mut self, p: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: p });
    }

    fn press(&mut self) {
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    }

    fn release(&mut self) {
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    }

    fn wheel(&mut self, delta: Vec2, mods: &[Key]) {
        for &k in mods {
            self.ui.push(InputEvent::Key { key: k, pressed: true, repeat: false });
        }
        self.ui.push(InputEvent::Wheel { delta, unit: WheelUnit::Pixel });
    }

    fn release_keys(&mut self, mods: &[Key]) {
        for &k in mods {
            self.ui.push(InputEvent::Key { key: k, pressed: false, repeat: false });
        }
    }

    /// Press at `from`, move to `to` over a couple of frames, release.
    fn drag(&mut self, from: Vec2, to: Vec2) {
        self.move_to(from);
        self.frame();
        self.press();
        self.frame();
        self.move_to(Vec2::new(from.x + (to.x - from.x) * 0.5, from.y + (to.y - from.y) * 0.5));
        self.frame();
        self.move_to(to);
        self.frame();
        self.release();
        self.frame();
    }
}

#[test]
fn the_timeline_gets_a_rect_and_draws_its_tracks() {
    let h = Harness::new();
    let r = h.ed().timeline_rect;
    assert!(r.w > 400.0 && r.h > 150.0, "the timeline did not get a sensible rect: {r:?}");
    assert_eq!(h.ed().seq.tracks.len(), 5);
}

/// A wheel scrolls the tracks vertically; with Shift, along time.
#[test]
fn the_wheel_scrolls_both_ways() {
    // Short enough that the five tracks do not fit.
    let mut h = Harness::sized(1400.0, 620.0);
    let at = h.ed().point_at(2.0, 1);
    h.move_to(at);
    h.frame();

    h.wheel(Vec2::new(0.0, -120.0), &[]);
    h.frame();
    assert!(h.ed().scroll_y > 0.0, "a plain wheel did not scroll the tracks");
    let after_v = h.ed().scroll_y;
    assert_eq!(h.ed().scroll_x, 0.0, "a plain wheel scrolled sideways too");

    h.wheel(Vec2::new(0.0, -240.0), &[Key::ShiftLeft]);
    h.frame();
    h.release_keys(&[Key::ShiftLeft]);
    h.frame();
    assert!(h.ed().scroll_x > 0.0, "Shift+wheel did not scroll along time");
    assert_eq!(h.ed().scroll_y, after_v, "Shift+wheel moved the tracks vertically");

    // And back up: scrolling clamps at the start rather than going negative.
    h.wheel(Vec2::new(0.0, 4000.0), &[]);
    h.frame();
    assert_eq!(h.ed().scroll_y, 0.0);
}

/// Cmd/Ctrl+wheel zooms, and the frame under the pointer stays under it —
/// which is the whole point of zooming a timeline.
#[test]
fn zoom_keeps_the_frame_under_the_pointer() {
    let mut h = Harness::new();
    let target = 4.0;
    let at = h.ed().point_at(target, 1);
    h.move_to(at);
    h.frame();
    let before = h.ed().pps;

    h.wheel(Vec2::new(0.0, 600.0), &[Key::ControlLeft]);
    h.frame();
    h.release_keys(&[Key::ControlLeft]);
    h.frame();

    let ed = h.ed();
    assert!(ed.pps > before * 1.2, "Ctrl+wheel did not zoom in: {} -> {}", before, ed.pps);
    let now = ed.point_at(target, 1);
    assert!((now.x - at.x).abs() < 1.5, "the frame under the pointer moved {} px while zooming", (now.x - at.x).abs());
    assert!(ed.scroll_x > 0.0, "zooming in at 4s should have scrolled the view");
}

/// Clicking a clip selects it; clicking empty track clears the selection.
#[test]
fn clicking_selects_a_clip() {
    let mut h = Harness::new();
    // The title clip on V2 (track 0), which starts at 9.2s.
    let at = h.ed().point_at(10.0, 0);
    h.move_to(at);
    h.frame();
    h.press();
    h.frame();
    h.release();
    h.frame();
    let selected = h.ed().selected_clip.expect("nothing was selected");
    assert_eq!(h.ed().seq.clips[selected].name, "HIKING");

    // Empty space on the same track, before that clip.
    let empty = h.ed().point_at(1.0, 0);
    h.move_to(empty);
    h.frame();
    h.press();
    h.frame();
    h.release();
    h.frame();
    assert_eq!(h.ed().selected_clip, None, "clicking empty track kept the selection");
}

/// Dragging a clip moves it in time, and onto another video track.
#[test]
fn dragging_a_clip_moves_it_in_time_and_between_tracks() {
    let mut h = Harness::new();
    // Grab the title clip on V2 and drag it later, and down onto V1.
    let grab = h.ed().point_at(10.0, 0);
    let start_before = h.ed().seq.clips[4].start;
    let to = Vec2::new(grab.x + 2.0 * h.ed().pps, h.ed().point_at(10.0, 1).y);
    h.drag(grab, to);

    let clip = &h.ed().seq.clips[4];
    assert_eq!(clip.name, "HIKING");
    assert!(clip.start > start_before + 1.0, "the clip did not move along time: {} -> {}", start_before, clip.start);
    assert_eq!(clip.track, 1, "the clip did not move to the track under the pointer");

    // An audio clip may not be dropped on a video track.
    let music = h.ed().seq.clips[8].track;
    let grab = h.ed().point_at(4.0, music);
    let to = Vec2::new(grab.x + 40.0, h.ed().point_at(4.0, 1).y);
    h.drag(grab, to);
    // It may land on another *audio* track — the drag passes over A1 on its
    // way — but never on a video one.
    let landed = h.ed().seq.clips[8].track;
    assert_eq!(
        h.ed().seq.tracks[landed].kind,
        libgui_cut::TrackKind::Audio,
        "audio was dropped onto a video track (from {music} to {landed})"
    );
}

/// With snapping on, a clip dragged near another's edge lands exactly on it.
#[test]
fn a_dragged_clip_snaps_to_an_edge() {
    let mut h = Harness::new();
    // V2's title clip, dragged so its head lands a few px past V1's cut at 6.4s.
    let edge = h.ed().seq.clips[0].end();
    let grab = h.ed().point_at(10.0, 0);
    let head_offset = 10.0 - h.ed().seq.clips[4].start;
    let want = edge + head_offset + 4.0 / h.ed().pps;
    let to = Vec2::new(h.ed().point_at(want, 0).x, grab.y);
    h.drag(grab, to);

    let start = h.ed().seq.clips[4].start;
    assert!((start - edge).abs() < 0.001, "expected a snap to {edge}, landed at {start}");
}

/// Dragging on the ruler scrubs, and the playhead follows the pointer.
#[test]
fn dragging_the_ruler_scrubs() {
    let mut h = Harness::new();
    let at = h.ed().ruler_point(5.0);
    h.move_to(at);
    h.frame();
    h.press();
    h.frame();
    assert!((h.ed().playhead - 5.0).abs() < 0.05, "the playhead did not jump to the press: {}", h.ed().playhead);

    h.move_to(h.ed().ruler_point(9.0));
    h.frame();
    assert!((h.ed().playhead - 9.0).abs() < 0.05, "the playhead did not follow the drag: {}", h.ed().playhead);
    h.release();
    h.frame();

    // Scrubbing leaves the selection alone: it is not an edit.
    assert_eq!(h.ed().selected_clip, Some(0));
    assert_eq!(h.ed().seq.clips[0].start, 0.0, "scrubbing moved a clip");
}

/// The scrollbar under the tracks drags the view.
#[test]
fn the_scrollbar_scrolls() {
    let mut h = Harness::new();
    let r = h.ed().timeline_rect;
    let bar_y = r.bottom() - libgui_cut::BAR * 0.5;
    let from = Vec2::new(r.x + libgui_cut::HEADER_W + 40.0, bar_y);
    h.drag(from, Vec2::new(from.x + 160.0, bar_y));
    assert!(h.ed().scroll_x > 0.0, "dragging the scrollbar did not scroll");
}

/// Playback advances the playhead and keeps it in view.
#[test]
fn playing_advances_and_follows() {
    let mut h = Harness::new();
    h.app.ed.playing = true;
    h.app.ed.playhead = 0.0;
    for _ in 0..40 {
        h.app.ed.tick(1.0 / 30.0);
        h.frame();
    }
    let ed = h.ed();
    assert!(ed.playhead > 1.0, "playback did not advance: {}", ed.playhead);
    let x = ed.playhead * ed.pps - ed.scroll_x;
    assert!(x >= 0.0 && x < ed.timeline_rect.w, "the playhead ran off screen at {x}");
}
