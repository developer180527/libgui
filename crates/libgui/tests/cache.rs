//! Subtree caching: a replay must be indistinguishable from the build it
//! replaced, or it is a bug that only shows up as pixels being wrong.
//!
//! So every test here compares against a *reference* `Ui` that never caches
//! and is driven with the same events, frame for frame. If the two ever
//! disagree on a single instance byte, the cache is lying.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// The subtree under test: enough text to be worth caching, plus a widget with
/// its own retained state so pruning bugs show up.
fn body(ui: &mut Ui, names: &[String], selected: usize) {
    ui.heading("Outliner");
    for (i, n) in names.iter().enumerate() {
        ui.with_key(i, |ui| {
            let _ = ui.selectable(n, i == selected);
        });
    }
}

/// Build the same UI with and without the cache. `live` stands in for the
/// meter that forces a frame every time.
fn frame(ui: &mut Ui, names: &[String], selected: usize, live: u32, cache: bool) -> Vec<u8> {
    ui.begin_frame(info());
    ui.label(&format!("live {live}"));
    if cache {
        ui.cached("panel", (names.len(), selected), |ui| body(ui, names, selected));
    } else {
        ui.container(Layout::column().width(Size::Fit).height(Size::Fit), Frame::none(), |ui| {
            body(ui, names, selected)
        });
    }
    let out = ui.end_frame();
    bytemuck::cast_slice(&out.draw.instances).to_vec()
}

struct Pair {
    cached: Ui,
    plain: Ui,
    names: Vec<String>,
}

impl Pair {
    fn new(rows: usize) -> Self {
        Self {
            cached: ui(),
            plain: ui(),
            names: (0..rows).map(|i| format!("Object {i}")).collect(),
        }
    }

    /// One frame on both, asserting they agree. Returns the cache's profile.
    fn step(&mut self, selected: usize, live: u32) -> Profile {
        let a = frame(&mut self.cached, &self.names, selected, live, true);
        let b = frame(&mut self.plain, &self.names, selected, live, false);
        assert_eq!(a, b, "the cached UI drew something different from the uncached one");
        self.cached.profile()
    }

    fn push_both(&mut self, e: InputEvent) {
        self.cached.push(e.clone());
        self.plain.push(e);
    }
}

#[test]
fn a_replay_draws_exactly_what_the_build_would_have() {
    let mut p = Pair::new(30);
    for f in 0..10 {
        p.step(3, f);
    }
    let last = p.cached.profile();
    assert_eq!(last.cached_hits, 1, "the subtree never replayed: {last}");
}

#[test]
fn changing_the_deps_rebuilds() {
    let mut p = Pair::new(30);
    for f in 0..6 {
        p.step(3, f);
    }
    assert_eq!(p.step(3, 99).cached_hits, 1, "expected a hit while deps were unchanged");
    // A different selection is a different dependency, so it must rebuild —
    // and the comparison inside `step` is what proves the pixels followed.
    let miss = p.step(7, 100);
    assert_eq!(miss.cached_hits, 0, "a changed dep replayed stale pixels");
    assert_eq!(miss.cached_misses, 1);

    // It keeps rebuilding while the selection fade runs, because a replay
    // would freeze it half way, and starts caching again once it settles.
    let mut settled = None;
    for f in 101..160 {
        if p.step(7, f).cached_hits == 1 {
            settled = Some(f - 100);
            break;
        }
    }
    let settled = settled.expect("the cache never resumed after a dep change");
    assert!(settled > 1, "it replayed while the selection fade was still running");
    assert!(settled < 40, "the fade took {settled} frames to settle");
}

#[test]
fn the_pointer_arriving_rebuilds_so_hover_still_works() {
    let mut p = Pair::new(30);
    for f in 0..6 {
        p.step(3, f);
    }
    assert_eq!(p.step(3, 10).cached_hits, 1);

    // Into the middle of the list.
    p.push_both(InputEvent::PointerMoved { pos: Vec2::new(60.0, 120.0) });
    for f in 11..24 {
        // Every one of these compares against the uncached build, so a frozen
        // hover would fail here rather than merely look wrong.
        let pr = p.step(3, f);
        assert_eq!(pr.cached_hits, 0, "the cache replayed while the pointer was inside it");
    }

    // Pointer away again: the fade has to finish before it may replay.
    p.push_both(InputEvent::PointerMoved { pos: Vec2::new(480.0, 380.0) });
    let mut hit = false;
    for f in 24..80 {
        hit |= p.step(3, f).cached_hits == 1;
    }
    assert!(hit, "the cache never recovered after the pointer left");
}

#[test]
fn a_replayed_subtree_keeps_its_widgets_hit_testable() {
    let mut p = Pair::new(30);
    for f in 0..8 {
        p.step(3, f);
    }
    assert_eq!(p.cached.profile().cached_hits, 1);

    // The rows were never built this frame, but their hit rects were replayed,
    // so the pointer still finds one — which is what invalidates the cache and
    // lets the next frame respond.
    p.push_both(InputEvent::PointerMoved { pos: Vec2::new(60.0, 120.0) });
    let pr = p.step(3, 9);
    assert_eq!(pr.cached_hits, 0);
    assert!(p.cached.wants_pointer(), "a replayed subtree stopped claiming the pointer");
}

#[test]
fn a_replay_costs_far_less_than_the_build_it_replaced() {
    let mut p = Pair::new(60);
    for f in 0..6 {
        p.step(3, f);
    }
    let hit = p.step(3, 10);
    assert_eq!(hit.cached_hits, 1);
    let miss = p.step(99, 11); // different deps → rebuild
    assert_eq!(miss.cached_hits, 0);
    // Same pixels either way, which `step` already asserted.
    assert_eq!(hit.instances, miss.instances, "a replay emitted a different number of instances");
    // The replay does no text work at all: that is the whole point.
    assert_eq!(hit.text_draws, 1, "a replay re-shaped text: {hit}");
    assert!(miss.text_draws > 50, "the rebuild should have drawn every label: {miss}");
}

#[test]
fn a_subtree_the_app_stops_building_is_forgotten() {
    let mut p = Pair::new(20);
    for f in 0..6 {
        p.step(3, f);
    }
    // Sixty-odd frames without it, then bring it back: it must rebuild rather
    // than replay a recording made against a different world.
    for _ in 0..70 {
        p.cached.begin_frame(info());
        p.cached.label("elsewhere");
        let _ = p.cached.end_frame();
    }
    let back = p.step(3, 200);
    assert_eq!(back.cached_hits, 0, "a swept recording was replayed anyway");
    p.step(3, 201);
    assert_eq!(p.step(3, 202).cached_hits, 1, "it never started caching again");
}

#[test]
fn a_theme_change_throws_the_cache_away() {
    let mut p = Pair::new(20);
    for f in 0..6 {
        p.step(3, f);
    }
    assert_eq!(p.step(3, 10).cached_hits, 1);
    p.cached.theme = Theme::light();
    p.plain.theme = Theme::light();
    let after = p.step(3, 11);
    assert_eq!(after.cached_hits, 0, "the cache replayed pixels drawn in the old theme");
}
