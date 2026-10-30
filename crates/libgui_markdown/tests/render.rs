//! Markdown through a real frame: what it draws, where, and what it reports.

use libgui::*;
use libgui_markdown::{Markdown, MarkdownOptions, MarkdownResponse};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

const DOC: &str = "# libgui\n\nAn **immediate-mode** UI library with a *C ABI*, `retained state` and [docs](https://example.com/docs).\n\n## Features\n\n- Layout and widgets\n- [x] Right-to-left text\n- [ ] Accessibility\n\n1. Build\n2. Run\n\n> Quoted text, muted, behind a bar.\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| Crate | Role |\n|:--|--:|\n| libgui | core |\n| libgui_c | C ABI |\n\n---\n\nThe end, ~~struck~~.";

struct Shot {
    ui: Ui,
}

fn faces(ui: &mut Ui) {
    // Real bold, italic and mono faces where this machine has them; the
    // document still draws without (they fall back to the body face).
    for (role, path, index) in [
        (FontRole::Bold, "/System/Library/Fonts/Supplemental/Arial Bold.ttf", 0),
        (FontRole::Italic, "/System/Library/Fonts/Supplemental/Arial Italic.ttf", 0),
        (FontRole::BoldItalic, "/System/Library/Fonts/Supplemental/Arial Bold Italic.ttf", 0),
        (FontRole::Mono, "/System/Library/Fonts/Menlo.ttc", 0),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(f) = FontdueRasterizer::from_collection(&bytes, index) {
                let id = ui.fonts.add_rasterizer(Box::new(f));
                ui.set_font_role(role, id);
            }
        }
    }
}

impl Shot {
    fn new() -> Self {
        Self { ui: Ui::new(Theme::dark(), FONT).expect("font") }
    }

    fn frame(&mut self, doc: &Markdown, w: f32, opts: &MarkdownOptions) -> (MarkdownResponse, FrameOutputSummary) {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, 1000.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = self.ui.container(Layout::column().padding(Insets::all(16.0)), Frame { fill: self.ui.theme.palette.bg_panel, ..Frame::none() }, |ui| doc.show_with(ui, "doc", opts));
        let out = self.ui.end_frame();
        let s = FrameOutputSummary::of(&out);
        (r, s)
    }
}

/// What a frame drew, kept past the frame.
struct FrameOutputSummary {
    glyphs: Vec<([f32; 4], [f32; 4])>,
    images: usize,
    bottom: f32,
}

impl FrameOutputSummary {
    fn of(out: &FrameOutput) -> Self {
        use render_contract::PrimitiveKind::*;
        let kind = |i: &Instance| render_contract::PrimitiveKind::from_code(i.params[3]);
        let glyphs: Vec<_> = out.draw.instances.iter().filter(|i| kind(i) == Some(Glyph)).map(|i| (i.rect, i.color)).collect();
        let bottom = glyphs.iter().map(|g| g.0[1] + g.0[3]).fold(0.0, f32::max);
        Self { glyphs, images: out.draw.instances.iter().filter(|i| kind(i) == Some(Image)).count(), bottom }
    }
}

#[test]
fn a_document_draws_and_reports_its_link() {
    let doc = Markdown::parse(DOC);
    let mut s = Shot::new();
    let opts = MarkdownOptions::default();
    s.frame(&doc, 600.0, &opts);
    let (_, out) = s.frame(&doc, 600.0, &opts);
    assert!(out.glyphs.len() > 200, "drew only {} glyphs", out.glyphs.len());
    // The link is drawn in the accent colour.
    let accent = s.ui.theme.palette.accent.to_array();
    let link: Vec<_> = out.glyphs.iter().filter(|g| g.1 == accent).collect();
    assert_eq!(link.len(), "docs".len(), "the link's glyphs are not in the link colour");
    // Hover, then click, the link where it was drawn.
    let at = Vec2::new(link[1].0[0] + 1.0, link[1].0[1] + 2.0);
    s.ui.push(InputEvent::PointerMoved { pos: at });
    let (hover, _) = s.frame(&doc, 600.0, &opts);
    assert_eq!(hover.hovered_link.as_deref(), Some("https://example.com/docs"));
    s.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    s.frame(&doc, 600.0, &opts);
    s.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let (click, _) = s.frame(&doc, 600.0, &opts);
    assert_eq!(click.clicked_link.as_deref(), Some("https://example.com/docs"));
}

