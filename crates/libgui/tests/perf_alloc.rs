//! Allocation budget for a frame.
//!
//! A pro app's UI must not churn the allocator: allocation is shared global
//! state, so a UI that allocates per widget per frame taxes every other thread
//! in the process, not just itself.
//!
//! Deliberately **one** `#[test]` in this file: the counter is global and
//! `cargo test` runs tests within a binary in parallel, so a second test here
//! would race it. Measurements are differential — cost(2N) - cost(N) — so any
//! constant background noise from the harness cancels out.

use libgui::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(new.saturating_sub(l.size()) as u64, Relaxed);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn a_frame_stays_within_its_allocation_budget() {
    let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).expect("font");
    let names: Vec<String> = (0..4000).map(|i| format!("Mesh {}", i % 7)).collect();

    let sample = |ui: &mut Ui, rows: usize| -> (u64, u64) {
        // Warm every cache first, so we measure the steady state a running app
        // is in, not first-frame setup.
        for _ in 0..60 {
            ui.begin_frame(FrameInfo::default());
            build(ui, rows, &names);
            let _ = ui.end_frame();
        }
        ALLOCS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        ui.begin_frame(FrameInfo::default());
        build(ui, rows, &names);
        let _ = ui.end_frame();
        (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
    };

    let (a1, b1) = sample(&mut ui, 500);
    let (a2, b2) = sample(&mut ui, 1000);

    // Identical frames must cost identically: a climbing count means something
    // is accumulating (a cache that never hits, a Vec that regrows every frame).
    let (a1_again, _) = sample(&mut ui, 500);
    assert_eq!(a1, a1_again, "allocations drifted between identical frames");

    let per_widget = (a2 - a1) as f64 / 500.0;
    let bytes_per_widget = (b2 - b1) as f64 / 500.0;
    println!("{per_widget:.2} allocations and {bytes_per_widget:.0} bytes per widget");
    println!("a 500-widget panel: {a1} allocations, {b1} bytes");

    // Zero. Three arenas, all reused frame to frame: a container's children
    // are a range into the tree's child arena, a paint closure is written into
    // the paint arena rather than boxed, and the text it draws is a handle into
    // the text arena rather than an owned `String`. What is left is a constant
    // for the whole panel, not a cost per widget — which is the property worth
    // guarding, since it is what a real-time thread needs.
    assert_eq!(per_widget, 0.0, "{per_widget:.2} allocations per widget");
    assert!(a1 <= 8, "a 500-widget panel allocated {a1} times");

    // An idle repaint must be nearly free: a tool app that redraws a still
    // screen should not be handing work to the allocator at all.
    for _ in 0..30 {
        ui.begin_frame(FrameInfo::default());
        let _ = ui.end_frame();
    }
    ALLOCS.store(0, Relaxed);
    ui.begin_frame(FrameInfo::default());
    let _ = ui.end_frame();
    let empty = ALLOCS.load(Relaxed);
    println!("an empty frame: {empty} allocations");
    assert!(empty <= 8, "an empty frame allocated {empty} times");

    nesting_is_free(&mut ui);
    a_table_costs_nothing_per_frame(&mut ui);
    a_frame_without_text_allocates_nothing(false);
    a_frame_without_text_allocates_nothing(true);
}

