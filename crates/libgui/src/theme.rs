//! Look, as data.
//!
//! A [`Theme`] has three layers:
//! 1. [`Palette`]: named colours (`accent`, `bg_panel`, …).
//! 2. [`Metrics`]: sizes and fonts, generated from a [`Density`] preset.
//! 3. Per-widget styles ([`ButtonStyle`], [`TabStyle`], …), *derived* from the
//!    palette and metrics by [`Theme::from_parts`] and then individually overridable.
//!
//! Change a palette colour and every style that uses it follows; change one
//! style field and only that widget changes. Themes load and hot-reload from
//! TOML (`theme_file` module), and any subtree can be restyled with
//! [`crate::Ui::with_style`]. Behaviour ("feel") lives elsewhere, e.g. `DockConfig`.

use crate::{Color, Insets};

#[cfg(feature = "theme-toml")]
macro_rules! serde_struct {
    ($item:item) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        $item
    };
}
#[cfg(not(feature = "theme-toml"))]
macro_rules! serde_struct {
    ($item:item) => {
        $item
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "theme-toml", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "lowercase"))]
pub enum Density {
    /// Dense desktop tools (Unity, Houdini, Blender-like).
    Compact,
    #[default]
    Regular,
    /// Tablets and touch screens: ~44 px targets.
    Touch,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Regular, Density::Touch];
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg_app: Color,
    pub bg_panel: Color,
    pub bg_inset: Color,
    pub surface: Color,
    pub surface_hover: Color,
    pub surface_active: Color,
    pub border: Color,
    pub border_strong: Color,
    pub shadow: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_active: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub text_on_accent: Color,
    pub knob: Color,
    pub danger: Color,
    pub warning: Color,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub radius: f32,
    pub radius_large: f32,
    /// Base spacing unit.
    pub space: f32,
    pub control_height: f32,
    pub row_height: f32,
    pub tab_height: f32,
    /// One level of tree indentation, and the width of a disclosure arrow.
    pub indent: f32,
    pub font_size: f32,
    pub font_size_small: f32,
    pub font_size_heading: f32,
    /// Exponential approach rate for hover/press animations (1/s).
    pub anim_speed: f32,
}
}

impl Metrics {
    pub fn for_density(d: Density) -> Self {
        match d {
            Density::Compact => Self {
                radius: 4.0,
                radius_large: 7.0,
                space: 6.0,
                control_height: 24.0,
                row_height: 22.0,
                tab_height: 26.0,
                indent: 12.0,
                font_size: 12.0,
                font_size_small: 10.5,
                font_size_heading: 13.5,
                anim_speed: 20.0,
            },
            Density::Regular => Self {
                radius: 6.0,
                radius_large: 10.0,
                space: 8.0,
                control_height: 30.0,
                row_height: 28.0,
                tab_height: 30.0,
                indent: 14.0,
                font_size: 13.0,
                font_size_small: 11.0,
                font_size_heading: 15.0,
                anim_speed: 18.0,
            },
            Density::Touch => Self {
                radius: 10.0,
                radius_large: 14.0,
                space: 12.0,
                control_height: 44.0,
                row_height: 44.0,
                tab_height: 44.0,
                indent: 20.0,
                font_size: 16.0,
                font_size_small: 13.0,
                font_size_heading: 19.0,
                anim_speed: 16.0,
            },
        }
    }

    /// Size factor relative to Regular, for derived sizes (knobs, tracks…).
    fn k(&self) -> f32 {
        self.control_height / 30.0
    }
}

serde_struct! {
/// A colour per interaction state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateColors {
    pub normal: Color,
    pub hover: Color,
    pub active: Color,
}
}

impl StateColors {
    pub const fn new(normal: Color, hover: Color, active: Color) -> Self {
        Self { normal, hover, active }
    }

    pub const fn same(c: Color) -> Self {
        Self::new(c, c, c)
    }

