//! Performance guards: a pro app's UI must stay negligible next to its real
//! work (simulation, rendering, solving). These are *tests*, not a benchmark,
//! so they assert properties that hold on any machine:
//!
//! - deterministic counts (draw instances, glyph rasterisation) — zero flake;
//! - an idle UI costing nothing at all, which is what actually matters for a
//!   tool that sits still most of the time;
//! - *ratios* rather than absolute times for complexity, so a slow or loaded
//!   CI box cannot produce a false failure;
//! - one absolute budget, asserted only in release (a debug build is an order
//!   of magnitude slower and its timings mean nothing) with wide headroom.
//!
//! See `crates/libgui_bench` for measurement; this file is for regressions.

use libgui::*;
use std::time::{Duration, Instant};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// A realistic inspector: headings, sections, sliders, toggles and a list.
/// Names repeat (objects in a scene usually do), which is also the shape that
/// used to make id disambiguation quadratic.
fn panel(ui: &mut Ui, rows: usize, names: &[String]) {
    ui.heading("Inspector");
    ui.section("Transform");
    let mut v = 0.5;
    for i in 0..3 {
        ui.slider_keyed(i, "Position", &mut v, 0.0, 1.0);
    }
    let mut b = true;
    ui.toggle("Visible", &mut b);
    ui.toggle("Cast Shadows", &mut b);
    ui.separator();
    ui.section("Objects");
    for (i, name) in names.iter().enumerate().take(rows) {
        ui.with_key(i, |ui| {
            let _ = ui.selectable(name, i == 3);
        });
    }
    let _ = ui.button("Add");
    let _ = ui.button_primary("Apply");
}

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Mesh {}", i % 7)).collect()
}

fn frame(ui: &mut Ui, rows: usize, names: &[String]) -> (usize, u64, Option<f32>) {
    ui.begin_frame(FrameInfo::default());
    panel(ui, rows, names);
    let o = ui.end_frame();
    (o.draw.instances.len(), o.atlas.version, o.platform.repaint_after)
}

/// Lowest of `runs` samples: noise only ever adds time, so the minimum is the
/// most robust estimate of the real cost on a shared machine.
fn fastest(runs: usize, mut f: impl FnMut()) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed());
    }
    best
}

/// Nothing may accumulate between identical frames: same draw work, and no
/// glyph re-rasterisation once the atlas is warm.
#[test]
fn a_steady_frame_repeats_exactly() {
    let mut ui = ui();
    let names = names(400);
    for _ in 0..60 {
        frame(&mut ui, 200, &names);
    }
    let (inst_a, atlas_a, _) = frame(&mut ui, 200, &names);
    for _ in 0..20 {
        frame(&mut ui, 200, &names);
    }
    let (inst_b, atlas_b, _) = frame(&mut ui, 200, &names);
    assert_eq!(inst_a, inst_b, "draw work drifted between identical frames");
    assert_eq!(atlas_a, atlas_b, "glyphs are being re-rasterised every frame");
}

/// Widgets scrolled or laid out past the window edge must not reach the GPU.
/// Build cost still scales with widget count (that is what virtualised lists
/// are for), but draw cost must not.
#[test]
fn offscreen_widgets_cost_no_draw_work() {
    let mut ui = ui();
    let names = names(2000);
    for _ in 0..30 {
        frame(&mut ui, 100, &names);
    }
    let (few, _, _) = frame(&mut ui, 100, &names);
    for _ in 0..30 {
        frame(&mut ui, 1500, &names);
    }
    let (many, _, _) = frame(&mut ui, 1500, &names);
    assert_eq!(few, many, "15x the rows produced {many} instances instead of {few}: culling broke");
}

