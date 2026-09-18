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

    // Today: 2 per text widget (the owned label and its boxed paint closure).
    // The ceiling catches a third being introduced, not normal variation.
    assert!(per_widget <= 2.5, "{per_widget:.2} allocations per widget");
    assert!(bytes_per_widget <= 256.0, "{bytes_per_widget:.0} bytes per widget");

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
