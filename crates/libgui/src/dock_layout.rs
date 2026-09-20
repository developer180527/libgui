//! Saving and restoring a dock layout.
//!
//! A tool that forgets where its panels were is a tool people re-arrange every
//! morning. [`DockState::layout`] takes a snapshot — the split tree, the
//! fractions, which tabs sit in which pane and which is active, and where the
//! floating windows are — and [`DockState::restore`] puts it back.
//!
//! # What identifies a tab
//!
//! The snapshot stores [`TabViewer::id`], not the tab itself: libgui never
//! knows what a `Tab` is. Restoring asks the app for the tab behind each id,
//! so **the id has to mean the same thing in the next version of your app**.
//! A hash of a stable name (`"outliner"`) survives adding and reordering
//! panels; an enum's discriminant (`tab as u64`) does not.
//!
//! # What a restore promises
//!
//! An app changes between the save and the load — panels are added, removed,
//! renamed, a plugin is uninstalled — so a layout is *advice*, not a command:
//!
//! - a tab the app no longer offers is dropped, and its pane, and the window
//!   it was alone in, rather than leaving an empty hole;
//! - a tab the app has that the layout never mentioned is **not** silently
//!   lost: it is not placed, and [`Restored::missing_from`] names it so the app can
//!   dock it somewhere (it knows which tabs it offered);
//! - a layout written by a newer version is refused, not half-applied;
//! - nonsense in the numbers — a NaN fraction, a zero-size window, an active
//!   index past the end — is clamped rather than trusted. A layout file is
//!   something users copy between machines and edit by hand.
//!
//! ```ignore
//! // Saving, e.g. when the window closes:
//! let text = dock.layout(&viewer).to_toml();
//! std::fs::write(path, text)?;            // the app's filesystem, not libgui's
//!
//! // Loading, at startup:
//! let saved = DockLayout::from_toml(&std::fs::read_to_string(path)?)?;
//! let report = dock.restore(&saved, |id| Tab::ALL.iter().copied().find(|t| t.id() == id));
//! for id in report.missing_from(Tab::ALL.iter().map(|t| t.id())) {
//!     // A panel this version added: put it somewhere sensible.
//! }
//! ```

use crate::dock::{DockNode, DockState, Leaf, Split, Surface, SurfaceId, TabViewer};
use crate::{Axis, Rect, Vec2};

#[cfg(feature = "serde")]
macro_rules! layout_serde {
    ($item:item) => {
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        $item
    };
}
#[cfg(not(feature = "serde"))]
macro_rules! layout_serde {
    ($item:item) => {
        $item
    };
}

layout_serde! {
/// A saved dock layout: plain data, no `Tab` and no host types, so an app can
/// write it wherever it keeps its settings.
///
/// libgui does no I/O. With the `theme-toml` feature (on by default) it can
/// render itself as TOML; otherwise serialise it with any serde format.
#[derive(Clone, Debug, PartialEq)]
pub struct DockLayout {
    /// [`DockLayout::VERSION`] when written.
    pub version: u32,
    /// The main window first, then the floating ones.
    pub surfaces: Vec<SurfaceLayout>,
}
}

layout_serde! {
/// One window's worth of layout.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceLayout {
    /// A torn-off window rather than the main one.
    pub floating: bool,
    /// Size of a floating window, logical px. The host owns *where* an OS
    /// window is: save that alongside if you want it back.
    pub size: Vec2,
    /// Placement of an in-app floating panel ([`crate::FloatingMode::InApp`]),
    /// in the main window's coordinates.
    pub rect: Rect,
    pub root: Option<NodeLayout>,
}
}

layout_serde! {
/// The split tree, with tab identities in place of tabs.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum NodeLayout {
    /// A pane: a stack of tabs, one of them active.
    Leaf {
        /// [`TabViewer::id`] per tab, in order.
        tabs: Vec<u64>,
        active: usize,
        /// [`TabViewer::title`] per tab, for whoever opens the file: ids are
        /// opaque numbers, and someone debugging a customer's workspace
        /// should be able to see that pane holds the outliner. Written on
        /// save, ignored on restore, and allowed to be absent.
        #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Vec::is_empty"))]
        titles: Vec<String>,
    },
    Split {
        axis: Axis,
        /// Share of the first child, 0..1.
        fraction: f32,
        first: Box<NodeLayout>,
        second: Box<NodeLayout>,
    },
}
}

/// Why a layout could not be restored at all. Anything survivable is repaired
/// instead and reported in [`Restored`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// Written by a newer version of libgui: its meaning is not known, so it
    /// is refused rather than half-applied. Fall back to your default layout.
    Version { found: u32, supported: u32 },
    /// The text was not a layout (only with `theme-toml`).
    Parse(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::Version { found, supported } => {
                write!(f, "dock layout version {found} is newer than this build understands ({supported})")
            }
            LayoutError::Parse(e) => write!(f, "not a dock layout: {e}"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// What a restore actually did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Restored {
    /// Tab ids placed, in tree order.
    pub placed: Vec<u64>,
    /// Tab ids in the layout that the app did not recognise, so were dropped.
    pub dropped: Vec<u64>,
}

impl Restored {
    /// The app's tabs that the layout never mentioned — a panel added since
    /// the layout was saved. They are nowhere yet; dock them.
    pub fn missing_from(&self, offered: impl IntoIterator<Item = u64>) -> Vec<u64> {
        offered.into_iter().filter(|id| !self.placed.contains(id)).collect()
    }

    /// Nothing was placed: the layout was empty, or described a version of the
    /// app with no tabs in common with this one. Use your default layout.
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }
}

impl DockLayout {
    /// Bumped when the meaning of the format changes. An older layout is
    /// understood; a newer one is refused.
    pub const VERSION: u32 = 1;

