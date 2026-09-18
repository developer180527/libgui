//! Where does a libgui frame's CPU time go?
//!
//! The question this exists to answer: is the per-widget `Box<dyn FnOnce>` +
//! `String` allocation actually the thing that costs, or is it text measurement
//! / id hashing / the per-frame `rects` map? Refactoring to an arena is a real
//! trade against the paint-closure API, so it should follow a number.
//!
//!   cargo run --release -p libgui_bench                          # timings
//!   cargo run --release -p libgui_bench --features count-allocs  # allocations
//!
//! Workloads are chosen so that subtracting one from another isolates a cost:
//!   leaf_plain leaf + id, non-capturing closure (0 allocs) -> tree/layout floor
//!   leaf_boxed leaf_plain + String + capturing closure      -> isolates allocation
//!   label      leaf_boxed + measure + glyph emission        -> isolates text
//!   space      built-in spacer (constant id key)
//!   button     label + interact + 2 animate + paint measure
//!   selectable the CAD-outliner row (a long list of these is the real case)
//!   nested     same leaf count, wrapped in rows           -> container overhead
//!   dup_label  every label identical                      -> make_id dedup path

use libgui::*;
use std::time::{Duration, Instant};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

// ---- allocation counting (feature-gated so timings stay clean) -------------

#[cfg(feature = "count-allocs")]
mod counter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);

    pub struct Counting;

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

    pub fn reset() {
        ALLOCS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
    }
    pub fn read() -> (u64, u64) {
        (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
    }
}

#[cfg(feature = "count-allocs")]
#[global_allocator]
static ALLOC: counter::Counting = counter::Counting;

#[cfg(not(feature = "count-allocs"))]
mod counter {
    pub fn reset() {}
    pub fn read() -> (u64, u64) {
        (0, 0)
    }
}

// ---- workloads -------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Work {
    Measure,
    LeafPlain,
    LeafBoxed,
    Space,
    Label,
    Button,
    Selectable,
    Nested,
    DupLabel,
}

impl Work {
    fn name(self) -> &'static str {
        match self {
            Work::Measure => "measure",
            Work::LeafPlain => "leaf_plain",
            Work::LeafBoxed => "leaf_boxed",
            Work::Space => "space",
            Work::Label => "label",
            Work::Button => "button",
            Work::Selectable => "selectable",
            Work::Nested => "nested",
            Work::DupLabel => "dup_label",
        }
    }
    const ALL: [Work; 9] = [
        Work::Measure,
        Work::LeafPlain,
        Work::LeafBoxed,
        Work::Label,
        Work::Button,
        Work::Selectable,
        Work::Nested,
        Work::Space,
        Work::DupLabel,
    ];

    fn sizes(self) -> &'static [usize] {
        // `space` and `dup_label` share one id key across every widget, which is
        // the case that used to go quadratic in `make_id`. Kept in the run as a
        // regression guard: they should now track `leaf_plain` / `label`.
        &[100, 1_000, 5_000, 20_000]
    }
}

/// Build `n` widgets. Label strings are pre-generated so we time the library,
/// not `format!`.
fn build(ui: &mut Ui, work: Work, n: usize, labels: &[String]) {
    let leaf = Layout::leaf(Size::Fixed(20.0), Size::Fixed(20.0));
    match work {
        // No widgets at all: just text measurement, the cost `label` and every
        // interactive widget pays at build time (and again inside paint, for
        // the ones that centre or right-align their text).
        Work::Measure => {
            let (font, size) = (ui.font, ui.theme.metrics.font_size);
            for l in labels.iter().take(n) {
                std::hint::black_box(ui.fonts.measure(font, size, l));
            }
        }
        // Same id hashing and tree work as `label`, but a non-capturing closure
        // (a ZST, so `Box` does not allocate) and no string.
        Work::LeafPlain => {
            for l in labels.iter().take(n) {
                let id = ui.make_id(("label", l.as_str()));
                ui.add_leaf(id, leaf, Vec2::ZERO, false, |_, _| {});
            }
        }
        // leaf_plain + exactly the two allocations `label` makes: the owned
        // string and the boxed closure that captures it.
        Work::LeafBoxed => {
            for l in labels.iter().take(n) {
                let id = ui.make_id(("label", l.as_str()));
                let text = l.clone();
                ui.add_leaf(id, leaf, Vec2::ZERO, false, move |_, r| {
                    std::hint::black_box((&text, r));
                });
            }
        }
        Work::Space => {
            for _ in 0..n {
                ui.space(4.0);
            }
        }
        Work::Label => {
            for l in labels.iter().take(n) {
                ui.label(l);
            }
        }
        Work::Button => {
            for l in labels.iter().take(n) {
                let _ = ui.button(l);
            }
        }
        Work::Selectable => {
            for (i, l) in labels.iter().take(n).enumerate() {
                let _ = ui.selectable(l, i % 7 == 0);
            }
        }
        Work::Nested => {
            for chunk in labels[..n].chunks(10) {
                ui.row(|ui| {
                    for l in chunk {
                        ui.label(l);
                    }
                });
            }
        }
        Work::DupLabel => {
            for _ in 0..n {
                ui.label("Item");
            }
        }
    }
}

