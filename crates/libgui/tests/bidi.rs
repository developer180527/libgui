//! Right-to-left and mixed-direction text (WO-4), through a stub font whose
//! glyphs say which character they are: every character advances 10 px, and
//! its bitmap's width names it (a = 1 px, b = 2, …, א = 7, ב = 8, …). Reading
//! the drawn glyphs left to right by x then spells the line *as it is shown*.
#![cfg(feature = "bidi")]

use libgui::*;

/// Characters the tests use, by the bitmap width that names them.
const NAMES: &[(char, u32)] = &[('a', 1), ('b', 2), ('c', 3), ('d', 4), ('e', 5), ('f', 6), ('א', 7), ('ב', 8), ('ג', 9), ('ד', 10), ('(', 11), (')', 12)];

struct Stub;

impl FontRasterizer for Stub {
    fn line_metrics(&self, px: f32) -> LineMetrics {
        LineMetrics { ascent: px * 0.8, descent: -px * 0.2 }
    }
    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        for (byte, ch) in text.char_indices() {
            // Direction marks are invisible and take no room.
            let advance = if ch.is_control() || ('\u{200E}'..='\u{200F}').contains(&ch) { 0.0 } else { px };
            out.push(ShapedGlyph { glyph: ch as u32, face: 0, cluster: byte as u32, advance, offset: Vec2::ZERO });
        }
    }
    fn rasterize(&self, _face: u16, glyph: u32, _px: f32) -> GlyphBitmap {
        let w = char::from_u32(glyph).and_then(|c| NAMES.iter().find(|n| n.0 == c)).map_or(0, |n| n.1);
        GlyphBitmap { width: w, height: if w > 0 { 6 } else { 0 }, left: 0.0, bottom: 0.0, coverage: vec![255; (w * 6) as usize] }
    }
}

fn ui() -> Ui {
    Ui::with_rasterizer(Theme::dark(), Box::new(Stub))
}

const SIZE: f32 = 10.0;

/// The glyphs drawn on the row at `y` (± a pixel), left to right, as text.
fn spell(out: &FrameOutput, y: Option<f32>) -> String {
    let mut g: Vec<(f32, f32, char)> = out
        .draw
        .instances
        .iter()
        .filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code())
        .filter_map(|i| NAMES.iter().find(|n| n.1 as f32 == i.rect[2]).map(|n| (i.rect[0], i.rect[1], n.0)))
        .filter(|&(_, gy, _)| y.is_none_or(|y| (gy - y).abs() < 1.5))
        .collect();
    g.sort_by(|a, b| a.0.total_cmp(&b.0));
    g.into_iter().map(|t| t.2).collect()
}

/// Draw one line at the window's top-left and spell what was shown.
fn shown(text: &'static str) -> String {
    let mut ui = ui();
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
    ui.add_leaf(Id::new("t"), Layout::leaf(Size::Grow(1.0), Size::Fixed(20.0)), Vec2::ZERO, false, move |p, r| {
        p.text(Vec2::new(r.x, r.y), SIZE, Color::WHITE, text);
    });
    let out = ui.end_frame();
    spell(&out, None)
}

#[test]
fn a_hebrew_word_reads_right_to_left() {
    assert_eq!(shown("אבג"), "גבא");
}

#[test]
fn a_hebrew_word_inside_latin_turns_round_in_place() {
    assert_eq!(shown("abc אבג def"), "abcגבאdef");
}

/// A paragraph whose first strong letter is Hebrew runs right to left: its
/// Latin run is a unit, placed after (to the left of) the Hebrew.
#[test]
fn a_right_to_left_paragraph_orders_its_runs_from_the_right() {
    assert_eq!(shown("אבג abc"), "abcגבא");
    // A mark at the start makes a Latin-first paragraph right to left too.
    assert_eq!(shown("\u{200F}abc אבג"), "גבאabc");
}

#[test]
fn latin_alone_is_untouched() {
    assert_eq!(shown("abc def"), "abcdef");
}