    /// Blend by animated hover (0..1) and press (0..1) amounts.
    pub fn at(&self, hover: f32, active: f32) -> Color {
        self.normal.lerp(self.hover, hover).lerp(self.active, active)
    }
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub offset_y: f32,
    pub blur: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    pub fill: StateColors,
    pub border: StateColors,
    pub text: StateColors,
    pub radius: f32,
    pub border_width: f32,
    pub padding_x: f32,
    pub height: f32,
    pub shadow: Shadow,
    /// Alpha of the 1px top highlight (0 = off).
    pub highlight: f32,
    /// Label offset while pressed (px).
    pub press_offset: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToggleStyle {
    pub track_off: Color,
    pub track_on: Color,
    pub border_off: Color,
    pub border_on: Color,
    pub knob: Color,
    pub label: Color,
    pub width: f32,
    pub height: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderStyle {
    pub track: Color,
    pub fill: Color,
    pub knob: Color,
    /// Hover/drag halo around the knob.
    pub ring: Color,
    pub label: Color,
    pub value: Color,
    pub value_active: Color,
    pub track_height: f32,
    pub knob_radius: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectableStyle {
    pub fill_hover: Color,
    pub fill_selected: Color,
    pub indicator: Color,
    pub text: Color,
    pub text_hover: Color,
    pub text_selected: Color,
    pub radius: f32,
    pub indicator_width: f32,
    pub height: f32,
    pub padding_x: f32,
}
}

serde_struct! {
/// Menus, popups and context menus, and their items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuStyle {
    pub fill: Color,
    pub border: Color,
    pub radius: f32,
    /// Inset of the items from the panel edge.
    pub padding: Insets,
    /// Space between items.
    pub gap: f32,
    pub item_height: f32,
    pub item_padding_x: f32,
    pub item_radius: f32,
    pub item_fill_hover: Color,
    pub text: Color,
    pub text_hover: Color,
    /// Items that cannot be chosen right now.
    pub text_disabled: Color,
    /// The shortcut printed on the right of an item.
    pub shortcut: Color,
    pub separator: Color,
    /// Space a separator takes, including the line.
    pub separator_height: f32,
    /// Width reserved on the left for a check mark or submenu state.
    pub gutter: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipStyle {
    pub fill: Color,
    pub border: Color,
    pub text: Color,
    pub radius: f32,
    pub padding: Insets,
    /// Seconds of hover before it appears.
    pub delay: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextInputStyle {
    pub fill: Color,
    pub border: Color,
    pub border_hover: Color,
    pub border_focus: Color,
    pub focus_ring: Color,
    pub text: Color,
    pub placeholder: Color,
    pub selection: Color,
    pub caret: Color,
    pub radius: f32,
    pub padding_x: f32,
    pub height: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentedStyle {
    pub fill: Color,
    pub border: Color,
    pub fill_selected: Color,
    pub text: Color,
    pub text_selected: Color,
    pub radius: f32,
    pub height: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarStyle {
    pub thumb: Color,
    pub thumb_hover: Color,
    pub width: f32,
    pub width_hover: f32,
    /// Thumb opacity while the pointer is elsewhere (0 = hidden when idle).
    pub rest_alpha: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStyle {
    pub height: f32,
    pub padding_x: f32,
    pub min_width: f32,
    pub radius: f32,
    pub gap: f32,
    pub bar_fill: Color,
    pub fill_hover: Color,
    pub fill_active: Color,
    pub text: Color,
    pub text_active: Color,
    /// Indicator on the active tab of the focused pane.
    pub accent: Color,
    pub accent_height: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitterStyle {
    /// Visible gap between panes.
    pub size: f32,
    pub line_hover: Color,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelStyle {
    pub fill: Color,
    pub border: Color,
    pub radius: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotStyle {
    pub fill: Color,
    pub border: Color,
    pub bar: Color,
    pub bar_high: Color,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportStyle {
    pub radius: f32,
    pub border: Color,
    pub border_hover: Color,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableStyle {
    pub header_height: f32,
    pub header_fill: Color,
    pub header_text: Color,
    /// Header under the pointer, and the sorted column's header.
    pub header_text_active: Color,
    pub row_height: f32,
    /// Every other row, for tracking a value across a wide table. Transparent
    /// turns striping off.
    pub row_fill_alt: Color,
    pub row_fill_hover: Color,
    pub row_fill_selected: Color,
    pub text: Color,
    pub text_selected: Color,
    /// Vertical rules between columns, and the line under the header.
    pub grid: Color,
    /// Width of the drag zone on a column edge, each side.
    pub resize_grip: f32,
    pub resize_hover: Color,
    pub cell_padding_x: f32,
}
}

serde_struct! {
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropPreviewStyle {
    pub fill: Color,
    pub border: Color,
    pub border_width: f32,
    pub radius: f32,
}
}

serde_struct! {
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub density: Density,
    pub palette: Palette,
    pub metrics: Metrics,
    pub button: ButtonStyle,
    pub button_primary: ButtonStyle,
    pub toggle: ToggleStyle,
    pub slider: SliderStyle,
    pub selectable: SelectableStyle,
    pub menu: MenuStyle,
    pub tooltip: TooltipStyle,
    pub text_input: TextInputStyle,
    pub segmented: SegmentedStyle,
    pub scrollbar: ScrollbarStyle,
    pub tab: TabStyle,
    pub splitter: SplitterStyle,
    pub panel: PanelStyle,
    pub plot: PlotStyle,
    pub viewport: ViewportStyle,
    pub table: TableStyle,
    pub drop_preview: DropPreviewStyle,
}
}

impl Theme {
    /// Palette + density; all metrics and styles derived.
    pub fn new(name: &str, palette: Palette, density: Density) -> Self {
        Self::from_parts(name, palette, density, Metrics::for_density(density))
    }

    /// The one place styles are derived from palette + metrics.
    pub fn from_parts(name: &str, palette: Palette, density: Density, metrics: Metrics) -> Self {
        let (p, m) = (&palette, &metrics);
        let k = m.k();
        let white = Color::WHITE;
        let shadow = Shadow { color: p.shadow.with_alpha(p.shadow.a * 0.75), offset_y: 1.5, blur: 3.0 };
        Self {
            name: name.to_string(),
            density,
            button: ButtonStyle {
                fill: StateColors::new(p.surface, p.surface_hover, p.surface_active),
                border: StateColors::new(p.border, p.border_strong, p.border),
                text: StateColors::same(p.text),
                radius: m.radius,
                border_width: 1.0,
                padding_x: m.space * 1.75,
                height: m.control_height,
                shadow,
                highlight: 0.05,
                press_offset: 0.5,
            },
            button_primary: ButtonStyle {
                fill: StateColors::new(p.accent, p.accent_hover, p.accent_active),
                border: StateColors::new(p.accent.lerp(white, 0.12), p.accent_hover.lerp(white, 0.12), p.accent_active),
                text: StateColors::same(p.text_on_accent),
                radius: m.radius,
                border_width: 1.0,
                padding_x: m.space * 1.75,
                height: m.control_height,
                shadow,
                highlight: 0.08,
                press_offset: 0.5,
            },
            toggle: ToggleStyle {
                track_off: p.bg_inset,
                track_on: p.accent,
                border_off: p.border_strong,
                border_on: p.accent_hover,
                knob: p.knob,
                label: p.text,
                width: 34.0 * k,
                height: 18.0 * k,
            },
            slider: SliderStyle {
                track: p.bg_inset,
                fill: p.accent,
                knob: p.knob,
                ring: p.accent.with_alpha(0.18),
                label: p.text_muted,
                value: p.text,
                value_active: p.accent_hover,
                track_height: 4.0 * k,
                knob_radius: 7.0 * k,
            },
            table: TableStyle {
                header_height: (m.row_height * 1.2).round(),
                header_fill: p.bg_panel,
                header_text: p.text_muted,
                header_text_active: p.text,
                row_height: m.row_height,
                row_fill_alt: p.surface.with_alpha(0.35),
                row_fill_hover: p.surface.with_alpha(0.6),
                row_fill_selected: p.accent.with_alpha(0.16),
                text: p.text,
                text_selected: p.text,
                grid: p.border,
                resize_grip: 4.0,
                resize_hover: p.accent,
                cell_padding_x: m.space,
            },
            selectable: SelectableStyle {
                fill_hover: p.surface.with_alpha(0.6),
                fill_selected: p.accent.with_alpha(0.16),
                indicator: p.accent,
                text: p.text_muted,
                text_hover: p.text,
                text_selected: p.text,
                radius: m.radius,
                indicator_width: 3.0,
                height: m.row_height,
                padding_x: m.space * 1.25,
            },
            menu: MenuStyle {
                fill: p.bg_panel,
                border: p.border_strong,
                radius: m.radius_large,
                padding: Insets::all(m.space * 0.5),
                gap: 1.0,
                item_height: m.row_height,
                item_padding_x: m.space,
                item_radius: m.radius,
                item_fill_hover: p.accent.with_alpha(0.18),
                text: p.text,
                text_hover: p.text,
                text_disabled: p.text_faint,
                shortcut: p.text_faint,
                separator: p.border,
                separator_height: m.space,
                gutter: m.space * 1.5,
            },
            tooltip: TooltipStyle {
                fill: p.bg_app.lerp(p.text, 0.08),
                border: p.border_strong,
                text: p.text,
                radius: m.radius,
                padding: Insets::xy(m.space * 0.75, m.space * 0.4),
                delay: 0.5,
            },
            text_input: TextInputStyle {
                fill: p.bg_inset,
                border: p.border_strong,
                border_hover: p.border_strong.lerp(p.text_faint, 0.5),
                border_focus: p.accent,
                focus_ring: p.accent.with_alpha(0.28),
                text: p.text,
                placeholder: p.text_faint,
                selection: p.accent.with_alpha(0.35),
                caret: p.accent_hover,
                radius: m.radius,
                padding_x: m.space * 1.25,
                height: m.control_height,
            },
            segmented: SegmentedStyle {
                fill: p.bg_inset,
                border: p.border,
                fill_selected: p.surface_hover,
                text: p.text_muted,
                text_selected: p.text,
                radius: m.radius,
                height: m.control_height,
            },
            scrollbar: ScrollbarStyle {
                thumb: p.text_faint,
                thumb_hover: p.text_muted,
                width: 4.0 * k,
                width_hover: 7.0 * k,
                rest_alpha: 0.25,
            },
            tab: TabStyle {
                height: m.tab_height,
                padding_x: m.space * 1.75,
                min_width: 72.0 * k,
                radius: m.radius,
                gap: 2.0,
                bar_fill: Color::TRANSPARENT,
                fill_hover: p.surface.with_alpha(0.55),
                fill_active: p.bg_panel,
                text: p.text_muted,
                text_active: p.text,
                accent: p.accent,
                accent_height: 2.0,
            },
            splitter: SplitterStyle { size: 3.0, line_hover: p.accent.with_alpha(0.85) },
            panel: PanelStyle { fill: p.bg_panel, border: p.border, radius: 0.0 },
            plot: PlotStyle { fill: p.bg_inset, border: p.border, bar: p.accent, bar_high: p.warning },
            viewport: ViewportStyle { radius: m.radius_large, border: p.border, border_hover: p.border_strong },
            drop_preview: DropPreviewStyle {
                fill: p.accent.with_alpha(0.2),
                border: p.accent.with_alpha(0.9),
                border_width: 2.0,
                radius: m.radius,
            },
            palette,
            metrics,
        }
    }

    /// Re-derive every widget style from the current palette and metrics,
    /// discarding per-style overrides.
    pub fn rederive(&mut self) {
        *self = Self::from_parts(&self.name, self.palette, self.density, self.metrics);
    }

    /// Switch density: metrics regenerated, styles re-derived.
    pub fn set_density(&mut self, density: Density) {
        self.density = density;
        self.metrics = Metrics::for_density(density);
        self.rederive();
    }

    /// Neutral graphite dark: no hue tint, not true black.
    pub fn dark() -> Self {
        Self::new("Dark", Palette::dark(), Density::Regular)
    }

    /// Deep slate with a blue tint.
    pub fn midnight() -> Self {
        Self::new("Midnight", Palette::midnight(), Density::Regular)
    }

    /// Light, paper-like.
    pub fn light() -> Self {
        Self::new("Light", Palette::light(), Density::Regular)
    }

    /// Built-in presets by name (case-insensitive), used by `extends = "…"`.
    pub fn preset(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "dark" => Some(Self::dark()),
            "midnight" => Some(Self::midnight()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Palette {
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
            text_on_accent: Color::hex(0xffffff),
            knob: Color::hex(0xf2f4f8),
            danger: Color::hex(0xf87171),
            warning: Color::hex(0xf59e0b),
        }
    }

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
            knob: Color::hex(0xf2f4f8),
            danger: Color::hex(0xf87171),
            warning: Color::hex(0xf59e0b),
        }
    }

    pub fn light() -> Self {
        Self {
            bg_app: Color::hex(0xe4e4e8),
            bg_panel: Color::hex(0xf6f6f7),
            bg_inset: Color::hex(0xffffff),
            surface: Color::hex(0xffffff),
            surface_hover: Color::hex(0xf0f0f3),
            surface_active: Color::hex(0xe2e2e7),
            border: Color::hex(0xdadadf),
            border_strong: Color::hex(0xbdbdc6),
            shadow: Color::rgba(0.0, 0.0, 0.0, 0.14),
            accent: Color::hex(0x2f6fed),
            accent_hover: Color::hex(0x4580f5),
            accent_active: Color::hex(0x255fd1),
            text: Color::hex(0x1c1c21),
            text_muted: Color::hex(0x5b5b66),
            text_faint: Color::hex(0x9696a0),
            text_on_accent: Color::hex(0xffffff),
            knob: Color::hex(0xffffff),
            danger: Color::hex(0xdc2626),
            warning: Color::hex(0xd97706),
        }
    }
}
