//! A second demo, shaped like a 3D application's editor: a menu bar, a shelf,
//! a viewport, a parameter panel, a node network, a scene graph tree, a
//! details table and a timeline, all on screen at once.
//!
//! The first demo shows the features one at a time. This one shows what
//! happens when a real tool's worth of UI is on screen together — which is the
//! only way to find out whether the density, the type size, the splitters and
//! the frame budget actually hold up.

use libgui::*;

pub mod theme;
mod widgets;

mod bar;
mod graph;
mod panels;

pub use theme::theme;

/// Everything the editor shows. The app owns all of it, as always.
pub struct App {
    pub shelf_tab: usize,
    pub view_tab: usize,
    pub param_tab: usize,
    pub param_sub: usize,
    pub attr_tab: usize,
    pub net_tab: usize,
    pub detail_tab: usize,
    pub tree_tab: usize,
    pub renderer: usize,
    pub camera: usize,
    pub sides: usize,
    pub attrs: Vec<usize>,
    pub camera_vis: bool,
    pub indirect_vis: bool,
    pub transmission_vis: bool,
    pub selected_prim: Option<usize>,
    pub frame: f32,
    pub graph: libgui_nodes::GraphState,
    pub nodes: Vec<graph::Node>,
    pub links: Vec<libgui_nodes::Link>,
    pub tree: Vec<panels::Row>,
    pub cols: TableState,
    pub detail_cols: TableState,
}

impl Default for App {
    fn default() -> Self {
        Self {
            shelf_tab: 0,
            view_tab: 0,
            param_tab: 0,
            param_sub: 0,
            attr_tab: 0,
            net_tab: 0,
            detail_tab: 0,
            tree_tab: 0,
            renderer: 0,
            camera: 0,
            sides: 0,
            attrs: vec![0; 10],
            camera_vis: true,
            indirect_vis: true,
            transmission_vis: false,
            selected_prim: Some(1),
            frame: 1.0,
            graph: libgui_nodes::GraphState::default(),
            nodes: graph::nodes(),
            links: graph::links(),
            tree: panels::tree(),
            cols: panels::tree_columns(),
            detail_cols: panels::detail_columns(),
        }
    }
}

impl App {
    /// One frame of the whole editor.
    pub fn ui(&mut self, ui: &mut Ui) {
        let t = ui.theme.clone();
        let root = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container(root, Frame { fill: t.palette.bg_app, clip: true, ..Frame::none() }, |ui| {
            bar::menu_bar(ui, self);
            bar::shelf(ui, self);
            // The work area: viewport and its tables on the left, parameters
            // and the network on the right.
            let body = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)).gap(2.0);
            ui.container(body, Frame::none(), |ui| {
                let left = Layout::column().width(Size::Grow(0.615)).height(Size::Grow(1.0)).gap(2.0);
                ui.container(left, Frame::none(), |ui| {
                    panels::viewport(ui, self);
                    let tables = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(258.0)).gap(2.0);
                    ui.container(tables, Frame::none(), |ui| {
                        panels::scene_graph_tree(ui, self);
                        panels::scene_graph_details(ui, self);
                    });
                });
                let right = Layout::column().width(Size::Grow(0.385)).height(Size::Grow(1.0)).gap(2.0);
                ui.container(right, Frame::none(), |ui| {
                    panels::parameters(ui, self);
                    graph::network(ui, self);
                });
            });
            bar::timeline(ui, self);
        });
    }
}