/// The caret before a character sits at its leading edge: the right side of a
/// right-to-left one. So the start of a Hebrew word is its right end, its end
/// the left; and at a change of direction the caret goes with the character
/// after it.
#[test]
fn carets_sit_at_the_leading_edge() {
    let ui = ui();
    let f = &ui.fonts;
    let font = FontId(0);
    let heb = "אבג";
    assert_eq!(f.caret_x(font, SIZE, heb, 0), 30.0, "the start of a Hebrew word is not its right end");
    assert_eq!(f.caret_x(font, SIZE, heb, heb.len()), 0.0, "the end of a Hebrew word is not its left end");
    // "ab אב": a b space at 0..30, then ב at 30..40 and א at 40..50 on screen.
    let mixed = "ab אב";
    assert_eq!(f.caret_x(font, SIZE, mixed, 3), 50.0, "the caret before א is not at its right edge");
    assert_eq!(f.byte_at_x(font, SIZE, mixed, 49.0), 3, "a click at א's right edge is not the caret before it");
    assert_eq!(f.byte_at_x(font, SIZE, mixed, 2.0), 0);
}

/// One logical selection, two pieces on screen. In "abc אבג def" the bytes
/// from b to the end of ב — "bc " and "אב" — are "bc " and "בא" on screen,
/// with ג between them unselected: two rectangles, not one.
#[test]
fn a_selection_across_a_change_of_direction_is_two_spans() {
    let mut ui = ui();
    let mut text = String::from("abc אבג def");
    let info = FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 };
    let frame = |ui: &mut Ui, text: &mut String| -> (TextResponse, usize) {
        let sel = ui.theme.text_input.selection.to_array();
        ui.begin_frame(info);
        let r = ui.text_input("t", text, "");
        let out = ui.end_frame();
        let rects = out.draw.instances.iter().filter(|i| i.color == sel).count();
        (r, rects)
    };
    let id = frame(&mut ui, &mut text).0.response.id;
    ui.set_focus(Some(id));
    frame(&mut ui, &mut text);
    // To b, then Shift+Right four stops: c, the space, past it, and past ג's
    // right edge — which on screen is the caret before ג, after ב.
    ui.push(InputEvent::Action(UiAction::Move { motion: Motion::DocStart, select: false }));
    ui.push(InputEvent::Action(UiAction::Move { motion: Motion::Right, select: false }));
    for _ in 0..4 {
        ui.push(InputEvent::Action(UiAction::Move { motion: Motion::Right, select: true }));
    }
    let mut out = frame(&mut ui, &mut text);
    // Let the focus fade finish, so the selection is drawn in its colour.
    for _ in 0..40 {
        out = frame(&mut ui, &mut text);
    }
    let (r, rects) = out;
    assert_eq!(r.selection, (1, "abc אב".len()), "the arrows did not select b..ב");
    assert_eq!(rects, 2, "a selection across a change of direction is not two pieces on screen");
}

/// Left and Right move by what is on screen: through a Hebrew word inside
/// Latin, Right walks the caret steadily rightwards, never back.
#[test]
fn arrows_move_on_screen() {
    let mut ui = ui();
    let mut text = String::from("ab אבג cd");
    let caret = |ui: &mut Ui, text: &mut String| -> f32 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.text_area_with("t", text, TextAreaOptions { wrap: false, ..Default::default() });
        let out = ui.end_frame();
        let x = out.platform.text_input.map_or(f32::NAN, |r| r.x);
        drop(out);
        if !ui.focused().is_some_and(|f| f == r.response.id) {
            ui.set_focus(Some(r.response.id));
        }
        x
    };
    for _ in 0..3 {
        caret(&mut ui, &mut text);
    }
    let mut last = caret(&mut ui, &mut text);
    let mut xs = vec![last];
    for _ in 0..9 {
        ui.push(InputEvent::Action(UiAction::Move { motion: Motion::Right, select: false }));
        let x = caret(&mut ui, &mut text);
        xs.push(x);
        assert!(x >= last, "Right moved the caret left: {xs:?}");
        last = x;
    }
    assert!(xs.windows(2).filter(|w| w[1] > w[0]).count() >= 8, "Right stalled: {xs:?}");
}

/// A right-to-left paragraph in a text area starts at the field's right edge:
/// the caret at its start is drawn there.
#[test]
fn a_right_to_left_paragraph_starts_at_the_right() {
    let mut ui = ui();
    let mut text = String::from("אבג");
    let mut x = 0.0;
    let mut field = Rect::default();
    for k in 0..4 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.text_area("t", &mut text, 3);
        let out = ui.end_frame();
        x = out.platform.text_input.map_or(0.0, |r| r.x);
        field = r.response.rect;
        drop(out);
        if k == 1 {
            ui.set_focus(Some(r.response.id));
        }
    }
    assert!(x > field.center().x, "the caret at the start of a Hebrew paragraph is at x {x}, left of the field's middle");
}

