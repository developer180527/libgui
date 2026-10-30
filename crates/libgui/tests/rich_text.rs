//! Rich text and font roles: several faces in one wrapped paragraph, on one
//! baseline, with clickable runs; and text drawn in the face its role names.

use libgui::*;
use std::rc::Rc;

/// A face whose every glyph advances `adv` × the size / 10 and draws a box
/// `mark` px wide, so a glyph's face can be read off the draw list.
struct Stub {
    adv: f32,
    mark: u32,
}

impl FontRasterizer for Stub {
    fn line_metrics(&self, px: f32) -> LineMetrics {
        LineMetrics { ascent: px * 0.8, descent: -px * 0.2 }
    }
    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        for (b, ch) in text.char_indices() {
            out.push(ShapedGlyph { glyph: ch as u32, face: 0, cluster: b as u32, advance: self.adv * px / 10.0, offset: Vec2::ZERO });
        }
    }
    fn rasterize(&self, _face: u16, glyph: u32, _px: f32) -> GlyphBitmap {
        let w = if glyph == ' ' as u32 { 0 } else { self.mark };
        GlyphBitmap { width: w, height: if w > 0 { 4 } else { 0 }, left: 0.0, bottom: 0.0, coverage: vec![255; (w * 4) as usize] }
    }
}

/// A Ui whose body face draws 1-px marks and whose bold face 2-px ones, each
/// glyph 10 px wide at size 10.
fn ui() -> (Ui, FontId, FontId) {
    let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(Stub { adv: 10.0, mark: 1 }));
    let bold = ui.fonts.add_rasterizer(Box::new(Stub { adv: 10.0, mark: 2 }));
    ui.set_font_role(FontRole::Bold, bold);
    let body = ui.font;
    (ui, body, bold)
}

fn run(text: &str, font: FontId, size: f32) -> TextRun {
    TextRun::new(text, font, size, Color::WHITE)
}

#[test]
fn roles_fall_back_to_the_body_face() {
    let (mut ui, body, bold) = ui();
    assert_eq!(ui.font_for(FontRole::Body), body);
    assert_eq!(ui.font_for(FontRole::Bold), bold);
    assert_eq!(ui.font_for(FontRole::Italic), body, "an unset role is the body face");
    assert_eq!(ui.font_for(FontRole::BoldItalic), bold, "bold italic falls back to bold first");
    assert!(!ui.has_font_role(FontRole::Mono));
    let mono = ui.fonts.add_rasterizer(Box::new(Stub { adv: 10.0, mark: 3 }));
    ui.set_font_role(FontRole::Mono, mono);
    assert_eq!(ui.font_for(FontRole::Mono), mono);
    assert_eq!(FontRole::of(true, true, false), FontRole::BoldItalic);
    assert_eq!(FontRole::of(true, false, true), FontRole::Mono);
}

/// Words wrap at spaces across runs; "bb" (bold) and "cc" (body) with no
/// space between are one word and stay on one line.
#[test]
fn words_wrap_across_runs_but_not_inside_one() {
    let (ui, body, bold) = ui();
    let mut rich = RichText::new();
    rich.push(run("aaaa ", body, 10.0)).push(run("bb", bold, 10.0)).push(run("cc dd", body, 10.0));
    // 60 px: "aaaa" (40) fits, "bbcc" (40) does not fit after it.
    let l = rich_layout(&ui, &rich, 60.0, Align::Start);
    let lines: Vec<(String, f32)> = by_line(&rich, &l);
    assert_eq!(lines.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["aaaa", "bbcc", "dd"]);
    assert_eq!(l.size, Vec2::new(40.0, 30.0), "three 10 px lines, the widest 40 px");
    // The narrowest it can be is its longest word.
    assert_eq!(rich_min_width(&ui, &rich), 40.0);
}

/// Runs of two sizes on one line share a baseline, and the line is as tall as
/// it needs to be for both.
#[test]
fn sizes_share_a_baseline() {
    let (ui, body, bold) = ui();
    let mut rich = RichText::new();
    rich.push(run("aa ", body, 10.0)).push(run("BB", bold, 20.0));
    let l = rich_layout(&ui, &rich, 400.0, Align::Start);
    assert_eq!(l.pieces.len(), 2);
    let base = |p: &RichPiece| p.y + p.ascent;
    assert_eq!(base(&l.pieces[0]), base(&l.pieces[1]), "the runs do not share a baseline");
    assert!(l.pieces[0].y > l.pieces[1].y, "the small run should sit lower, on the large one's baseline");
    assert_eq!(l.size.y, 20.0);
}

#[test]
fn alignment_moves_each_line() {
    let (ui, body, _) = ui();
    let mut rich = RichText::new();
    rich.push(run("aa", body, 10.0));
    assert_eq!(rich_layout(&ui, &rich, 100.0, Align::Center).pieces[0].x, 40.0);
    assert_eq!(rich_layout(&ui, &rich, 100.0, Align::End).pieces[0].x, 80.0);
}

