//! Everything the graph looks like. Derived from a [`Theme`] by default, then
//! yours to change: nothing here is baked into the drawing code.

use libgui::{Color, Theme};

/// How a link is routed between two ports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Routing {
    /// Cubic bezier with horizontal tangents: the usual "cable".
    #[default]
    Bezier,
    /// Right angles, like a schematic or Unreal's reroute style.
    Orthogonal,
    /// Straight from port to port.
    Straight,
}

/// Look of the graph. Sizes are in canvas units, so they scale with zoom.
#[derive(Clone, Copy, Debug)]
pub struct GraphStyle {
    // Nodes
    pub node_width: f32,
    pub node_fill: Color,
    pub node_border: Color,
    pub node_border_selected: Color,
    pub node_border_width: f32,
    pub node_radius: f32,
    pub node_shadow: bool,
    pub header_height: f32,
    pub header_fill: Color,
    /// Coloured stripe down the left of the header; 0 turns it off.
    pub header_stripe: f32,
    pub title: Color,
    pub title_size: f32,

    // Ports
    pub port_radius: f32,
    /// Vertical space each port row takes.
    pub port_row: f32,
    /// Invisible margin around a port, so a small dot is still easy to grab.
    pub port_grab: f32,
    pub port_fill: Color,
    pub port_fill_hover: Color,
    pub port_border: Color,
    pub port_label: Color,
    pub port_label_size: f32,

    // Links
    pub routing: Routing,
    pub link_width: f32,
    /// Lower bound on the drawn width in *window* pixels, so a wire does not
    /// vanish when you zoom out.
    pub link_min_px: f32,
    pub link: Color,
    pub link_hover: Color,
    pub link_selected: Color,
    /// A link being dragged that has nowhere valid to land.
    pub link_invalid: Color,
    /// How close the pointer must be to a link to hit it, in window px.
    pub link_grab_px: f32,

    // Dragging
    /// How close a port must be to catch a dragged link end, in window px.
    pub snap_px: f32,
    /// How fast a dragged end eases into a port it has caught (1/s).
    pub snap_speed: f32,

    // Background
    pub grid: Color,
    pub grid_strong: Color,
    /// Spacing of the finest grid, in canvas units. 0 turns the grid off.
    pub grid_step: f32,
}

impl GraphStyle {
    /// Derived from a theme, so a graph matches the rest of the app by default.
    pub fn from_theme(t: &Theme) -> Self {
        let p = &t.palette;
        Self {
            node_width: 190.0,
            node_fill: p.bg_panel,
            node_border: p.border_strong,
            node_border_selected: p.accent,
            node_border_width: 1.0,
            node_radius: t.metrics.radius_large,
            node_shadow: true,
            header_height: 26.0,
            header_fill: p.bg_inset,
            header_stripe: 3.0,
            title: p.text,
            title_size: t.metrics.font_size,

            port_radius: 4.5,
            port_row: 20.0,
            port_grab: 9.0,
            port_fill: p.bg_panel.lerp(p.text, 0.35),
            port_fill_hover: p.accent,
            port_border: p.border_strong,
            port_label: p.text_muted,
            port_label_size: t.metrics.font_size_small,

            routing: Routing::Bezier,
            link_width: 2.0,
            link_min_px: 1.5,
            link: p.accent,
            link_hover: p.accent.lerp(Color::WHITE, 0.35),
            link_selected: Color::WHITE,
            link_invalid: p.danger,
            link_grab_px: 7.0,

            snap_px: 20.0,
            snap_speed: 26.0,

            grid: p.border,
            grid_strong: p.border_strong,
            grid_step: 40.0,
        }
    }
}