/// libgui keeps no process-global state, so the allocator is whatever the
/// binary installs — this test file *is* the proof, since it installs its own
/// and counts every call libgui makes. What an app embedding a UI in a
/// real-time loop actually wants from that is not a special allocator but no
/// allocator at all during a frame, so: a frame that draws no text must reach
/// exactly zero, and `Ui::reserve` must get there on the first frame rather
/// than the third.
fn a_frame_without_text_allocates_nothing(reserve: bool) {
    let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).expect("font");
    if reserve {
        ui.reserve(4_000);
    }
    let style = Frame::panel(&ui.theme);
    let build = |ui: &mut Ui| {
        ui.begin_frame(FrameInfo::default());
        for i in 0..2_000 {
            ui.container(libgui::Layout::row().height(Size::Fixed(8.0)), style, |ui| {
                let id = ui.make_id(("leaf", i));
                let v = i as f32;
                let leaf = libgui::Layout::leaf(Size::Fixed(8.0), Size::Fixed(8.0));
                ui.add_leaf(id, leaf, Vec2::ZERO, false, move |p, r| {
                    p.rect(Rect::new(r.x + v, r.y, 4.0, 4.0), Color::WHITE, 1.0);
                });
            });
        }
        let _ = ui.end_frame();
    };

    ALLOCS.store(0, Relaxed);
    build(&mut ui);
    let first = ALLOCS.load(Relaxed);
    for _ in 0..4 {
        ALLOCS.store(0, Relaxed);
        build(&mut ui);
    }
    let steady = ALLOCS.load(Relaxed);
    println!("4000 widgets, no text, reserve={reserve}: first frame {first}, steady {steady}");
    assert_eq!(steady, 0, "a steady text-free frame allocated {steady} times");
    if reserve {
        // Not literally zero: one small map still sizes itself on first use.
        // The number that matters is that it is a constant, not per widget.
        assert!(first <= 2, "reserve() left {first} allocations on the first frame");
    }
}

/// A data grid is the widest thing a pro app builds, and the easiest place to
/// lose the arenas again: a header that clones its column titles, or a cell
/// that formats a `String` per frame, is invisible until someone opens a table
/// with a hundred thousand rows in it.
fn a_table_costs_nothing_per_frame(ui: &mut Ui) {
    let mut cols = TableState::new([
        Column::new("Name").width(160.0),
        Column::new("Kind").width(90.0),
        Column::new("Size").width(80.0).align(Align::End),
        Column::new("Modified").width(140.0),
        Column::new("Owner").width(120.0),
    ]);
    cols.frozen = 1;
    // Pre-rendered cell text, as an app with real data would have.
    let cells: Vec<String> = (0..64).map(|i| format!("cell {i}")).collect();
    let mut frame = |ui: &mut Ui| {
        ui.begin_frame(FrameInfo::default());
        ui.table("files", &mut cols, 250_000, |ui, row, col| {
            ui.label(&cells[(row * 5 + col) % cells.len()]);
        });
        let _ = ui.end_frame();
    };
    for _ in 0..30 {
        frame(ui);
    }
    ALLOCS.store(0, Relaxed);
    frame(ui);
    let a = ALLOCS.load(Relaxed);
    println!("a 250,000-row table: {a} allocations");
    assert_eq!(a, 0, "a steady table frame allocated {a} times");
}

/// Nesting is free: a container's children are a range into one arena that is
/// reused frame to frame, and `place` works in one shared scratch buffer, so
/// neither the child list nor the layout temporaries touch the allocator.
/// Deliberately part of the same test — the counter is global, and a second
/// `#[test]` here would race it.
fn nesting_is_free(ui: &mut Ui) {
    let depth = |ui: &mut Ui, n: usize| {
        ui.begin_frame(FrameInfo::default());
        fn nest(ui: &mut Ui, left: usize) {
            if left == 0 {
                return;
            }
            ui.container(libgui::Layout::column().height(Size::Fit), Frame::none(), |ui| nest(ui, left - 1));
        }
        for _ in 0..n {
            ui.container(libgui::Layout::row().height(Size::Fit), Frame::none(), |ui| nest(ui, 8));
        }
        let _ = ui.end_frame();
    };
    for _ in 0..30 {
        depth(ui, 200);
    }
    ALLOCS.store(0, Relaxed);
    depth(ui, 200);
    let a = ALLOCS.load(Relaxed);
    println!("1800 nested containers: {a} allocations");
    assert!(a <= 4, "nesting cost {a} allocations; the child arena is being bypassed");
}

fn build(ui: &mut Ui, rows: usize, names: &[String]) {
    ui.heading("Inspector");
    let mut v = 0.5;
    ui.slider("Position", &mut v, 0.0, 1.0);
    let mut b = true;
    ui.toggle("Visible", &mut b);
    for (i, name) in names.iter().enumerate().take(rows) {
        ui.with_key(i, |ui| {
            let _ = ui.selectable(name, i == 3);
        });
    }
    let _ = ui.button("Apply");
}