/// Every wrapped row of a right-to-left paragraph is ordered by the
/// *paragraph's* direction — including a row that starts with a Latin word,
/// which on its own would read left to right.
#[test]
fn wrapped_rows_take_the_paragraph_s_direction() {
    let mut ui = ui();
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 200.0), scale: 1.0, dt: 1.0 / 60.0 });
    // Each row is "abc אבג": 70 px, in a box 75 px wide.
    let text = "\u{200F}abc אבג abc אבג abc אבג";
    ui.add_leaf(Id::new("p"), Layout::leaf(Size::Fixed(75.0), Size::Fixed(150.0)), Vec2::ZERO, false, move |p, r| {
        p.text_wrapped(r, SIZE, Color::WHITE, Align::Start, text);
    });
    let out = ui.end_frame();
    let ys: std::collections::BTreeSet<i32> = out.draw.instances.iter().map(|i| i.rect[1].round() as i32).collect();
    assert!(ys.len() >= 3, "the paragraph did not wrap: rows at {ys:?}");
    for y in ys {
        assert_eq!(spell(&out, Some(y as f32)), "גבאabc", "the row at y {y} is not ordered right to left");
    }
}

/// The spaces a wrap leaves at a row's end stay at its end (rule L1), even
/// between two Hebrew words in a left-to-right paragraph — where, ordered
/// with the Hebrew around them, they would land in the middle of the row. So
/// End on that row draws the caret at the row's right end.
#[test]
fn the_space_at_a_wrap_stays_at_the_row_s_end() {
    let mut ui = ui();
    // "abc אבג " fits a row; "דדד" goes to the next.
    let mut text = String::from("abc אבג דדד");
    let adv = ui.theme.metrics.font_size;
    let info = FrameInfo { screen_size: Vec2::new(adv * 8.0 + 40.0, 200.0), scale: 1.0, dt: 1.0 / 60.0 };
    let frame = |ui: &mut Ui, text: &mut String| -> (TextResponse, Rect) {
        ui.begin_frame(info);
        let r = ui.text_area("t", text, 4);
        let out = ui.end_frame();
        let caret = out.platform.text_input.unwrap_or_default();
        (r, caret)
    };
    let id = frame(&mut ui, &mut text).0.response.id;
    frame(&mut ui, &mut text);
    ui.set_focus(Some(id));
    let (_, start) = frame(&mut ui, &mut text);
    ui.push(InputEvent::Action(UiAction::Move { motion: Motion::LineEnd, select: false }));
    let (r, end) = frame(&mut ui, &mut text);
    assert_eq!(r.selection.0, "abc אבג ".len(), "the text did not wrap after the second word");
    assert!(end.x >= start.x + adv * 7.0 - 1.0, "End drew the caret at x {} — inside the row, not at its end (start {})", end.x, start.x);
}

/// Turned text is ordered like upright text. A slight turn keeps the upright
/// order; a half turn of "אבג", shown upright as "גבא", reads from left to
/// right on screen as "אבג".
#[test]
fn turned_text_is_reordered_too() {
    let spin = |text: &'static str, turn: f32, pad: f32| -> String {
        let mut ui = ui();
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.add_leaf(Id::new("t"), Layout::leaf(Size::Grow(1.0), Size::Fixed(40.0)), Vec2::ZERO, false, move |p, _| {
            p.text_rotated(Vec2::new(200.0, 50.0), SIZE, Color::WHITE, text, turn);
        });
        let out = ui.end_frame();
        // Turned glyphs are drawn half a texel wider on each side (`pad`).
        let mut g: Vec<(f32, char)> = out
            .draw
            .instances
            .iter()
            .filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code())
            .filter_map(|i| NAMES.iter().find(|n| (n.1 as f32 + pad - i.rect[2]).abs() < 0.01).map(|n| (i.rect[0], n.0)))
            .collect();
        g.sort_by(|a, b| a.0.total_cmp(&b.0));
        g.into_iter().map(|t| t.1).collect()
    };
    assert_eq!(spin("abc אבג", 0.3, 1.0), "abcגבא");
    assert_eq!(spin("אבג", std::f32::consts::PI, 1.0), "אבג");
}

/// A bracket in right-to-left text is drawn mirrored (rule L4): "א(ב)" reads
/// from the right as א, an opening bracket, ב, a closing one — so on screen,
/// left to right, "(ב)א". Unmirrored it would be ")ב(א".
#[test]
fn brackets_in_right_to_left_text_are_mirrored() {
    assert_eq!(shown("א(ב)"), "(ב)א");
    // In left-to-right text, nothing turns.
    assert_eq!(shown("a(b)"), "a(b)");
}
