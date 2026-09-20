//! What a `Ui::cached` recording is only valid under.
//!
//! A recording holds *finished* instances: positions in window pixels, glyph
//! uvs in atlas texels, colours already resolved from the theme. None of that
//! is in the app's `deps`, and each of it can change without the app touching
//! the subtree at all — the window moves to a 2x display, a theme file is
//! reloaded, a canvas zooms, the glyph atlas fills up and is repacked.
//!
//! Every test drives a cached `Ui` and a reference `Ui` that never caches with
//! the same events, and compares them on instance bytes: if a replay is not
//! what the build would have produced, they disagree.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// Enough text to be worth caching, and a widget with retained state.
fn body(ui: &mut Ui, rows: usize) {
    ui.heading("Outliner");
    for i in 0..rows {
        ui.with_key(i, |ui| {
            let _ = ui.selectable(&format!("Object {i}"), false);
        });
    }
}

/// `live` stands in for the meter that forces a frame every time, so the
/// cached subtree is the only thing that could be skipped.
fn frame(ui: &mut Ui, cache: bool, rows: usize, live: u32, scale: f32) -> Vec<u8> {
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale, dt: 1.0 / 60.0 });
    ui.label(&format!("live {live}"));
    if cache {
        ui.cached("panel", rows, |ui| body(ui, rows));
    } else {
        ui.container(Layout::column().width(Size::Fit).height(Size::Fit), Frame::none(), |ui| body(ui, rows));
    }
    let out = ui.end_frame();
    bytemuck::cast_slice(&out.draw.instances).to_vec()
}

struct Pair {
    cached: Ui,
    plain: Ui,
    rows: usize,
    live: u32,
}

impl Pair {
    fn new(rows: usize) -> Self {
        Self { cached: ui(), plain: ui(), rows, live: 0 }
    }

    /// One frame on both. Returns whether they agree.
    fn step(&mut self, scale: f32) -> bool {
        self.live += 1;
        let a = frame(&mut self.cached, true, self.rows, self.live, scale);
        let b = frame(&mut self.plain, false, self.rows, self.live, scale);
        a == b
    }

    fn settle(&mut self, scale: f32, n: usize, what: &str) {
        for f in 0..n {
            assert!(self.step(scale), "{what}: the two disagreed on frame {f} before the change");
        }
    }

    fn expect_same(&mut self, scale: f32, n: usize, what: &str) {
        for f in 0..n {
            assert!(self.step(scale), "{what}: replay differs from a fresh build, frame {f}");
        }
    }
}

/// Moving to a display with a different DPI re-snaps every glyph.
#[test]
fn a_dpi_change_is_not_replayed() {
    let mut p = Pair::new(20);
    p.settle(1.0, 4, "dpi");
    p.expect_same(2.0, 4, "dpi");
}

/// The palette is the app's to edit in place — a hot-reloaded theme file, a
/// colour picker — and the theme's name does not change when it does.
#[test]
fn a_palette_edited_in_place_is_not_replayed() {
    let mut p = Pair::new(20);
    p.settle(1.0, 4, "palette");
    for ui in [&mut p.cached, &mut p.plain] {
        ui.theme.palette.text = Color::hex(0xff3366);
        ui.theme.selectable.text = Color::hex(0xff3366);
    }
    p.expect_same(1.0, 4, "palette");
}

/// A canvas learns its own origin on its second frame, so the transform a
/// subtree was recorded under is not the one it is replayed under. (Its
/// recording is in window coordinates; the rect it is placed at is in the
/// canvas's.)
#[test]
fn a_canvas_transform_change_is_not_replayed() {
    let mut c = ui();
    let mut plain = ui();
    let frame = |ui: &mut Ui, cache: bool, st: &mut CanvasState, live: u32| -> Vec<u8> {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.label(&format!("live {live}"));
        ui.canvas("c", st, |ui, _| {
            if cache {
                ui.cached("panel", 12usize, |ui| body(ui, 12));
            } else {
                ui.container(Layout::column().width(Size::Fit).height(Size::Fit), Frame::none(), |ui| body(ui, 12));
            }
        });
        let out = ui.end_frame();
        bytemuck::cast_slice(&out.draw.instances).to_vec()
    };
    let mut sc = CanvasState { zoom: 1.0, ..CanvasState::default() };
    let mut sp = sc;
    // Frame 1 is where the canvas first knows where it is.
    for f in 0..4 {
        assert_eq!(frame(&mut c, true, &mut sc, f), frame(&mut plain, false, &mut sp, f), "canvas: frame {f}");
    }
    // And zooming rescales a recording rather than moving it.
    sc.zoom = 2.0;
    sp.zoom = 2.0;
    for f in 4..8 {
        assert_eq!(frame(&mut c, true, &mut sc, f), frame(&mut plain, false, &mut sp, f), "canvas zoom: frame {f}");
    }
    // Panning, likewise.
    sc.pan = Vec2::new(37.0, -12.0);
    sp.pan = sc.pan;
    for f in 8..12 {
        assert_eq!(frame(&mut c, true, &mut sc, f), frame(&mut plain, false, &mut sp, f), "canvas pan: frame {f}");
    }
}

