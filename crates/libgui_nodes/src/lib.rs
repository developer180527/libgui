//! Node-graph editing for libgui.
//!
//! The app keeps its own graph. This crate never stores your nodes or links:
//! you declare them each frame, it lays them out, draws them, runs the
//! interactions, and hands back a list of [`GraphEvent`]s to apply. The only
//! thing it retains is interaction state — what is selected, what is being
//! dragged — in a [`GraphState`] you own.
//!
//! ```ignore
//! let events = libgui_nodes::graph(ui, "shader", &mut state, &style, |g| {
//!     for n in &doc.nodes {
//!         g.node(n.id, n.pos, &NodeConfig::new(&n.title).inputs(&n.ins).outputs(&n.outs),
//!                |ui| { ui.slider("Amount", &mut n.amount, 0.0, 1.0); });
//!     }
//!     for l in &doc.links {
//!         g.link(l.from, l.to);
//!     }
//! });
//! for e in events { doc.apply(e); }
//! ```
//!
//! Every edit arrives as an event rather than a mutation, so undo, validation
//! and networking are the app's to implement in one place.

mod graph;
mod style;
mod wire;

pub use graph::{graph, GraphUi, NodeConfig, NodeResponse, PortResponse};
pub use style::{GraphStyle, Routing};
pub use wire::wire_points;

use libgui::{CanvasState, Rect, Vec2};
use std::collections::HashSet;

/// Identifies a node. The app's own id, whatever it uses.
pub type NodeId = u64;

/// Which side of a node a port is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    In,
    Out,
}

/// A port: a node, a side, and which row it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortId {
    pub node: NodeId,
    pub side: Side,
    pub index: u16,
}

impl PortId {
    pub fn input(node: NodeId, index: u16) -> Self {
        Self { node, side: Side::In, index }
    }

    pub fn output(node: NodeId, index: u16) -> Self {
        Self { node, side: Side::Out, index }
    }
}

/// A link, from an output port to an input port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Link {
    pub from: PortId,
    pub to: PortId,
}

/// Something the user did. Apply these to your own graph; nothing is changed
/// for you, so one place in your code owns every edit.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphEvent {
    /// Move this node by `delta` (canvas units). Emitted once per selected
    /// node, so dragging a selection moves all of it.
    NodeMoved { node: NodeId, delta: Vec2 },
    /// The selection changed; read it from [`GraphState::selection`].
    SelectionChanged,
    /// Double-clicked a node's header.
    NodeActivated { node: NodeId },
    LinkCreated { link: Link },
    /// A link was detached — by dragging its input end away, or by deleting it.
    /// A re-route arrives as a `LinkRemoved` and then a `LinkCreated`.
    LinkRemoved { link: Link },
    /// Empty canvas was clicked; the selection has already been cleared.
    BackgroundClicked { pos: Vec2 },
}

/// What the user is currently dragging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    /// Moving the selection. `moved` guards the click-to-select on release.
    Nodes { moved: bool },
    /// Pulling a link out of `from`, with the loose end following the pointer.
    /// `detached` is the link picked up, to be restored if the drag is cancelled.
    Link { from: PortId, detached: Option<Link> },
}

/// Interaction state: what is selected and what is being dragged. Owned by the
/// app so it can be saved, inspected, or driven from elsewhere.
#[derive(Clone, Debug, Default)]
pub struct GraphState {
    pub view: CanvasState,
    pub selection: HashSet<NodeId>,
    pub selected_link: Option<Link>,
    pub(crate) drag: Option<Drag>,
    /// Last frame's links, so an input port knows it already has one before
    /// this frame's links have been declared.
    pub(crate) links_prev: Vec<Link>,
    /// Last frame's measured node heights, for nodes whose body decides it.
    pub(crate) heights: Vec<(NodeId, f32)>,
}

impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a link is being dragged: useful for dimming incompatible
    /// ports, or for a status line.
    pub fn dragging_link(&self) -> Option<PortId> {
        match self.drag {
            Some(Drag::Link { from, .. }) => Some(from),
            _ => None,
        }
    }

    pub fn is_selected(&self, node: NodeId) -> bool {
        self.selection.contains(&node)
    }

    /// Centre the view on `bounds`, fitting it in `area` with a margin.
    pub fn frame_bounds(&mut self, bounds: Rect, area: Rect, margin: f32) {
        if bounds.w <= 0.0 || bounds.h <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
            return;
        }
        let zx = (area.w - margin * 2.0) / bounds.w;
        let zy = (area.h - margin * 2.0) / bounds.h;
        let z = zx.min(zy).clamp(self.view.min_zoom, self.view.max_zoom);
        self.view.zoom = z;
        let c = bounds.center();
        self.view.pan = Vec2::new(area.w * 0.5 - c.x * z, area.h * 0.5 - c.y * z);
    }
}
