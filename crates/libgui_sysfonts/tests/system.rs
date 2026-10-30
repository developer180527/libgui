//! Against the fonts actually installed. A machine with no font for a script
//! (a bare CI image) skips that script rather than failing: what is checked
//! is that whatever the system has is found and drawn.

use libgui::*;
use libgui_sysfonts::SystemFonts;
use std::time::Instant;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// The glyph rects drawn for `text` (x, w, h), settled over two frames.
fn glyphs(ui: &mut Ui, text: &'static str) -> Vec<[i32; 3]> {
    let mut g = Vec::new();
    for _ in 0..2 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(600.0, 60.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.add_leaf(Id::new("t"), Layout::leaf(Size::Grow(1.0), Size::Fixed(30.0)), Vec2::ZERO, false, move |p, r| {
            p.text(Vec2::new(r.x, r.y), 14.0, Color::WHITE, text);
        });
        let out = ui.end_frame();
        g = out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).map(|i| [i.rect[0] as i32, i.rect[2] as i32, i.rect[3] as i32]).collect();
    }
    g
}

#[test]
fn a_chinese_filename_is_drawn_from_a_system_font() {
    let sys = SystemFonts::new();
    if sys.is_empty() {
        eprintln!("no system fonts here; skipped");
        return;
    }
    // Inter alone draws the Latin part and nothing for the Han.
    let mut plain = Ui::new(Theme::dark(), FONT).expect("font");
    let han = "报告";
    let boxes = glyphs(&mut plain, han);

    let t = Instant::now();
    let stack = FontStack::from_fonts(&[FONT]).unwrap().with_source(Box::new(sys));
    let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(stack));
    let drawn = glyphs(&mut ui, han);
    eprintln!("first Chinese text, font found and loaded: {:?}", t.elapsed());
    eprintln!("Inter alone: {boxes:?}; with system fonts: {drawn:?}");
    if SystemFonts::new().face_for('报').is_none() {
        eprintln!("no installed font has Han characters; skipped");
        return;
    }
    // Two real characters, not Inter's stand-in for one it does not have.
    assert_eq!(drawn.len(), 2, "both Han characters should be drawn");
    assert_ne!(drawn, boxes, "the Han characters are still Inter's .notdef");
    // The Latin around it is still Inter's.
    assert_eq!(glyphs(&mut ui, "2026.pdf"), glyphs(&mut plain, "2026.pdf"));
    // Once found, more Chinese costs no search.
    let t = Instant::now();
    glyphs(&mut ui, "文件 设置 帮助");
    eprintln!("more Chinese after that: {:?}", t.elapsed());
}

#[test]
fn every_common_script_finds_a_face_where_one_is_installed() {
    let sys = SystemFonts::new();
    for (name, ch) in [("Chinese", '文'), ("Japanese", 'か'), ("Korean", '한'), ("Arabic", 'ع'), ("Hebrew", 'א'), ("Hindi", 'क'), ("Thai", 'ก')] {
        let t = Instant::now();
        let found = sys.face_for(ch);
        eprintln!("{name:9} {}: {:?}", if found.is_some() { "found" } else { "none " }, t.elapsed());
        if let Some(face) = found {
            assert!(face.covers(ch), "{name}: the face found does not draw {ch}");
        }
    }
}
