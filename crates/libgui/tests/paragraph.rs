//! Wrapping text as a *layout* problem: a paragraph's height follows from the
//! width it is given, and everything below it has to move accordingly.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const TEXT: &str = "The quick brown fox jumps over the lazy dog, and then it does so again \
                    because one sentence was not enough to make this wrap at a sensible width.";

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info(w: f32) -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(w, 800.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// A paragraph with a marker above and below, so the test can see what moved.
fn build(ui: &mut Ui, text: &str) {
    ui.add_leaf(Id::new("above"), Layout::leaf(Size::Grow(1.0), Size::Fixed(10.0)), Vec2::ZERO, false, |_, _| {});
    ui.paragraph(text);
    ui.add_leaf(Id::new("below"), Layout::leaf(Size::Grow(1.0), Size::Fixed(10.0)), Vec2::ZERO, false, |_, _| {});
}

/// Run until the layout settles, and report the paragraph's rect and what
/// follows it.
fn settle(ui: &mut Ui, w: f32, text: &str) -> (Rect, f32) {
    for _ in 0..4 {
        ui.begin_frame(info(w));
        build(ui, text);
        let _ = ui.end_frame();
    }
    let p = ui.rect_of(Id::new("root").with(("paragraph", text))).expect("paragraph");
    let below = ui.rect_of(Id::new("below")).expect("below").y;
    (p, below)
}

#[test]
fn a_narrower_paragraph_is_taller_and_pushes_what_follows_it_down() {
    let mut wide = ui();
    let mut narrow = ui();
    let (w, wb) = settle(&mut wide, 600.0, TEXT);
    let (n, nb) = settle(&mut narrow, 240.0, TEXT);

    assert!(n.h > w.h, "narrowing the window did not make the paragraph taller: {} vs {}", n.h, w.h);
    assert!(nb > wb, "the widget below a taller paragraph did not move down: {nb} vs {wb}");
    assert_eq!(wb, w.y + w.h, "the widget below is not directly under the paragraph");
    assert_eq!(nb, n.y + n.h);
}

#[test]
fn the_height_is_a_whole_number_of_lines() {
    let ui = ui();
    let lh = ui.fonts.line_height(ui.font, ui.theme.metrics.font_size);
    for w in [200.0f32, 300.0, 420.0, 610.0] {
        let mut u = self::ui();
        let (p, _) = settle(&mut u, w, TEXT);
        let lines = p.h / lh;
        assert!((lines - lines.round()).abs() < 0.01, "at {w}px the paragraph was {lines} lines tall");
        assert!(lines >= 1.0);
    }
    let _ = lh;
}

#[test]
fn it_settles_in_one_extra_solve_and_then_stops_moving() {
    let mut ui = ui();
    let mut heights = Vec::new();
    for _ in 0..6 {
        ui.begin_frame(info(300.0));
        build(&mut ui, TEXT);
        let _ = ui.end_frame();
        heights.push(ui.rect_of(Id::new("root").with(("paragraph", TEXT))).map(|r| r.h));
    }
    // Frame one has no previous width to guess from; from frame two on it is
    // settled and must not oscillate.
    let settled: Vec<_> = heights[1..].to_vec();
    assert!(settled.windows(2).all(|w| w[0] == w[1]), "the paragraph never settled: {heights:?}");
    assert!(settled[0].unwrap() > 0.0);
}

#[test]
fn a_hard_newline_starts_a_line_however_much_room_is_left() {
    let mut ui = ui();
    let lh = ui.fonts.line_height(ui.font, ui.theme.metrics.font_size);
    let (p, _) = settle(&mut ui, 600.0, "one\ntwo\nthree");
    assert_eq!(p.h, lh * 3.0, "three short lines did not take three lines");
}

#[test]
fn a_paragraph_never_forces_the_panel_around_it_wider() {
    // Its minimum is its longest word, so a container that would otherwise be
    // sized by its children stays narrow rather than growing to one long line.
    let mut ui = ui();
    let longest = ui.fonts.min_wrap_width(ui.font, ui.theme.metrics.font_size, TEXT);
    let full = ui.fonts.measure(ui.font, ui.theme.metrics.font_size, TEXT).x;
    assert!(longest * 4.0 < full, "min width {longest} is not much less than the full {full}");

    for _ in 0..4 {
        ui.begin_frame(info(240.0));
        ui.container_id(Id::new("panel"), Layout::column().width(Size::Grow(1.0)).height(Size::Fit), Frame::none(), |ui| {
            ui.paragraph(TEXT);
        });
        let _ = ui.end_frame();
    }
    let panel = ui.rect_of(Id::new("panel")).expect("panel");
    assert!(panel.w <= 240.0, "the paragraph pushed its panel to {}px", panel.w);
}

#[test]
fn wrapping_is_measured_once_and_drawn_from_the_same_answer() {
    // Measuring and drawing both ask for the wrap; the cache means the text is
    // shaped once, not once per pass.
    let mut ui = ui();
    for _ in 0..4 {
        ui.begin_frame(info(300.0));
        build(&mut ui, TEXT);
        let _ = ui.end_frame();
    }
    assert_eq!(ui.frame_cost().text_shaped, 0, "a settled paragraph re-shaped its text");
    assert_eq!(ui.frame_cost().glyphs_rasterized, 0);
}

#[test]
fn cjk_wraps_between_characters_rather_than_running_off_the_edge() {
    let mut ui = ui();
    let text = "日本語のテキストは、単語の区切りが無くても折り返します。";
    let lh = ui.fonts.line_height(ui.font, ui.theme.metrics.font_size);
    let (p, _) = settle(&mut ui, 160.0, text);
    assert!(p.h > lh * 1.5, "CJK did not wrap: one line of {}px", p.h);
    assert!(p.w <= 160.0);
}

/// The whole reason a frame containing a paragraph solves twice: on the frame
/// the width changes, the height that follows from it has to be right *then*,
/// not one frame later. A paragraph that lagged by a frame would leave a gap
/// under it, or overlap what comes next, every time a panel is resized — which
/// is continuously, while someone drags a splitter.
#[test]
fn a_width_change_is_absorbed_in_the_same_frame() {
    let mut ui = ui();
    for _ in 0..4 {
        ui.begin_frame(info(600.0));
        build(&mut ui, TEXT);
        let _ = ui.end_frame();
    }
    let wide = ui.rect_of(Id::new("root").with(("paragraph", TEXT))).unwrap();

    // One frame at half the width. No settling: this is the frame that has to
    // be right.
    ui.begin_frame(info(240.0));
    build(&mut ui, TEXT);
    let _ = ui.end_frame();
    let narrow = ui.rect_of(Id::new("root").with(("paragraph", TEXT))).unwrap();
    let below = ui.rect_of(Id::new("below")).unwrap().y;

    assert!(narrow.h > wide.h, "the paragraph did not grow taller on the frame it narrowed");
    assert_eq!(below, narrow.y + narrow.h, "what follows the paragraph was left at the old height");

    // …and widening again is absorbed the same way.
    ui.begin_frame(info(600.0));
    build(&mut ui, TEXT);
    let _ = ui.end_frame();
    let back = ui.rect_of(Id::new("root").with(("paragraph", TEXT))).unwrap();
    assert_eq!(back.h, wide.h, "widening again did not restore the height");
}