    /// Render as TOML. Pure data: writing it somewhere is the app's job.
    #[cfg(feature = "theme-toml")]
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Parse a layout written by [`DockLayout::to_toml`].
    #[cfg(feature = "theme-toml")]
    pub fn from_toml(text: &str) -> Result<Self, LayoutError> {
        toml::from_str(text).map_err(|e| LayoutError::Parse(e.to_string()))
    }
}

/// `v` if it is a real number in `lo..=hi`, else `fallback`.
fn clamped(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

impl<T> DockState<T> {
    /// Snapshot the layout. Tabs are recorded by [`TabViewer::id`], which must
    /// mean the same thing the next time the app runs.
    pub fn layout<V: TabViewer<Tab = T>>(&self, viewer: &V) -> DockLayout {
        DockLayout {
            version: DockLayout::VERSION,
            surfaces: self
                .surfaces()
                .iter()
                .map(|s| SurfaceLayout {
                    floating: s.floating,
                    size: s.window_size,
                    rect: s.rect,
                    root: s.root.as_ref().map(|r| node_layout(r, viewer)),
                })
                .collect(),
        }
    }

    /// Rebuild from a snapshot, asking `tab` for the tab behind each saved id.
    ///
    /// Returns `None` from `tab` for an id this version of the app does not
    /// have: the tab is dropped, and any pane or window left empty goes with
    /// it. Every surface and tab the dock held before is replaced.
    ///
    /// See [`Restored`] for what to do about tabs the layout never mentioned.
    pub fn restore(
        &mut self,
        layout: &DockLayout,
        mut tab: impl FnMut(u64) -> Option<T>,
    ) -> Result<Restored, LayoutError> {
        if layout.version > DockLayout::VERSION {
            return Err(LayoutError::Version { found: layout.version, supported: DockLayout::VERSION });
        }
        let mut report = Restored::default();
        // Built first, because growing the tree needs `self` for fresh ids.
        let mut roots: Vec<(bool, Vec2, Rect, Option<DockNode<T>>)> = Vec::new();
        for s in &layout.surfaces {
            let root = match &s.root {
                Some(n) => self.build_node(n, &mut tab, &mut report),
                None => None,
            };
            let size = Vec2::new(clamped(s.size.x, 80.0, 16_384.0, 480.0), clamped(s.size.y, 60.0, 16_384.0, 360.0));
            let rect = Rect::new(
                clamped(s.rect.x, -16_384.0, 16_384.0, 80.0),
                clamped(s.rect.y, -16_384.0, 16_384.0, 80.0),
                clamped(s.rect.w, 80.0, 16_384.0, size.x),
                clamped(s.rect.h, 60.0, 16_384.0, size.y),
            );
            roots.push((s.floating, size, rect, root));
        }

        self.reset_surfaces();
        for (i, (floating, size, rect, root)) in roots.into_iter().enumerate() {
            // The main surface is the one that already exists, whatever the
            // file says: a dock always has one, and it is never floating.
            if i == 0 && !floating {
                if let Some(s) = self.surface_mut(SurfaceId::MAIN) {
                    s.root = root;
                    s.window_size = size;
                }
                continue;
            }
            // A window whose tabs all vanished is not worth reopening.
            if root.is_none() {
                continue;
            }
            let id = self.new_surface_id();
            let mut s = Surface::restored(id, root, floating);
            s.window_size = size;
            s.rect = rect;
            self.push_surface(s);
        }
        Ok(report)
    }

    /// One node, dropping tabs the app does not know and collapsing whatever
    /// that empties.
    fn build_node(
        &mut self,
        node: &NodeLayout,
        tab: &mut impl FnMut(u64) -> Option<T>,
        report: &mut Restored,
    ) -> Option<DockNode<T>> {
        match node {
            NodeLayout::Leaf { tabs, active, .. } => {
                let mut kept = Vec::new();
                let mut active_out = 0;
                for (i, id) in tabs.iter().enumerate() {
                    match tab(*id) {
                        Some(t) => {
                            if i == *active {
                                active_out = kept.len();
                            }
                            report.placed.push(*id);
                            kept.push(t);
                        }
                        None => report.dropped.push(*id),
                    }
                }
                if kept.is_empty() {
                    return None;
                }
                let id = self.new_node_id();
                let active = active_out.min(kept.len() - 1);
                Some(DockNode::Leaf(Leaf { id, tabs: kept, active }))
            }
            NodeLayout::Split { axis, fraction, first, second } => {
                let a = self.build_node(first, tab, report);
                let b = self.build_node(second, tab, report);
                match (a, b) {
                    // A split with one side gone is just the other side.
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                    (Some(a), Some(b)) => {
                        let id = self.new_node_id();
                        Some(DockNode::Split(Split {
                            id,
                            axis: *axis,
                            fraction: clamped(*fraction, 0.05, 0.95, 0.5),
                            first: Box::new(a),
                            second: Box::new(b),
                        }))
                    }
                }
            }
        }
    }
}

fn node_layout<T, V: TabViewer<Tab = T>>(node: &DockNode<T>, viewer: &V) -> NodeLayout {
    match node {
        DockNode::Leaf(l) => NodeLayout::Leaf {
            tabs: l.tabs.iter().map(|t| viewer.id(t)).collect(),
            active: l.active,
            titles: l.tabs.iter().map(|t| viewer.title(t)).collect(),
        },
        DockNode::Split(s) => NodeLayout::Split {
            axis: s.axis,
            fraction: s.fraction,
            first: Box::new(node_layout(&s.first, viewer)),
            second: Box::new(node_layout(&s.second, viewer)),
        },
    }
}