/// A newline breaks the line, and an empty line keeps its height.
#[test]
fn newlines_break_lines() {
    let (ui, body, _) = ui();
    let mut rich = RichText::new();
    rich.push(run("aa\n\nbb", body, 10.0));
    let l = rich_layout(&ui, &rich, 400.0, Align::Start);
    assert_eq!(by_line(&rich, &l).iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["aa", "bb"]);
    assert_eq!(l.size.y, 30.0, "the empty line between has no height");
}

/// In a column, a wrapped rich paragraph pushes what follows down by its real
/// height — on the very frame its width changes, because it is re-measured
/// once layout knows the new width, not a frame later.
#[test]
fn a_wrapped_paragraph_takes_its_height_in_the_layout() {
    let (mut ui, body, bold) = ui();
    let mut rich = RichText::new();
    rich.push(run("aaaa bbbb ", body, 10.0)).push(run("cccc dddd", bold, 10.0));
    let rich = Rc::new(rich);
    let below = |ui: &mut Ui, w: f32| -> f32 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.container(Layout::column().width(Size::Fixed(w)), Frame::none(), |ui| {
            ui.rich_text("p", rich.clone());
            ui.add_leaf(Id::new("after"), Layout::leaf(Size::Fixed(10.0), Size::Fixed(10.0)), Vec2::ZERO, false, |_, _| {});
        });
        drop(ui.end_frame());
        ui.rect_of(Id::new("after")).map_or(0.0, |r| r.y)
    };
    below(&mut ui, 300.0);
    assert_eq!(below(&mut ui, 300.0), 10.0, "one line at 300 px");
    assert_eq!(below(&mut ui, 60.0), 40.0, "four lines at 60 px, on the frame the width changed");
}

/// A run with a tag is a link: hovering it says so, clicking it reports it,
/// and a click on untagged text reports nothing.
#[test]
fn a_tagged_run_is_clickable() {
    let (mut ui, body, _) = ui();
    let mut rich = RichText::new();
    rich.push(run("see ", body, 10.0));
    let mut link = run("here", body, 10.0);
    link.tag = 7;
    link.underline = true;
    rich.push(link);
    let rich = Rc::new(rich);
    let mut frame = |ui: &mut Ui| -> RichResponse {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.rich_text("p", rich.clone());
        drop(ui.end_frame());
        r
    };
    frame(&mut ui);
    frame(&mut ui);
    let click = |ui: &mut Ui, x: f32, frame: &mut dyn FnMut(&mut Ui) -> RichResponse| -> (RichResponse, RichResponse) {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, 5.0) });
        let hover = frame(ui);
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        frame(ui);
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        (hover, frame(ui))
    };
    let (hover, clicked) = click(&mut ui, 55.0, &mut frame);
    assert_eq!(hover.hovered_tag, 7, "the link was not hovered");
    assert_eq!(ui.cursor, Cursor::Pointer);
    assert_eq!(clicked.clicked_tag, 7, "the link's click was not reported");
    let (_, plain) = click(&mut ui, 15.0, &mut frame);
    assert_eq!(plain.clicked_tag, 0, "a click on plain text reported a link");
}

/// Plain rich text (no tags) takes no input: it is not a hit target.
#[test]
fn untagged_rich_text_is_not_interactive() {
    let (mut ui, body, _) = ui();
    let mut rich = RichText::new();
    rich.push(run("just words", body, 10.0));
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
    ui.rich_text("p", Rc::new(rich));
    drop(ui.end_frame());
    assert!(ui.hit_test(Vec2::new(5.0, 5.0)).is_none());
}

/// A mono text area draws in the mono face; without one set, the body face.
#[test]
fn a_mono_text_area_draws_in_the_mono_face() {
    let (mut ui, _, _) = ui();
    let marks = |ui: &mut Ui| -> Vec<f32> {
        let mut code = String::from("fn x");
        let mut m = Vec::new();
        for _ in 0..2 {
            ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 200.0), scale: 1.0, dt: 1.0 / 60.0 });
            ui.text_area_with("code", &mut code, TextAreaOptions { mono: true, ..Default::default() });
            let out = ui.end_frame();
            m = out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).map(|i| i.rect[2]).collect();
        }
        m
    };
    assert_eq!(marks(&mut ui), vec![1.0; 3], "with no mono face, the body face");
    let mono = ui.fonts.add_rasterizer(Box::new(Stub { adv: 10.0, mark: 3 }));
    ui.set_font_role(FontRole::Mono, mono);
    assert_eq!(marks(&mut ui), vec![3.0; 3], "the mono face was not used");
}

fn rich_layout(ui: &Ui, rich: &RichText, w: f32, align: Align) -> RichLayout {
    libgui::rich_layout(&ui.fonts, rich, w, align)
}

fn rich_min_width(ui: &Ui, rich: &RichText) -> f32 {
    libgui::rich_min_width(&ui.fonts, rich)
}

/// The text on each line, and the line's top.
fn by_line(rich: &RichText, l: &RichLayout) -> Vec<(String, f32)> {
    let mut lines: Vec<(String, f32)> = Vec::new();
    for p in &l.pieces {
        let t = &rich.runs[p.run as usize].text[p.start as usize..p.end as usize];
        match lines.last_mut() {
            Some(last) if last.1 == p.line_top => last.0.push_str(t),
            _ => lines.push((t.to_string(), p.line_top)),
        }
    }
    lines
}