/// The property that decides whether a tool app burns a core while the user
/// reads the screen: with nothing happening, the UI asks the host not to
/// redraw at all. An animation that never settles would pin the CPU forever.
#[test]
fn an_idle_ui_lets_the_host_sleep() {
    let mut ui = ui();
    let names = names(64);
    for _ in 0..60 {
        frame(&mut ui, 32, &names);
    }
    let (_, _, idle) = frame(&mut ui, 32, &names);
    assert_eq!(idle, None, "an untouched UI still asks to be redrawn");

    // Hover a row: now it animates and must ask for frames.
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(40.0, 300.0) });
    let (_, _, hovered) = frame(&mut ui, 32, &names);
    assert_eq!(hovered, Some(0.0), "a hover must drive repaints while it animates");

    // Move away: the animation must run down and stop asking, in bounded time.
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(4000.0, 4000.0) });
    let mut settled = None;
    for i in 0..240 {
        if frame(&mut ui, 32, &names).2.is_none() {
            settled = Some(i);
            break;
        }
    }
    let settled = settled.expect("hover animation never settled: the host can never sleep");
    assert!(settled < 120, "took {settled} frames to go idle");
}

/// Complexity guard. A ratio, so it is independent of machine, load and build
/// profile: quadratic cost would show up as ~16x for 4x the widgets. This is
/// the test that would have caught the O(n^2) id disambiguation.
#[test]
fn frame_cost_grows_linearly_with_widget_count() {
    let mut ui = ui();
    let names = names(8000);
    let mut measure = |rows: usize| {
        for _ in 0..20 {
            frame(&mut ui, rows, &names);
        }
        fastest(30, || {
            frame(&mut ui, rows, &names);
        })
    };
    let small = measure(500);
    let large = measure(4 * 500);
    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
    println!("500 rows {small:?}, 2000 rows {large:?}, ratio {ratio:.2} (linear = 4.0)");
    assert!(ratio < 7.0, "4x the widgets cost {ratio:.1}x the time: worse than linear");
}

/// Same guard on the path where every widget shares one id key, which is what
/// went quadratic before: spacers, separators and repeated labels.
#[test]
fn repeated_keys_stay_linear() {
    let mut ui = ui();
    let mut measure = |n: usize| {
        let build = |ui: &mut Ui| {
            ui.begin_frame(FrameInfo::default());
            for _ in 0..n {
                ui.space(2.0);
                ui.label("Mesh");
            }
            let _ = ui.end_frame();
        };
        for _ in 0..10 {
            build(&mut ui);
        }
        fastest(20, || build(&mut ui))
    };
    let small = measure(500);
    let large = measure(4 * 500);
    let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
    println!("500 repeated-key widgets {small:?}, 2000 {large:?}, ratio {ratio:.2}");
    assert!(ratio < 7.0, "repeated keys cost {ratio:.1}x for 4x the widgets");
}

/// The point of a virtual list: cost tracks what is *visible*, not what the
/// list contains. A scene outliner with a million objects must cost the same
/// as one with fifty, in draw work and in time.
#[test]
fn a_virtual_list_does_not_care_how_long_it_is() {
    let mut ui = ui();
    let names = names(64);
    let mut run = |rows: usize| -> (usize, usize, Duration) {
        let mut built = 0;
        let mut frame = |ui: &mut Ui| {
            ui.begin_frame(FrameInfo::default());
            built = ui
                .virtual_list("objects", rows, 24.0, |ui, i| {
                    let _ = ui.selectable(&names[i % 64], false);
                })
                .len();
            ui.end_frame().draw.instances.len()
        };
        for _ in 0..30 {
            frame(&mut ui);
        }
        let instances = frame(&mut ui);
        let cost = fastest(40, || {
            frame(&mut ui);
        });
        (built, instances, cost)
    };

    let (small_built, small_inst, small_cost) = run(100);
    let (big_built, big_inst, big_cost) = run(1_000_000);
    let ratio = big_cost.as_secs_f64() / small_cost.as_secs_f64().max(1e-9);
    println!(
        "100 rows: {small_built} built, {small_inst} instances, {small_cost:?}\n         1_000_000 rows: {big_built} built, {big_inst} instances, {big_cost:?}  (ratio {ratio:.2})"
    );

    assert_eq!(small_built, big_built, "a longer list built more rows");
    assert_eq!(small_inst, big_inst, "a longer list produced more draw work");
    assert!(ratio < 2.0, "10_000x the rows cost {ratio:.1}x the time");
}