/// A subtree that moves under a resting pointer has a different widget under
/// that pointer, so its hover cannot be replayed. `cached` cannot see the move
/// coming — this frame's rect is not known until layout — so it is told by a
/// layout epoch instead.
#[test]
fn a_subtree_scrolling_under_the_pointer_is_not_replayed() {
    let mut c = ui();
    let mut plain = ui();
    let frame = |ui: &mut Ui, cache: bool, live: u32| -> Vec<u8> {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.label(&format!("live {live}"));
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(150.0)), |ui| {
            if cache {
                ui.cached("panel", 25usize, |ui| body(ui, 25));
            } else {
                ui.container(Layout::column().width(Size::Fit).height(Size::Fit), Frame::none(), |ui| body(ui, 25));
            }
        });
        let out = ui.end_frame();
        bytemuck::cast_slice(&out.draw.instances).to_vec()
    };
    for f in 0..4 {
        assert_eq!(frame(&mut c, true, f), frame(&mut plain, false, f), "scroll: frame {f}");
    }
    // Rest the pointer on a row and let its hover finish fading in: with
    // nothing left animating, only the epoch can tell that the row under the
    // pointer is about to change.
    for ui in [&mut c, &mut plain] {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 80.0) });
    }
    for f in 0..20 {
        assert_eq!(frame(&mut c, true, f), frame(&mut plain, false, f), "hover settling: frame {f}");
    }
    // Now the content scrolls out from under it.
    for ui in [&mut c, &mut plain] {
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -120.0), unit: WheelUnit::Pixel });
    }
    for f in 4..10 {
        assert_eq!(frame(&mut c, true, f), frame(&mut plain, false, f), "scroll: frame {f}, hover is stale");
    }
    // And once it has settled and the pointer has left.
    for ui in [&mut c, &mut plain] {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(480.0, 390.0) });
    }
    for f in 10..14 {
        assert_eq!(frame(&mut c, true, f), frame(&mut plain, false, f), "scroll: frame {f}, pointer outside");
    }
}

/// Repacking the atlas moves every glyph, so recordings made before it hold
/// uvs into texels that now belong to something else. The atlas only repacks
/// between frames (see `Fonts::repack`), so the check is that a repack is
/// followed by a rebuild rather than a replay.
#[test]
fn an_atlas_repack_forces_a_rebuild() {
    let mut ui = ui();
    // Many sizes of real glyphs: what a zoomed canvas asks of the atlas.
    let text = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let frame = |ui: &mut Ui, flood: bool, live: u32| {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.label(&format!("live {live}"));
        if flood {
            for size in (10..200).step_by(2) {
                ui.text_with(text, size as f32, Color::WHITE);
            }
        }
        ui.cached("panel", 20usize, |ui| body(ui, 20));
        let out = ui.end_frame();
        (out.atlas.repacks, out.platform.repaint_after)
    };
    for f in 0..4 {
        frame(&mut ui, false, f);
    }
    assert_eq!(ui.profile().cached_hits, 1, "the subtree should be replaying by now");

    // Overflow the atlas. The frame that hits a full atlas repaints itself
    // rather than repacking mid-frame, which would have moved the glyphs of
    // everything already drawn.
    let (before, repaint) = frame(&mut ui, true, 4);
    assert_eq!(repaint, Some(0.0), "a frame that outgrew the atlas did not ask for another");
    let (after, _) = frame(&mut ui, true, 5);
    assert!(after > before, "the atlas never repacked: {before} -> {after}");
    assert_eq!(ui.profile().cached_hits, 0, "a recording from before the repack was replayed");
    assert_eq!(ui.profile().cached_misses, 1);
}