/// Narrower is taller: paragraphs wrap, and what follows them moves down.
#[test]
fn a_narrow_window_wraps_and_grows() {
    let doc = Markdown::parse(DOC);
    let opts = MarkdownOptions::default();
    let height = |w: f32| {
        let mut s = Shot::new();
        s.frame(&doc, w, &opts);
        s.frame(&doc, w, &opts).1.bottom
    };
    let (wide, narrow) = (height(700.0), height(260.0));
    assert!(narrow >= wide + 12.0, "260 px wide ends at {narrow}, 700 px at {wide}: nothing wrapped");
}

/// A code block's lines are coloured by the app's highlighter, given the
/// fence's language.
#[test]
fn code_blocks_use_the_highlighter() {
    let doc = Markdown::parse("```rust\nfn main() {}\n```\n");
    let red = Color::hex(0xff0000);
    let seen = std::cell::RefCell::new(Vec::new());
    let hl = |lang: &str, line: &str, spans: &mut Vec<TextSpan>| {
        seen.borrow_mut().push((lang.to_string(), line.to_string()));
        if line.starts_with("fn") {
            spans.push(TextSpan::new(0..2, red));
        }
    };
    let opts = MarkdownOptions { highlight: Some(&hl), ..Default::default() };
    let mut s = Shot::new();
    s.frame(&doc, 500.0, &opts);
    let (_, out) = s.frame(&doc, 500.0, &opts);
    assert_eq!(out.glyphs.iter().filter(|g| g.1 == red.to_array()).count(), 2);
    assert_eq!(seen.borrow()[0], ("rust".to_string(), "fn main() {}".to_string()));
}

/// An image the app has loaded is drawn; one it has not is its alt text.
#[test]
fn images_are_the_apps() {
    let doc = Markdown::parse("![a cat](cat.png)\n");
    let loaded = |url: &str| (url == "cat.png").then_some((TextureId::User(5), Vec2::new(64.0, 48.0)));
    let mut s = Shot::new();
    let with = MarkdownOptions { image: Some(&loaded), ..Default::default() };
    s.frame(&doc, 500.0, &with);
    let (_, out) = s.frame(&doc, 500.0, &with);
    assert_eq!(out.images, 1);
    let (_, out) = s.frame(&doc, 500.0, &MarkdownOptions::default());
    assert_eq!(out.images, 0);
    assert_eq!(out.glyphs.len(), "acat".len(), "the alt text was not shown");
}

/// The theme's colours reach the text: a theme change redraws it in the new
/// colours, not the cached old ones.
#[test]
fn a_theme_change_recolours_the_text() {
    let doc = Markdown::parse("plain words");
    let mut s = Shot::new();
    let opts = MarkdownOptions::default();
    s.frame(&doc, 500.0, &opts);
    let dark = s.frame(&doc, 500.0, &opts).1.glyphs[0].1;
    s.ui.theme = Theme::light();
    s.frame(&doc, 500.0, &opts);
    let light = s.frame(&doc, 500.0, &opts).1.glyphs[0].1;
    assert_ne!(dark, light);
    assert_eq!(light, s.ui.theme.palette.text.to_array());
}

/// Renders the sample document to a PNG at `LIBGUI_MD_PREVIEW`, to look at.
#[test]
fn preview() {
    let Ok(path) = std::env::var("LIBGUI_MD_PREVIEW") else { return };
    let doc = Markdown::parse(DOC);
    let mut s = Shot::new();
    faces(&mut s.ui);
    let red = Color::hex(0xc678dd);
    let hl = |_: &str, line: &str, spans: &mut Vec<TextSpan>| {
        for kw in ["fn ", "println!"] {
            for (i, _) in line.match_indices(kw) {
                spans.push(TextSpan::new(i..i + kw.trim_end().len(), red));
            }
        }
    };
    let opts = MarkdownOptions { highlight: Some(&hl), ..Default::default() };
    let w: f32 = std::env::var("LIBGUI_MD_W").ok().and_then(|v| v.parse().ok()).unwrap_or(560.0);
    let (h, scale) = (900.0f32, 2.0f32);
    let mut out_img = None;
    for _ in 0..3 {
        s.ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, h), scale, dt: 1.0 / 60.0 });
        s.ui.container(Layout::column().padding(Insets::all(20.0)), Frame { fill: s.ui.theme.palette.bg_panel, ..Frame::none() }, |ui| doc.show_with(ui, "doc", &opts));
        let out = s.ui.end_frame();
        out_img = Some(libgui_soft::SoftRenderer::new().render_to_image(&out, (w * scale) as u32, (h * scale) as u32));
    }
    let t = out_img.unwrap();
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), t.width, t.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&t.data).unwrap();
}
