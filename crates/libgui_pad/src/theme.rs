//! Two looks for the same app: dark chrome and light chrome.
//!
//! The page stays white in both, which is what a word processor does in its
//! dark mode — the document is paper, the tool around it is not.
//!
//! As with the other demos, nothing here is machinery: it is [`Theme`] with
//! different numbers, and the app owns them.

use libgui::*;

/// Colours the page is drawn in. Not part of [`Theme`], because a page is this
/// app's idea, not the library's.
pub struct Paper {
    pub sheet: Color,
    pub ink: Color,
    pub ink_faint: Color,
    pub selection: Color,
    /// The ruler over the page: the strip the text column sits on, the darker
    /// ends that stand for the margins, and the ticks.
    pub ruler: Color,
    pub ruler_margin: Color,
    pub rule: Color,
}

pub const PAGE: Paper = Paper {
    sheet: Color::WHITE,
    ink: Color::hex(0x17171a),
    ink_faint: Color::hex(0x9e9ea6),
    selection: Color::hex(0x2f72d4).with_alpha(0.26),
    ruler: Color::hex(0xcfcfd6),
    ruler_margin: Color::hex(0x9a9aa4),
    rule: Color::hex(0x6e6e78),
};

/// `paper` swaps the chrome to light; the sheet does not change.
pub fn theme(paper: bool) -> Theme {
    let mut t = if paper { Theme::light() } else { Theme::dark() };
    t.name = if paper { "Pad Light".into() } else { "Pad".into() };

    let p = &mut t.palette;
    if paper {
        p.bg_app = Color::hex(0xdedee1);
        p.bg_panel = Color::hex(0xf3f3f5);
        p.bg_inset = Color::hex(0xe7e7ea);
        p.surface = Color::hex(0xfbfbfd);
        p.border = Color::hex(0xcccccf);
        p.border_strong = Color::hex(0xb4b4b8);
        p.text = Color::hex(0x24242a);
        p.text_muted = Color::hex(0x5c5c66);
        p.text_faint = Color::hex(0x8b8b95);
    } else {
        p.bg_app = Color::hex(0x1b1b1d);
        p.bg_panel = Color::hex(0x252527);
        p.bg_inset = Color::hex(0x1b1b1d);
        p.surface = Color::hex(0x323235);
        p.border = Color::hex(0x141416);
        p.border_strong = Color::hex(0x3e3e42);
        p.text = Color::hex(0xdcdcdf);
        p.text_muted = Color::hex(0x9d9da5);
        p.text_faint = Color::hex(0x72727a);
    }
    // One accent for both, the blue a document tool marks the current thing in.
    p.accent = Color::hex(0x2f72d4);
    p.accent_hover = Color::hex(0x3d82e6);
    p.accent_active = Color::hex(0x2763b9);
    p.focus_ring = Color::hex(0x2f72d4);
    p.text_on_accent = Color::hex(0xffffff);

    let m = &mut t.metrics;
    m.font_size = 12.0;
    m.font_size_small = 11.0;
    m.font_size_heading = 14.0;
    m.radius = 4.0;
    m.radius_large = 6.0;
    m.space = 6.0;
    m.control_height = 24.0;
    m.row_height = 20.0;

    let (pal, met, d) = (t.palette, t.metrics, t.density);
    let mut t = Theme::from_parts(if paper { "Pad Light" } else { "Pad" }, pal, d, met);
    t.button.height = 24.0;
    t.button.padding_x = 10.0;
    t.button.shadow.color = Color::TRANSPARENT;
    t.button_primary.shadow.color = Color::TRANSPARENT;
    t.panel.radius = 0.0;
    t.text_input.height = 24.0;
    t.selectable.height = 22.0;
    t.selectable.radius = 4.0;
    t.scrollbar.width = 8.0;
    t
}
