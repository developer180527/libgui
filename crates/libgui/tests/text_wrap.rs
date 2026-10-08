//! Word wrap in the text area (WO-2): rows on screen are what Home, End, Up
//! and Down move by, a wrap point is two caret positions, the pointer lands
//! where it is aimed, and a long document still costs a screenful.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

const PARA: &str = "The quick brown fox jumps over the lazy dog while the five boxing wizards jump quickly past the sphinx of black quartz.";

struct Field {
    ui: Ui,
    text: String,
    width: f32,
    opts: TextAreaOptions,
}

/// What a frame reported, and where the caret was drawn.
struct Seen {
    r: TextResponse,
    caret: Rect,
}

impl Field {
    fn new(text: &str) -> Self {
        Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), text: text.into(), width: 260.0, opts: TextAreaOptions { rows: 8, ..Default::default() } }
    }

    fn frame(&mut self) -> Seen {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(self.width, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = self.ui.text_area_with("note", &mut self.text, self.opts);
        let out = self.ui.end_frame();
        let caret = out.platform.text_input.unwrap_or_default();
        Seen { r, caret }
    }

    fn focus(&mut self) -> Seen {
        for _ in 0..2 {
            self.frame();
        }
        let id = self.frame().r.response.id;
        self.ui.set_focus(Some(id));
        self.frame()
    }

    fn act(&mut self, a: UiAction) -> Seen {
        self.ui.push(InputEvent::Action(a));
        self.frame()
    }

    fn mv(&mut self, motion: Motion) -> Seen {
        self.act(UiAction::Move { motion, select: false })
    }

    fn click(&mut self, at: Vec2) -> Seen {
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame()
    }
}

/// Down moves to the next row of the same paragraph, and the caret is drawn
/// a line lower: the paragraph is several rows on screen.
#[test]
fn down_moves_by_the_rows_on_screen() {
    let mut f = Field::new(&format!("{PARA}\nsecond"));
    let start = f.focus();
    assert_eq!(start.r.selection.0, 0);
    let s = f.mv(Motion::Down);
    let b = s.r.selection.0;
    assert!(b > 0 && b < PARA.len(), "Down left the wrapped paragraph: byte {b}");
    assert!(s.caret.y > start.caret.y + 5.0, "the caret was not drawn on the next row");
    assert_eq!(s.r.caret.0, 0, "the logical line changed although the caret stayed in the paragraph");
}

/// End goes to the end of the row on screen, drawn there — not at the start
/// of the next row, which is the same byte. Typing goes in there; Home then
/// goes to the row's start, not the paragraph's.
#[test]
fn end_and_home_are_the_row_s_ends() {
    let mut f = Field::new(PARA);
    let start = f.focus();
    let end = f.mv(Motion::LineEnd);
    let b = end.r.selection.0;
    assert!(b > 0 && b < PARA.len(), "End went to the paragraph's end, not the row's: {b}");
    assert!((end.caret.y - start.caret.y).abs() < 1.0, "End drew the caret on the next row");
    assert!(end.caret.x > start.caret.x + 100.0, "End did not draw the caret at the row's right end");
    // Down from the end of row one, Up again: back on row one, not two.
    f.mv(Motion::Down);
    let back = f.mv(Motion::Up);
    assert!((back.caret.y - start.caret.y).abs() < 1.0);
    // Home on the second row is the second row's start.
    f.mv(Motion::Down);
    let row2 = f.mv(Motion::LineStart);
    assert!(row2.r.selection.0 > 0, "Home went to the paragraph's start");
    assert!((row2.caret.x - start.caret.x).abs() < 1.0, "Home did not draw the caret at the left edge");
    assert!(row2.caret.y > start.caret.y + 5.0);
}

/// Up and Down aim for the same x across rows of any length.
#[test]
fn vertical_motion_keeps_its_x_across_wrapped_rows() {
    let mut f = Field::new(&format!("{PARA}\n{PARA}"));
    f.focus();
    for _ in 0..4 {
        f.mv(Motion::Right);
    }
    let first = f.mv(Motion::Right);
    let x = first.caret.x;
    for _ in 0..4 {
        let s = f.mv(Motion::Down);
        assert!((s.caret.x - x).abs() < 8.0, "the caret drifted from x {x} to {}", s.caret.x);
    }
}