struct Sample {
    build: Duration,
    end: Duration,
    allocs: u64,
    bytes: u64,
    instances: usize,
}

fn frame(ui: &mut Ui, work: Work, n: usize, labels: &[String]) -> Sample {
    let input = Input { screen_size: Vec2::new(1600.0, 1200.0), dt: 1.0 / 60.0, ..Input::default() };
    counter::reset();
    let t0 = Instant::now();
    ui.begin_frame(input);
    build(ui, work, n, labels);
    let t1 = Instant::now();
    let out = ui.end_frame();
    let instances = out.draw.instances.len();
    let t2 = Instant::now();
    let (allocs, bytes) = counter::read();
    Sample { build: t1 - t0, end: t2 - t1, allocs, bytes, instances }
}

fn pct(mut v: Vec<Duration>, p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p) as usize]
}

fn main() {
    let counting = cfg!(feature = "count-allocs");
    println!(
        "libgui frame benchmark  ({}, {})\n",
        if cfg!(debug_assertions) { "DEBUG - numbers are meaningless, use --release" } else { "release" },
        if counting { "counting allocations" } else { "timing" }
    );

    let labels: Vec<String> = (0..20_000).map(|i| format!("Object {i}")).collect();
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");

    println!(
        "{:<11} {:>7} {:>9} {:>9} {:>9} {:>9} {:>8} {:>10} {:>9}",
        "workload", "n", "build", "end_frame", "total", "p95", "us/widget", "allocs/w", "bytes/w"
    );
    println!("{}", "-".repeat(94));

    for work in Work::ALL {
        for &n in work.sizes() {
            // Warm up: glyph raster, map growth, animation settling.
            for _ in 0..30 {
                frame(&mut ui, work, n, &labels);
            }
            let runs = if n >= 20_000 { 60 } else { 200 };
            let mut builds = Vec::with_capacity(runs);
            let mut ends = Vec::with_capacity(runs);
            let mut totals = Vec::with_capacity(runs);
            let mut last = None;
            for _ in 0..runs {
                let s = frame(&mut ui, work, n, &labels);
                builds.push(s.build);
                ends.push(s.end);
                totals.push(s.build + s.end);
                last = Some(s);
            }
            let s = last.unwrap();
            let med = pct(totals.clone(), 0.5);
            let per = med.as_secs_f64() * 1e6 / n as f64;
            println!(
                "{:<11} {:>7} {:>9} {:>9} {:>9} {:>9} {:>8.3} {:>10.2} {:>9.0}",
                work.name(),
                n,
                format!("{:.2}ms", pct(builds, 0.5).as_secs_f64() * 1e3),
                format!("{:.2}ms", pct(ends, 0.5).as_secs_f64() * 1e3),
                format!("{:.2}ms", med.as_secs_f64() * 1e3),
                format!("{:.2}ms", pct(totals, 0.95).as_secs_f64() * 1e3),
                per,
                s.allocs as f64 / n as f64,
                s.bytes as f64 / n as f64,
            );
            let _ = s.instances;
        }
        println!();
    }
}
