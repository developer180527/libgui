//! Deliberately unreasonable UIs.
//!
//! The perf guards assert that the ordinary case stays cheap. These assert
//! that the *unreasonable* case stays correct: thousands of live controls, a
//! tree nested deeper than any real layout, a thousand distinct font sizes,
//! tens of thousands of strings that are each seen once. Every one of those is
//! something a real app eventually does by accident, and the failure mode is
//! never a wrong pixel — it is a stack overflow, an atlas thrashing itself, or
//! a cache that grows until the machine swaps.

use libgui::testing::{steady_frame, Budget};
use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(1400.0, 900.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// A row of everything: the mix a real inspector or mixer actually has.
fn busy_row(ui: &mut Ui, i: usize, state: &mut State) {
    let n = state.names.len();
    let (name, selected) = (state.names[i % n].clone(), i == state.selected);
    ui.row(|ui| {
        let _ = ui.selectable(&name, selected);
        ui.checkbox("On", &mut state.flags[i % n]);
        ui.slider_keyed(i, "Gain", &mut state.gains[i % n], 0.0, 1.0);
        ui.progress("Load", Some((i % 100) as f32 / 100.0));
        let _ = ui.button_keyed(i, "Solo");
        ui.drag_value_range("Pan", &mut state.pans[i % n], 0.01, -1.0..=1.0);
    });
}

struct State {
    names: Vec<String>,
    flags: Vec<bool>,
    gains: Vec<f32>,
    pans: Vec<f32>,
    selected: usize,
}

impl State {
    fn new(n: usize) -> Self {
        Self {
            names: (0..64).map(|i| format!("Track {i}")).collect(),
            flags: vec![false; 64],
            gains: vec![0.5; 64],
            pans: vec![0.0; 64],
            selected: n / 2,
        }
    }
}

#[test]
fn ten_thousand_live_controls_cost_what_a_screenful_costs() {
    const ROWS: usize = 10_000;
    let mut ui = ui();
    let mut st = State::new(ROWS);
    let cost = steady_frame(&mut ui, info(), |ui| {
        ui.virtual_list("mixer", ROWS, 28.0, |ui, i| busy_row(ui, i, &mut st));
    });

    // Six controls a row, and only the rows on screen: about thirty of them.
    Budget::steady(900).instances(3_000).offscreen_nodes(64).assert(&cost);
    println!("10,000 rows of six controls: {cost:?}");
}

/// The same UI written the way it gets written first. Not a failure — it is
/// what the budget API is *for* — so the test is that the guard fires.
#[test]
fn the_same_ui_unvirtualised_is_caught_rather_than_merely_slow() {
    const ROWS: usize = 2_000;
    let mut ui = ui();
    let mut st = State::new(ROWS);
    let cost = steady_frame(&mut ui, info(), |ui| {
        ui.scroll_area("mixer", |ui| {
            for i in 0..ROWS {
                busy_row(ui, i, &mut st);
            }
        });
    });
    let over = Budget::steady(900).offscreen_nodes(64).check(&cost);
    assert!(over.iter().any(|m| m.starts_with("nodes")), "an unvirtualised 2,000-row mixer was not flagged: {over:?}");
    assert!(over.iter().any(|m| m.starts_with("offscreen_nodes")), "{over:?}");
}

/// `measure`, `place` and `paint` all recurse, once per level each. A layout
/// nested deeper than the stack can take does not render wrongly — it aborts
/// the process, which is the one failure mode a UI library must not have.
/// Measured before this was bounded: a debug build died between 400 and 500
/// levels, and the build itself was fine, so it was the library's recursion.
#[test]
fn a_layout_nested_far_deeper_than_any_real_one_is_dropped_rather_than_fatal() {
    fn nest(ui: &mut Ui, left: usize) {
        if left == 0 {
            ui.label("bottom");
            return;
        }
        ui.container(Layout::column().height(Size::Fit), Frame::none(), |ui| nest(ui, left - 1));
    }
    let mut ui = ui();
    for _ in 0..3 {
        ui.begin_frame(info());
        nest(&mut ui, 900);
        let _ = ui.end_frame();
    }
    let cost = ui.frame_cost();
    // Everything up to the limit is laid out and drawn as normal…
    assert!(cost.nodes > 200, "the whole tree was dropped, not only the part past the limit");
    // …and what went past it is reported rather than silently gone, so a
    // `Budget` can fail a test on it.
    assert!(cost.too_deep > 500, "deep nesting was not counted: {cost:?}");
    println!("900 levels of nesting: {} laid out, {} dropped", cost.nodes, cost.too_deep);
}

/// A thousand distinct font sizes is what a zooming canvas produces. The atlas
/// is finite, so it will fill; what must not happen is it thrashing every
/// frame once it has.
#[test]
fn many_font_sizes_fill_the_atlas_once_and_then_settle() {
    let mut ui = ui();
    let sizes: Vec<f32> = (0..400).map(|i| 6.0 + i as f32 * 0.25).collect();
    let frame = |ui: &mut Ui| -> u32 {
        ui.begin_frame(info());
        for (i, s) in sizes.iter().enumerate() {
            ui.text_with(&format!("size {i}"), *s, ui.theme.palette.text);
        }
        let _ = ui.end_frame();
        ui.frame_cost().glyphs_rasterized
    };
    let first = frame(&mut ui);
    let mut later = 0;
    for _ in 0..6 {
        later = frame(&mut ui);
    }
    println!("400 font sizes: {first} glyphs rasterised on the first frame, {later} once warm");
    // The atlas grows to hold the set, once, rather than resetting to the
    // same size and re-rasterising all of it every frame. Before it grew,
    // this read 1704 -> 1706: the whole set, forever.
    assert_eq!(later, 0, "the atlas is thrashing: {first} -> {later} glyphs a frame");
    assert!(first > 1_000, "the test did not actually fill the atlas ({first} glyphs)");
}

/// Text that is different every frame — a timecode, a meter readout, a frame
/// counter — must not grow the shaped-run cache without bound.
#[test]
fn text_that_is_never_the_same_twice_does_not_grow_without_bound() {
    let mut ui = ui();
    let mut shaped_total = 0u64;
    for f in 0..400u32 {
        ui.begin_frame(info());
        for i in 0..40 {
            ui.label(&format!("{i}:{f:08}.{:03}", f % 1000));
        }
        let _ = ui.end_frame();
        shaped_total += ui.frame_cost().text_shaped as u64;
    }
    // Every string is new, so every one is shaped: the point is that this
    // stays linear in frames rather than the cache growing until it clears
    // wholesale and re-shapes everything it still needed.
    println!("16,000 one-shot strings: {shaped_total} shaping passes");
    assert!(shaped_total <= 400 * 40 + 400, "re-shaped {shaped_total} times for 16,000 strings");
}

/// Ids are derived by hashing; a huge flat container is where a bad scheme
/// turns quadratic or starts colliding.
#[test]
fn fifty_thousand_widgets_in_one_container_keep_distinct_identities() {
    let mut ui = ui();
    ui.begin_frame(info());
    ui.audit = true;
    for i in 0..50_000 {
        ui.with_key(i, |ui| {
            let _ = ui.selectable("same label every time", false);
        });
    }
    let _ = ui.end_frame();
    let cost = ui.frame_cost();
    assert_eq!(cost.unkeyed_duplicates, 0, "keys did not keep 50,000 identical labels apart");
    assert!(cost.nodes >= 50_000);
}
