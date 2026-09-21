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
//! A hash of a stable name survives adding and reordering panels — use
//! [`crate::Id::from_name`]`("outliner").0`, whose encoding no toolchain can
//! change; an enum's discriminant (`tab as u64`) does not.
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
//! let text = dock.layout(&viewer).to_toml()?;
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
        /// [`TabViewer::id`] per tab, in order. Written as `"0x…"` strings:
        /// a hashed id uses all 64 bits, and TOML integers stop at `i64::MAX`
        /// while a JavaScript JSON reader stops at 2^53. Read as either.
        #[cfg_attr(feature = "serde", serde(with = "tab_ids"))]
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

/// Tab ids as `"0x…"` strings on the way out, and a string or an integer on
/// the way in — version-1 layouts wrote plain integers, and they still load.
#[cfg(feature = "serde")]
mod tab_ids {
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(ids: &[u64], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(ids.iter().map(|id| format!("{id:#018x}")))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u64>, D::Error> {
        struct Ids;
        impl<'de> Visitor<'de> for Ids {
            type Value = Vec<u64>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a list of tab ids")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u64>, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(Id(id)) = seq.next_element()? {
                    out.push(id);
                }
                Ok(out)
            }
        }
        d.deserialize_seq(Ids)
    }

    struct Id(u64);

    impl<'de> serde::Deserialize<'de> for Id {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct One;
            impl Visitor<'_> for One {
                type Value = Id;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a tab id: \"0x…\" or a non-negative integer")
                }
                fn visit_u64<E: de::Error>(self, v: u64) -> Result<Id, E> {
                    Ok(Id(v))
                }
                fn visit_i64<E: de::Error>(self, v: i64) -> Result<Id, E> {
                    u64::try_from(v).map(Id).map_err(|_| E::custom("a tab id cannot be negative"))
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Id, E> {
                    let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
                        Some(hex) => u64::from_str_radix(hex, 16),
                        None => v.parse(),
                    };
                    parsed.map(Id).map_err(|_| E::custom(format!("not a tab id: {v:?}")))
                }
            }
            d.deserialize_any(One)
        }
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
    /// The layout could not be written out (only with `theme-toml`).
    Serialize(String),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::Version { found, supported } => {
                write!(f, "dock layout version {found} is newer than this build understands ({supported})")
            }
            LayoutError::Parse(e) => write!(f, "not a dock layout: {e}"),
            LayoutError::Serialize(e) => write!(f, "could not write the dock layout: {e}"),
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
    ///
    /// 2: tab ids are written as `"0x…"` strings. Version 1 wrote integers,
    /// which is out of spec for TOML above `i64::MAX` — about half of all
    /// hashed ids. Both are read.
    pub const VERSION: u32 = 2;

    /// Render as TOML. Pure data: writing it somewhere is the app's job.
    ///
    /// A `Result`, because the usual next step is writing it over the file
    /// the last good layout is in: an error must stop that, where an empty
    /// string would have silently replaced the user's workspace with nothing.
    #[cfg(feature = "theme-toml")]
    pub fn to_toml(&self) -> Result<String, LayoutError> {
        toml::to_string_pretty(self).map_err(|e| LayoutError::Serialize(e.to_string()))
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
