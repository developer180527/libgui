//! Text in several colours (syntax highlighting): one run shaped once, each
//! glyph coloured by the span holding it, the layout unchanged by colour.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const RED: Color = Color::hex(0xff0000);
const GREEN: Color = Color::hex(0x00ff00);

fn glyph_kind() -> f32 {
    render_contract::PrimitiveKind::Glyph.code()
}

/// Glyphs drawn by `paint`, as (rect, colour), left to right.
fn glyphs(paint: impl Fn(&mut Painter, Rect) + 'static) -> Vec<([f32; 4], [f32; 4])> {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
    ui.add_leaf(Id::new("t"), Layout::leaf(Size::Grow(1.0), Size::Fixed(40.0)), Vec2::ZERO, false, paint);
    let out = ui.end_frame();
    let mut g: Vec<_> = out.draw.instances.iter().filter(|i| i.params[3] == glyph_kind()).map(|i| (i.rect, i.color)).collect();
    g.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
    g
}

#[test]
fn each_glyph_takes_its_span_s_colour() {
    let g = glyphs(|p, r| p.text_spans(Vec2::new(r.x, r.y), 14.0, Color::WHITE, "abc def gh", &[TextSpan::new(0..3, RED), TextSpan::new(4..7, GREEN)]));
    let colours: Vec<[f32; 4]> = g.iter().map(|t| t.1).collect();
    let (r, gr, w) = (RED.to_array(), GREEN.to_array(), Color::WHITE.to_array());
    assert_eq!(colours, vec![r, r, r, gr, gr, gr, w, w], "spans coloured the wrong glyphs");
}

/// A later span wins where two overlap, and spans need not be sorted.
#[test]
fn a_later_span_wins() {
    let g = glyphs(|p, r| p.text_spans(Vec2::new(r.x, r.y), 14.0, Color::WHITE, "abcd", &[TextSpan::new(2..4, GREEN), TextSpan::new(0..3, RED)]));
    let colours: Vec<[f32; 4]> = g.iter().map(|t| t.1).collect();
    assert_eq!(colours, vec![RED.to_array(), RED.to_array(), RED.to_array(), GREEN.to_array()]);
}

/// Colour never moves a glyph: the same text in spans and in one colour puts
/// every glyph in the same place — kerning across a span edge included
/// ("AV" kerns, and the edge is between them).
#[test]
fn colour_does_not_change_the_layout() {
    let text = "AVAV To Wa";
    let plain = glyphs(move |p, r| p.text(Vec2::new(r.x, r.y), 18.0, Color::WHITE, text));
    let spans = glyphs(move |p, r| p.text_spans(Vec2::new(r.x, r.y), 18.0, Color::WHITE, text, &[TextSpan::new(1..2, RED), TextSpan::new(5..7, GREEN)]));
    let at = |g: &[([f32; 4], [f32; 4])]| g.iter().map(|t| t.0).collect::<Vec<_>>();
    assert_eq!(at(&plain), at(&spans), "colouring moved glyphs");
}

struct Area {
    ui: Ui,
    text: String,
    opts: TextAreaOptions,
    asked: Vec<usize>,
}

impl Area {
    fn new(text: &str, wrap: bool) -> Self {
        Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), text: text.into(), opts: TextAreaOptions { rows: 6, wrap, ..Default::default() }, asked: Vec::new() }
    }

    /// One frame, highlighting every "fn" red and every "word" green; the
    /// glyphs drawn in each.
    fn frame(&mut self) -> (usize, usize) {
        self.asked.clear();
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(200.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
        let asked = &mut self.asked;
        self.ui.text_area_highlighted("code", &mut self.text, self.opts, |i, line, spans| {
            asked.push(i);
            for (k, _) in line.match_indices("fn") {
                spans.push(TextSpan::new(k..k + 2, RED));
            }
            for (k, _) in line.match_indices("word") {
                spans.push(TextSpan::new(k..k + 4, GREEN));
            }
        });
        let out = self.ui.end_frame();
        let n = |c: Color| out.draw.instances.iter().filter(|i| i.params[3] == glyph_kind() && i.color == c.to_array()).count();
        (n(RED), n(GREEN))
    }
}

#[test]
fn a_text_area_draws_its_highlighter_s_colours() {
    let mut a = Area::new("fn main\nlet fn_x = 1;\nplain", false);
    // The field has no size on its first frame; the second draws it all.
    a.frame();
    let (red, green) = a.frame();
    assert_eq!(red, 4, "two 'fn's, two glyphs each, should be red");
    assert_eq!(green, 0);
    assert_eq!(a.asked, vec![0, 1, 2], "each line on screen is asked once, by index");
}

/// The spans are in bytes of the whole line; a wrapped line's second row
/// starts partway in, and its colours must still land on the right glyphs.
#[test]
fn a_wrapped_row_keeps_its_line_s_colours() {
    let line = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii word";
    let mut a = Area::new(line, true);
    a.frame();
    let (_, green) = a.frame();
    assert_eq!(green, 4, "the last word, on a later row, was not coloured whole");
    assert_eq!(a.asked, vec![0], "a line of several rows was asked more than once");
    // And it did wrap, so the test means something.
    assert!(a.ui.fonts.measure(FontId(0), a.ui.theme.metrics.font_size, line).x > 200.0);
}

/// A long file asks only for the lines on screen.
#[test]
fn only_the_lines_on_screen_are_asked() {
    let doc: String = (0..10_000).map(|i| format!("fn f{i}() {{}}\n")).collect();
    let mut a = Area::new(&doc, false);
    a.frame();
    a.frame();
    assert!(!a.asked.is_empty() && a.asked.len() < 20, "asked for {} lines", a.asked.len());
}
