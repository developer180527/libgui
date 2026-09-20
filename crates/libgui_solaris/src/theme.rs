//! The dark, dense look a 3D tool wants: near-black panels, thin borders, a
//! small type size, and an amber accent that only marks what is selected.
//!
//! Nothing here is new machinery — it is [`Theme`] with different numbers,
//! which is the point. A tool's whole visual identity is a value the app owns.

use libgui::*;

pub fn theme() -> Theme {
    let mut t = Theme::dark();
    t.name = "Solaris".into();

    let p = &mut t.palette;
    p.bg_app = Color::hex(0x151515);
    p.bg_panel = Color::hex(0x333333);
    p.bg_inset = Color::hex(0x1a1a1a);
    p.surface = Color::hex(0x3a3a3a);
    p.border = Color::hex(0x141414);
    p.border_strong = Color::hex(0x4a4a4a);
    p.accent = Color::hex(0xd8892c);
    p.accent_hover = Color::hex(0xe89a3c);
    p.accent_active = Color::hex(0xc27a20);
    p.focus_ring = Color::hex(0xd8892c);
    p.text = Color::hex(0xc9c9c9);
    p.text_muted = Color::hex(0x9a9a9a);
    p.text_faint = Color::hex(0x6d6d6d);
    p.text_on_accent = Color::hex(0x1a1a1a);

    let m = &mut t.metrics;
    m.font_size = 11.0;
    m.font_size_small = 10.0;
    m.font_size_heading = 13.0;
    m.radius = 2.0;
    m.radius_large = 3.0;
    m.space = 5.0;
    m.control_height = 18.0;
    m.row_height = 15.0;
    m.tab_height = 20.0;
    m.indent = 11.0;
    m.focus_ring_width = 1.0;

    // Rebuild the per-widget styles from the new palette and metrics, then
    // flatten the ones a 3D tool draws flatter than a document editor does.
    let (pal, met, d) = (t.palette, t.metrics, t.density);
    let mut t = Theme::from_parts("Solaris", pal, d, met);
    t.button.height = 18.0;
    t.button.radius = 2.0;
    t.button.shadow.color = Color::TRANSPARENT;
    t.button_primary.shadow.color = Color::TRANSPARENT;
    t.selectable.height = 15.0;
    t.selectable.radius = 0.0;
    t.selectable.indicator_width = 0.0;
    t.text_input.height = 17.0;
    t.text_input.radius = 2.0;
    t.panel.radius = 0.0;
    t.panel.border = Color::hex(0x141414);
    t.tab.height = 19.0;
    t.tab.radius = 0.0;
    t.tab.accent_height = 2.0;
    t.scrollbar.width = 6.0;
    t.scrollbar.width_hover = 9.0;
    t.table.header_height = 17.0;
    t.table.row_height = 14.0;
    t.table.row_fill_alt = Color::TRANSPARENT;
    t.table.grid = Color::hex(0x242424);
    t.table.cell_padding_x = 4.0;
    t
}
