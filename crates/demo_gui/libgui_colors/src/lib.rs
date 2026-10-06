//! A demo of colour: the picker in each of its forms, gradients, and themes.
//!
//! - **Inline.** The whole picker in a panel, editing an accent colour — and,
//!   switched on, rebuilding the theme around it, so the window re-colours as
//!   you drag.
//! - **Swatches.** An inspector's layer rows, each a `color_button` that opens
//!   a picker in a popup: the form a CAD layer list uses.
//! - **Compact.** A picker with no alpha and no hex field, for a colour that is
//!   always drawn opaque.
//! - **Gradients.** What `Painter::gradient` draws: across and down, fading in
//!   over a checkerboard, a ramp between two picked colours, and a meter.
//! - **Recent.** Every finished edit, so `finished` is visible: one swatch per
//!   drag, not one per frame. Click one to go back to it.
//!
//! All of it is keyboard-reachable: Tab to any part of a picker, the arrows
//! adjust it (Shift for a big step), Space opens a swatch.

use libgui::*;

pub const THEMES: [&str; 3] = ["Dark", "Midnight", "Light"];

/// One gallery row's drawing.
type Paint = Box<dyn Fn(&mut Painter, Rect)>;

/// The layers an inspector shows, with their colours.
pub const LAYERS: [&str; 4] = ["Body", "Sketch", "Construction", "Dimensions"];

pub struct Colors {
    pub theme: usize,
    pub accent: Color,
    /// Rebuild the theme around `accent`.
    pub accent_theme: bool,
    pub layers: [Color; 4],
    pub compact: Color,
    /// Finished edits, newest first.
    pub recent: Vec<Color>,
    /// The theme last built, and what it was built from.
    built: Option<(usize, bool, [f32; 4])>,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            theme: 0,
            accent: Color::rgba(0.36, 0.55, 1.0, 1.0),
            accent_theme: false,
            layers: [
                Color::rgba(0.78, 0.80, 0.84, 1.0),
                Color::rgba(0.25, 0.75, 1.0, 1.0),
                Color::rgba(1.0, 0.62, 0.15, 0.6),
                Color::rgba(0.40, 0.90, 0.45, 1.0),
            ],
            compact: Color::rgba(0.85, 0.30, 0.45, 1.0),
            recent: Vec::new(),
            built: None,
        }
    }
}

/// The theme a demo frame is drawn in: a stock one, or one rebuilt with the
/// picked accent — which is all a custom theme is.
pub fn theme_for(index: usize, accent: Option<Color>) -> Theme {
    let mut palette = match index {
        1 => Palette::midnight(),
        2 => Palette::light(),
        _ => Palette::dark(),
    };
    let name = THEMES[index.min(2)];
    if let Some(a) = accent {
        let a = Color::rgba(a.r, a.g, a.b, 1.0);
        palette.accent = a;
        palette.accent_hover = a.lerp(Color::WHITE, 0.12);
        palette.accent_active = a.lerp(Color::BLACK, 0.12);
        palette.focus_ring = a.with_alpha(0.6);
    }
    Theme::new(name, palette, Density::Regular)
}

impl Colors {
    /// Remember a finished edit, newest first, without repeats.
    fn finished(&mut self, c: Color) {
        self.recent.retain(|r| r.to_array() != c.to_array());
        self.recent.insert(0, c);
        self.recent.truncate(12);
    }

