//! A font stack that fetches faces it lacks (system fonts, through
//! `libgui_sysfonts`): asked once per character, kept, never re-asked.

use libgui::*;
use std::cell::RefCell;
use std::rc::Rc;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// A face that draws only `日`, as a 5-wide box.
struct Kanji;

impl FontRasterizer for Kanji {
    fn line_metrics(&self, px: f32) -> LineMetrics {
        LineMetrics { ascent: px * 0.8, descent: -px * 0.2 }
    }
    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        for (b, ch) in text.char_indices() {
            out.push(ShapedGlyph { glyph: ch as u32, face: 0, cluster: b as u32, advance: px, offset: Vec2::ZERO });
        }
    }
    fn rasterize(&self, _face: u16, glyph: u32, _px: f32) -> GlyphBitmap {
        let w = if glyph == '日' as u32 { 5 } else { 0 };
        GlyphBitmap { width: w, height: if w > 0 { 5 } else { 0 }, left: 0.0, bottom: 0.0, coverage: vec![255; (w * 5) as usize] }
    }
    fn covers(&self, ch: char) -> bool {
        ch == '日'
    }
}

/// Has a face for `日` only, and records what it was asked.
struct Source(Rc<RefCell<Vec<char>>>);

impl FontSource for Source {
    fn face_for(&self, ch: char) -> Option<Box<dyn FontRasterizer>> {
        self.0.borrow_mut().push(ch);
        (ch == '日').then(|| Box::new(Kanji) as Box<dyn FontRasterizer>)
    }
}

/// The glyph widths drawn for `text`, two frames in a row.
fn draw(ui: &mut Ui, text: &'static str) -> Vec<f32> {
    let mut widths = Vec::new();
    for _ in 0..2 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 60.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.add_leaf(Id::new("t"), Layout::leaf(Size::Grow(1.0), Size::Fixed(30.0)), Vec2::ZERO, false, move |p, r| {
            p.text(Vec2::new(r.x, r.y), 14.0, Color::WHITE, text);
        });
        let out = ui.end_frame();
        widths = out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).map(|i| i.rect[2]).collect();
    }
    widths
}

#[test]
fn a_missing_character_is_fetched_once_and_drawn() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let stack = FontStack::from_fonts(&[FONT]).unwrap().with_source(Box::new(Source(asked.clone())));
    let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(stack));
    let widths = draw(&mut ui, "a日b");
    assert!(widths.contains(&5.0), "the fetched face did not draw 日: {widths:?}");
    assert_eq!(widths.len(), 3, "the Latin letters stopped being drawn");
    // 日 was asked for once, though shaped, measured and drawn over two frames.
    assert_eq!(*asked.borrow(), vec!['日']);
    // Text the chain already covers never asks.
    draw(&mut ui, "plain words 日日");
    assert_eq!(*asked.borrow(), vec!['日'], "covered text consulted the source");
}

#[test]
fn a_character_no_one_has_is_asked_about_once() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let stack = FontStack::from_fonts(&[FONT]).unwrap().with_source(Box::new(Source(asked.clone())));
    let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(stack));
    draw(&mut ui, "\u{10FFFD}x");
    draw(&mut ui, "y\u{10FFFD}");
    assert_eq!(*asked.borrow(), vec!['\u{10FFFD}'], "a miss was asked about again");
}

/// The stack keeps what it was given: its source's faces join the end, so
/// glyphs already shaped keep the faces they name.
#[test]
fn fetched_faces_join_the_end() {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let stack = FontStack::from_fonts(&[FONT]).unwrap().with_source(Box::new(Source(asked)));
    assert_eq!(stack.faces(), 1);
    let mut out = Vec::new();
    stack.shape("a日", 14.0, &mut out);
    assert_eq!(stack.faces(), 2);
    assert_eq!(out.iter().map(|g| g.face).collect::<Vec<_>>(), vec![0, 1]);
}