/// A click right of a row's last word lands at the end of that row, drawn
/// there; a click at the next row's left edge is the next row.
#[test]
fn the_pointer_lands_on_the_row_it_points_at() {
    let mut f = Field::new(PARA);
    let start = f.focus();
    let rect = start.r.response.rect;
    let row1 = start.caret.y + start.caret.h * 0.5;
    let right = f.click(Vec2::new(rect.right() - 6.0, row1));
    assert!((right.caret.y - start.caret.y).abs() < 1.0, "a click on row one put the caret on row two");
    assert!(right.r.selection.0 > 0 && right.r.selection.0 < PARA.len());
    let next = f.click(Vec2::new(rect.x + 2.0, row1 + start.caret.h));
    assert!(next.caret.y > start.caret.y + 5.0, "a click on row two did not reach it");
}

/// The field re-wraps to its width: narrower is more rows, wider fewer, and
/// a caret at the end of the text is still drawn on its last row.
#[test]
fn a_change_of_width_rewraps() {
    let rows_down = |width: f32| -> usize {
        let mut f = Field::new(PARA);
        f.width = width;
        f.focus();
        let mut n = 0;
        loop {
            let before = f.frame().r.selection.0;
            let after = f.mv(Motion::Down).r.selection.0;
            if after == before || after == PARA.len() {
                return n + 1;
            }
            n += 1;
        }
    };
    let (narrow, wide) = (rows_down(180.0), rows_down(420.0));
    assert!(narrow > wide && wide >= 2, "{narrow} rows narrow, {wide} wide");
    let mut f = Field::new(PARA);
    f.focus();
    f.width = 160.0;
    f.frame();
    let end = f.mv(Motion::DocEnd);
    assert_eq!(end.r.selection.0, PARA.len());
    assert!(end.caret.y > 0.0 && end.caret.y < 400.0, "the caret at the end was not drawn: {:?}", end.caret);
}

/// Ten thousand paragraphs, each several rows: an idle frame and a jump to
/// the end both cost about a screenful of shaping, not the document.
#[test]
fn a_long_wrapped_document_costs_a_screenful() {
    let doc: String = (0..10_000).map(|i| format!("{i} {PARA}\n")).collect();
    let mut f = Field::new(&doc);
    f.focus();
    f.frame();
    f.frame();
    let idle = f.ui.frame_cost();
    assert_eq!(idle.text_shaped, 0, "an idle frame shaped text");
    assert!(idle.text_scanned < 4_000, "an idle frame read {} bytes", idle.text_scanned);
    let end = f.mv(Motion::DocEnd);
    let jump = f.ui.frame_cost();
    assert_eq!(end.r.caret.0, 10_000, "DocEnd did not reach the last line");
    assert!(jump.text_shaped < 80, "the jump shaped {} strings: it wrapped the document on the way", jump.text_shaped);
    assert!(end.caret.y > 0.0, "the caret at the end is not on screen");
}

/// An input method's composition is drawn inline at the caret on its row,
/// and the IME is told how wide it is.
#[test]
fn a_composition_is_drawn_inline() {
    let mut f = Field::new(PARA);
    let before = f.focus();
    let glyphs = |f: &mut Field| -> usize {
        f.ui.begin_frame(FrameInfo { screen_size: Vec2::new(f.width, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        f.ui.text_area_with("note", &mut f.text, f.opts);
        let out = f.ui.end_frame();
        out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).count()
    };
    let plain = glyphs(&mut f);
    f.ui.push(InputEvent::ImePreedit { text: "にほん".into(), cursor: 9 });
    let s = f.frame();
    let composing = glyphs(&mut f);
    assert!(composing > plain, "the composition was not drawn ({plain} -> {composing} glyphs)");
    assert!(s.caret.w > 10.0, "the IME was not told the composition's width: {:?}", s.caret);
    assert_eq!(f.text, PARA, "the composition reached the text before it was committed");
    let _ = before;
}

/// Off, a long line stays one row and scrolls sideways, as code wants.
#[test]
fn without_wrap_a_long_line_scrolls_sideways() {
    let mut f = Field::new(PARA);
    f.opts.wrap = false;
    let start = f.focus();
    let end = f.mv(Motion::LineEnd);
    assert_eq!(end.r.selection.0, PARA.len(), "End stopped short of the line's end");
    assert!((end.caret.y - start.caret.y).abs() < 1.0);
    assert!(end.caret.x < f.width, "the field did not scroll to keep the caret in view");
    assert!(f.mv(Motion::Down).r.selection.0 == PARA.len());
}
