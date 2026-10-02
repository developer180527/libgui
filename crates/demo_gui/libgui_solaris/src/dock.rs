//! The editor body as a dock tree.
//!
//! The rigid version of this demo nested the five panels in hand-written
//! containers with the split fractions baked in, which looked right and moved
//! not at all. A tool's panels are expected to be draggable, resizable and
//! tearable into their own window, and libgui already has all of that — so the
//! body is a `DockState` and the panels are its tabs.
//!
//! The tab strips are the pleasant part: a panel in an application like this
//! wears a row of tabs ("Scene View", "Animation Editor", …) that the first
//! version drew as decoration. Here they are real dock tabs in one leaf, so
//! they switch, reorder, move between panels and tear off for free.

use libgui::*;

use crate::{Editor, graph, panels};

/// One panel. The dock owns which are open and where; this only says what each
/// one draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    SceneView,
    AnimationEditor,
    GeometrySpreadsheet,
    SceneGraphTree,
    SceneGraphDetails,
    SceneGraphLayers,
    LayoutAssetGallery,
    Parameters,
    ContextOptions,
    PerformanceMonitor,
    RenderScheduler,
    Network,
    MaterialPalette,
    PresetBrowser,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::SceneView => "Scene View",
            Tab::AnimationEditor => "Animation Editor",
            Tab::GeometrySpreadsheet => "Geometry Spreadsheet",
            Tab::SceneGraphTree => "Scene Graph Tree",
            Tab::SceneGraphDetails => "Scene Graph Details",
            Tab::SceneGraphLayers => "Scene Graph Layers",
            Tab::LayoutAssetGallery => "Layout Asset Gallery",
            Tab::Parameters => "rendergeometrysettings2",
            Tab::ContextOptions => "Context Options Editor",
            Tab::PerformanceMonitor => "Performance Monitor",
            Tab::RenderScheduler => "Render Scheduler",
            Tab::Network => "/stage",
            Tab::MaterialPalette => "Material Palette",
            Tab::PresetBrowser => "Preset Browser",
        }
    }

    /// A fixed name per panel, never shown. Titles are for people and may be
    /// reworded; this is what a saved layout would record, so it stays put.
    fn key(self) -> &'static str {
        match self {
            Tab::SceneView => "scene-view",
            Tab::AnimationEditor => "animation-editor",
            Tab::GeometrySpreadsheet => "geometry-spreadsheet",
            Tab::SceneGraphTree => "scene-graph-tree",
            Tab::SceneGraphDetails => "scene-graph-details",
            Tab::SceneGraphLayers => "scene-graph-layers",
            Tab::LayoutAssetGallery => "layout-asset-gallery",
            Tab::Parameters => "parameters",
            Tab::ContextOptions => "context-options",
            Tab::PerformanceMonitor => "performance-monitor",
            Tab::RenderScheduler => "render-scheduler",
            Tab::Network => "network",
            Tab::MaterialPalette => "material-palette",
            Tab::PresetBrowser => "preset-browser",
        }
    }

    /// The viewport and the node network place their own content to the pixel;
    /// everything else is happier in a scroll area.
    fn scrolls(self) -> bool {
        !matches!(self, Tab::SceneView | Tab::Network)
    }
}

/// Borrows the editor state so the dock can own the tree: `show` needs `&mut`
/// on the dock while the panels need `&mut` on everything else.
pub struct Viewer<'a> {
    pub ed: &'a mut Editor,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&self, tab: &Tab) -> String {
        tab.title().to_string()
    }

    fn id(&self, tab: &Tab) -> u64 {
        // Stable per panel, so widget ids and retained state follow a tab when
        // it is dragged to another leaf or torn into its own window — and from
        // a fixed name rather than the enum's position, so a saved layout would
        // survive a panel being added in the middle.
        Id::from_name(tab.key()).0
    }

    fn scroll(&self, tab: &Tab) -> bool {
        tab.scrolls()
    }

    fn padding(&self, _tab: &Tab) -> Insets {
        Insets::all(0.0)
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::SceneView => panels::viewport(ui, self.ed),
            Tab::SceneGraphTree => panels::scene_graph_tree(ui, self.ed),
            Tab::SceneGraphDetails => panels::scene_graph_details(ui, self.ed),
            Tab::Parameters => panels::parameters(ui, self.ed),
            Tab::Network => graph::network(ui, self.ed),
            // The demo is about density and layout, not about having fourteen
            // finished panels. These say so rather than pretending.
            other => placeholder(ui, other.title()),
        }
    }
}

/// An empty panel that admits it. Cheap, and it keeps every tab draggable.
fn placeholder(ui: &mut Ui, title: &str) {
    let t = ui.theme.clone();
    let col = Layout::column()
        .width(Size::Grow(1.0))
        .height(Size::Grow(1.0))
        .padding(Insets::all(10.0))
        .gap(4.0)
        .align(Align::Center, Align::Center);
    let id = ui.make_id(("empty", title));
    ui.container_id(id, col, Frame::none(), |ui| {
        ui.text_with(title, t.metrics.font_size, t.palette.text_muted);
    });
}

/// The layout the editor opens with — the same arrangement the rigid version
/// hard-coded, now a tree the user can take apart.
pub fn initial() -> DockState<Tab> {
    let mut dock = DockState::new();

    let view = dock.leaf(vec![Tab::SceneView, Tab::AnimationEditor, Tab::GeometrySpreadsheet]);
    let tree = dock.leaf(vec![Tab::SceneGraphTree]);
    let details = dock.leaf(vec![Tab::SceneGraphDetails, Tab::SceneGraphLayers, Tab::LayoutAssetGallery]);
    let params = dock.leaf(vec![
        Tab::Parameters,
        Tab::ContextOptions,
        Tab::PerformanceMonitor,
        Tab::RenderScheduler,
    ]);
    let net = dock.leaf(vec![Tab::Network, Tab::MaterialPalette, Tab::PresetBrowser]);

    // Left: the viewport over the two scene-graph tables.
    let tables = dock.split(Axis::X, 0.52, tree, details);
    let left = dock.split(Axis::Y, 0.74, view, tables);
    // Right: parameters over the network.
    let right = dock.split(Axis::Y, 0.52, params, net);
    let root = dock.split(Axis::X, 0.615, left, right);

    dock.set_root(SurfaceId::MAIN, root);
    dock
}