/// Variable row heights cost one height lookup per row to locate the window,
/// so unlike the uniform case the frame is O(rows). Pin what that costs, and
/// pin that *building* still only touches the visible rows.
#[test]
fn variable_height_rows_pay_only_for_locating() {
    let mut ui = ui();
    let names = names(64);
    let h = |i: usize| [18.0f32, 40.0, 26.0][i % 3];
    let mut run = |rows: usize| -> (usize, Duration) {
        let mut built = 0;
        let mut frame = |ui: &mut Ui| {
            ui.begin_frame(FrameInfo::default());
            built = ui
                .virtual_rows("rows", rows, h, |ui, i| {
                    let _ = ui.selectable_keyed(i, &names[i % 64], false);
                })
                .len();
            let _ = ui.end_frame();
        };
        for _ in 0..20 {
            frame(&mut ui);
        }
        let cost = fastest(30, || frame(&mut ui));
        (built, cost)
    };

    let (small_built, small) = run(1_000);
    let (big_built, big) = run(100_000);
    println!("1k variable rows: {small_built} built, {small:?}");
    println!("100k variable rows: {big_built} built, {big:?}");

    // Building must not grow with the list, only the height scan does.
    assert_eq!(small_built, big_built, "a longer list built more rows");
    if cfg!(debug_assertions) {
        return;
    }
    // 100k rows is far past what this path is meant for and it still has to
    // stay well inside a frame; a regression to O(rows) *building* would be
    // orders of magnitude worse than this.
    assert!(big < Duration::from_millis(4), "100k variable rows took {big:?}");
}

/// A line's quad is its bounding box, so a long diagonal would rasterise its
/// whole box to draw a thin line. Pin that the emitted quads stay close to the
/// area the line actually covers.
#[test]
fn a_long_diagonal_does_not_rasterise_its_bounding_box() {
    use libgui::render_contract::PrimitiveKind;
    let mut ui = ui();
    let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(800.0, 600.0));
    let width = 2.0;

    ui.begin_frame(FrameInfo { screen_size: Vec2::new(1600.0, 1200.0), ..FrameInfo::default() });
    let id = ui.make_id("l");
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, _| {
        p.line(a, b, width, Color::WHITE);
    });
    let out = ui.end_frame();

    let quads: f32 = out
        .draw
        .instances
        .iter()
        .filter(|i| PrimitiveKind::from_code(i.params[3]) == Some(PrimitiveKind::Line))
        .map(|i| i.rect[2] * i.rect[3])
        .sum();
    let covered = (b.x - a.x).hypot(b.y - a.y) * width;
    let whole_box = (b.x - a.x) * (b.y - a.y);
    let ratio = quads / covered;
    println!(
        "diagonal: {quads:.0}px of quad for {covered:.0}px of line ({ratio:.1}x); \
         whole box would be {whole_box:.0}px ({:.0}x saved)",
        whole_box / quads
    );
    // Each piece carries an anti-aliasing pad, so splitting cannot drive this
    // to 1: past a point the pad, not the box, is the cost. What matters is
    // that it no longer scales with the bounding box.
    assert!(ratio < 15.0, "rasterising {ratio:.0}x the line's own area");
    assert!(quads < whole_box * 0.1, "barely better than the whole bounding box");
}

/// The headline claim, as a number. A visible inspector must be a rounding
/// error in a 60 fps frame. Asserted in release only: a debug build is ~10x
/// slower and its timings say nothing about shipped code.
#[test]
fn a_visible_panel_is_a_rounding_error_in_a_frame() {
    let mut ui = ui();
    let names = names(400);
    for _ in 0..60 {
        frame(&mut ui, 200, &names);
    }
    let cost = fastest(60, || {
        frame(&mut ui, 200, &names);
    });
    let frame_60hz = Duration::from_secs_f64(1.0 / 60.0);
    let share = cost.as_secs_f64() / frame_60hz.as_secs_f64() * 100.0;
    println!("211-widget panel: {cost:?} = {share:.2}% of a 60 fps frame");
    if cfg!(debug_assertions) {
        println!("(debug build: budget not asserted)");
        return;
    }
    // ~0.05 ms here, so this leaves more than an order of magnitude of room
    // for a slower or loaded machine while still catching a real regression.
    assert!(share < 5.0, "a single panel costs {share:.1}% of a frame");
}
