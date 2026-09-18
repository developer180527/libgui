use crate::Color;

/// Design tokens. Every widget reads from here, so polish is tuned in one
/// place. Swap or hot-reload a `Theme` at runtime to restyle everything.
#[derive(Clone, Debug)]
pub struct Theme {
    // Surfaces
    pub bg_app: Color,
    pub bg_panel: Color,
    pub bg_inset: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_active: Color,
    pub border: Color,
    pub border_strong: Color,
    pub shadow: Color,
    // Accent
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_active: Color,
    // Text
    pub text: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub text_on_accent: Color,
    // Metrics (logical px)
    pub radius: f32,
    pub radius_large: f32,
    pub space: f32,
    pub control_height: f32,
    pub row_height: f32,
    pub font_size: f32,
    pub font_size_small: f32,
    pub font_size_heading: f32,
    /// Exponential approach rate for hover/press animations (1/s).
    pub anim_speed: f32,
}

impl Theme {
    /// Neutral graphite dark: no hue tint, not true black.
    pub fn dark() -> Self {
        Self {
            bg_app: Color::hex(0x161618),
            bg_panel: Color::hex(0x1d1d20),
            bg_inset: Color::hex(0x131315),
            surface: Color::hex(0x2a2a2e),
            surface_hover: Color::hex(0x343439),
            surface_active: Color::hex(0x222225),
            border: Color::hex(0x2b2b2f),
            border_strong: Color::hex(0x404046),
            shadow: Color::rgba(0.0, 0.0, 0.0, 0.55),
            accent: Color::hex(0x4c8dff),
            accent_hover: Color::hex(0x66a0ff),
            accent_active: Color::hex(0x3d78e0),
            text: Color::hex(0xececee),
            text_muted: Color::hex(0xa3a3aa),
            text_faint: Color::hex(0x6c6c74),
            ..Self::midnight()
        }
    }

    /// Deep slate with a blue tint.
    pub fn midnight() -> Self {
        Self {
            bg_app: Color::hex(0x0e1016),
            bg_panel: Color::hex(0x151821),
            bg_inset: Color::hex(0x0b0d12),
            surface: Color::hex(0x232837),
            surface_hover: Color::hex(0x2c3345),
            surface_active: Color::hex(0x1b1f2a),
            border: Color::hex(0x262b39),
            border_strong: Color::hex(0x3a4256),
            shadow: Color::rgba(0.0, 0.0, 0.0, 0.6),
            accent: Color::hex(0x5b8def),
            accent_hover: Color::hex(0x6e9cff),
            accent_active: Color::hex(0x4a78d6),
            text: Color::hex(0xe6e9ef),
            text_muted: Color::hex(0x9aa1b2),
            text_faint: Color::hex(0x5f6679),
            text_on_accent: Color::hex(0xffffff),
            radius: 6.0,
            radius_large: 10.0,
            space: 8.0,
            control_height: 30.0,
            row_height: 28.0,
            font_size: 13.0,
            font_size_small: 11.0,
            font_size_heading: 15.0,
            anim_speed: 18.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}