    /// One frame of the whole demo.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Rebuilt only when what it depends on changed: a theme is a few
        // hundred colours, and this runs every frame.
        let key = (self.theme, self.accent_theme, if self.accent_theme { self.accent.to_array() } else { [0.0; 4] });
        if self.built != Some(key) {
            ui.theme = theme_for(self.theme, self.accent_theme.then_some(self.accent));
            self.built = Some(key);
        }
        let t = ui.theme.clone();
        let page = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(24.0)).gap(16.0);
        ui.container(page, Frame { fill: t.palette.bg_app, ..Frame::none() }, |ui| {
            ui.container(Layout::row().width(Size::Grow(1.0)).gap(16.0).align(Align::Start, Align::Center), Frame::none(), |ui| {
                ui.heading("Colour");
                ui.flex();
                ui.label_muted("Theme");
                let mut theme = self.theme;
                ui.segmented("theme", &mut theme, &THEMES);
                self.theme = theme;
            });
            ui.label_muted("Tab reaches every part of a picker; the arrows adjust it, Shift for a big step.");

            let columns = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)).gap(16.0);
            ui.container(columns, Frame::none(), |ui| {
                self.inline(ui, &t);
                self.swatches(ui, &t);
                self.gradients(ui, &t);
            });
            self.recent_row(ui);
        });
    }

    fn column(ui: &mut Ui, t: &Theme, id: &str, width: Size, body: impl FnOnce(&mut Ui)) {
        let layout = Layout::column().width(width).height(Size::Grow(1.0)).padding(Insets::all(16.0)).gap(12.0);
        ui.container_id(Id::new(id), layout, Frame::panel(t), body);
    }

    fn inline(&mut self, ui: &mut Ui, t: &Theme) {
        Self::column(ui, t, "col_inline", Size::Fixed(280.0), |ui| {
            ui.section("Inline");
            let r = ui.color_picker("accent", &mut self.accent);
            if r.finished {
                let c = self.accent;
                self.finished(c);
            }
            ui.toggle("Use as the theme's accent", &mut self.accent_theme);
            ui.button_primary("A primary button");
            let mut v = 0.6;
            ui.slider("Accent on a slider", &mut v, 0.0, 1.0);
        });
    }

    fn swatches(&mut self, ui: &mut Ui, t: &Theme) {
        Self::column(ui, t, "col_swatches", Size::Fixed(260.0), |ui| {
            ui.section("Swatches");
            ui.label_muted("Click one to open its picker.");
            for (i, name) in LAYERS.iter().enumerate() {
                let row = Layout::row().width(Size::Grow(1.0)).gap(8.0).align(Align::Start, Align::Center);
                let mut done = None;
                ui.container_id(Id::new(("layer", i)), row, Frame::none(), |ui| {
                    if ui.color_button(name, &mut self.layers[i]).finished {
                        done = Some(self.layers[i]);
                    }
                    ui.label(name);
                    ui.flex();
                    ui.label_muted(&self.layers[i].to_hex());
                });
                if let Some(c) = done {
                    self.finished(c);
                }
            }
            ui.separator();
            ui.section("Compact");
            ui.label_muted("No alpha strip, no hex field.");
            let opts = ColorPickerOptions { alpha: false, hex: false, square_height: 110.0 };
            if ui.color_picker_with("compact", &mut self.compact, opts).finished {
                let c = self.compact;
                self.finished(c);
            }
        });
    }

    fn gradients(&mut self, ui: &mut Ui, t: &Theme) {
        let (a, b, layer) = (self.accent, self.compact, self.layers[2]);
        let ink = t.palette.text_muted;
        let title = t.palette.text;
        let size = t.metrics.font_size * 0.9;
        Self::column(ui, t, "col_gradients", Size::Grow(1.0), |ui| {
            ui.section("Gradients");
            let row = |ui: &mut Ui, key: &str, h: f32, paint: Paint| {
                let id = ui.make_id(("gradient", key));
                ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(120.0, h), false, move |p, r| paint(p, r));
            };
            let border = t.palette.border;
            let opaque = |c: Color| Color::rgba(c.r, c.g, c.b, 1.0);

            ui.label_muted("Across, between two picked colours");
            row(ui, "across", 36.0, Box::new(move |p, r| {
                p.gradient(r, opaque(a), opaque(b), Axis::X);
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
            }));

            ui.label_muted("Down, white to the accent to black");
            row(ui, "down", 72.0, Box::new(move |p, r| {
                let top = Rect::new(r.x, r.y, r.w, (r.h * 0.5).round());
                let bottom = Rect::new(r.x, top.bottom(), r.w, r.h - top.h);
                p.gradient(top, Color::WHITE, opaque(a), Axis::Y);
                p.gradient(bottom, opaque(a), Color::BLACK, Axis::Y);
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
            }));

            ui.label_muted("Fading in over a checkerboard: the construction layer");
            row(ui, "fade", 36.0, Box::new(move |p, r| {
                let cell = 9.0;
                p.rect(r, Color::rgba(0.85, 0.85, 0.85, 1.0), 0.0);
                p.draw.push_clip(r);
                let cols = (r.w / cell).ceil() as i32;
                for j in 0..4 {
                    for i in 0..cols {
                        if (i + j) % 2 == 1 {
                            p.rect(Rect::new(r.x + i as f32 * cell, r.y + j as f32 * cell, cell, cell), Color::rgba(0.6, 0.6, 0.6, 1.0), 0.0);
                        }
                    }
                }
                p.draw.pop_clip();
                p.gradient(r, Color::TRANSPARENT, layer, Axis::X);
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
            }));

            ui.label_muted("A meter: three stops, cut where it is full");
            row(ui, "meter", 18.0, Box::new(move |p, r| {
                let stops = [Color::rgba(0.3, 0.8, 0.4, 1.0), Color::rgba(1.0, 0.8, 0.2, 1.0), Color::rgba(0.95, 0.3, 0.25, 1.0)];
                let full = 0.72;
                p.rect(r, Color::rgba(0.0, 0.0, 0.0, 0.25), 0.0);
                p.draw.push_clip(Rect::new(r.x, r.y, r.w * full, r.h));
                let half = Rect::new(r.x, r.y, (r.w * 0.5).round(), r.h);
                p.gradient(half, stops[0], stops[1], Axis::X);
                p.gradient(Rect::new(half.right(), r.y, r.w - half.w, r.h), stops[1], stops[2], Axis::X);
                p.draw.pop_clip();
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
            }));

            ui.label_muted("A card's header, washed with the accent");
            row(ui, "card", 90.0, Box::new(move |p, r| {
                let head = Rect::new(r.x, r.y, r.w, 34.0);
                p.rect(r, opaque(a).with_alpha(0.08), 0.0);
                p.gradient(head, opaque(a).with_alpha(0.0), opaque(a).with_alpha(0.55), Axis::X);
                // The theme's text colour, not white: a light theme's wash is
                // light too.
                p.text_left(head.shrink(10.0, 0.0, 0.0, 0.0), size, title, "Bracket_v3");
                p.text_left(Rect::new(r.x + 10.0, head.bottom() + 8.0, r.w, 18.0), size, ink, "Aluminium 6061 · 1.204 kg");
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
            }));
        });
    }

    fn recent_row(&mut self, ui: &mut Ui) {
        let row = Layout::row().width(Size::Grow(1.0)).gap(6.0).align(Align::Start, Align::Center);
        let mut pick = None;
        ui.container_id(Id::new("recent"), row, Frame::none(), |ui| {
            ui.label_muted("Recent");
            if self.recent.is_empty() {
                ui.label_muted("— finish an edit to keep it here");
            }
            for (i, c) in self.recent.iter().enumerate() {
                let id = ui.make_id(("recent", i));
                let r = ui.interact_focusable(id, FocusKind::Control);
                let hot = ui.animate_bool(id, 0, r.hovered || r.focused);
                let (c, border) = (*c, ui.theme.palette.border_strong);
                ui.add_leaf(id, Layout::leaf(Size::Fixed(24.0), Size::Fixed(24.0)), Vec2::ZERO, true, move |p, rect| {
                    p.rect(rect.shrink(2.0, 2.0, 2.0, 2.0), Color::rgba(c.r, c.g, c.b, 1.0), 4.0);
                    p.rect_bordered(rect, Color::TRANSPARENT, 5.0, 1.0 + hot, border);
                });
                if r.clicked {
                    pick = Some(c);
                }
            }
        });
        // A recent colour goes back to the inline picker.
        if let Some(c) = pick {
            self.accent = c;
        }
    }
}
