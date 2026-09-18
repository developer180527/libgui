//! Unity-style docking: tabs, splitters, and tear-off into real OS windows.
//!
//! # Model
//! A [`DockState`] owns one [`Surface`] per window. `SurfaceId::MAIN` is the
//! main window; every other surface is a floating window. Each surface holds a
//! tree of [`DockNode`]s: splits (with a fraction) and leaves (a tab stack).
//! Your tab type `T` is anything; panels are drawn through [`TabViewer`].
//!
//! # Host contract (multi-window)
//! libgui never creates windows. Once per loop iteration the host:
//! 1. forwards the global pointer: [`DockState::set_pointer`] (physical screen px);
//! 2. reports each window's placement: [`DockState::set_surface_frame`];
//! 3. calls [`DockState::update`] (drag state machine);
//! 4. syncs OS windows to [`DockState::surfaces`]: create windows for new floating
//!    surfaces, destroy windows whose surface disappeared, apply
//!    [`Surface::window_pos`] and [`Surface::visible`];
//! 5. renders every window with [`DockState::show`].
//!
//! # Feel
//! Everything that affects how docking *feels* is in [`DockConfig`].

use crate::{Axis, Color, Cursor, Frame, Id, Insets, Layout, LeafOptions, Rect, ScrollOptions, Size, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SurfaceId(pub u64);

impl SurfaceId {
    pub const MAIN: SurfaceId = SurfaceId(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn axis(self) -> Axis {
        match self {
            Side::Left | Side::Right => Axis::X,
            Side::Top | Side::Bottom => Axis::Y,
        }
    }

    /// Does the new node go first (left/top)?
    fn first(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }

    /// Part of `r` covered by a new pane of `fraction` docked on this side.
    fn part(self, r: Rect, fraction: f32) -> Rect {
        match self {
            Side::Left => Rect::new(r.x, r.y, r.w * fraction, r.h),
            Side::Right => Rect::new(r.right() - r.w * fraction, r.y, r.w * fraction, r.h),
            Side::Top => Rect::new(r.x, r.y, r.w, r.h * fraction),
            Side::Bottom => Rect::new(r.x, r.bottom() - r.h * fraction, r.w, r.h * fraction),
        }
    }
}

/// Every knob that affects the docking feel. Tweak live; nothing is cached.
#[derive(Clone, Debug)]
pub struct DockConfig {
    // Tabs
    pub tab_height: f32,
    pub tab_padding: f32,
    pub tab_min_width: f32,
    /// Left inset of the tab bar (floating windows use it to place the grab point).
    pub tab_bar_padding: f32,
    pub tab_radius: f32,
    // Splitters
    /// Visual gap between panes.
    pub splitter_size: f32,
    /// Extra grab area on each side of the gap.
    pub splitter_hit_pad: f32,
    pub min_pane_size: f32,
    // Dragging
    /// Pointer travel (logical px) before a press on a tab becomes a drag.
    pub drag_threshold: f32,
    /// How far (logical px) outside the tab bar the pointer must go to tear off.
    pub tear_off_distance: f32,
    /// Hide the dragged window while it hovers a drop target (Unity-style).
    pub hide_window_over_target: bool,
    /// Size of torn-off windows: the source pane's size, clamped to these.
    pub floating_min_size: Vec2,
    pub floating_max_size: Vec2,
    // Drop zones
    /// Fraction of a pane's width/height near each edge that means "split".
    pub edge_zone: f32,
    /// Distance from a window's edge (logical px) that means "dock to whole side".
    pub root_edge_px: f32,
    /// Share of the pane a split-drop gives to the dropped tab.
    pub split_fraction: f32,
    /// Share of the window a root-edge drop gives to the dropped tab.
    pub root_split_fraction: f32,
    // Animation (1/s; higher = snappier)
    pub preview_speed: f32,
    pub reorder_speed: f32,
    pub preview_alpha: f32,
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            tab_height: 30.0,
            tab_padding: 14.0,
            tab_min_width: 72.0,
            tab_bar_padding: 6.0,
            tab_radius: 6.0,
            splitter_size: 3.0,
            splitter_hit_pad: 3.0,
            min_pane_size: 90.0,
            drag_threshold: 6.0,
            tear_off_distance: 26.0,
            hide_window_over_target: true,
            floating_min_size: Vec2::new(280.0, 200.0),
            floating_max_size: Vec2::new(900.0, 700.0),
            edge_zone: 0.28,
            root_edge_px: 22.0,
            split_fraction: 0.5,
            root_split_fraction: 0.3,
            preview_speed: 26.0,
            reorder_speed: 22.0,
            preview_alpha: 0.2,
        }
    }
}

/// How the host draws panels.
pub trait TabViewer {
    type Tab;
    fn title(&self, tab: &Self::Tab) -> String;
    /// Stable identity (widget ids and retained state follow the tab).
    fn id(&self, tab: &Self::Tab) -> u64;
    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab);
    /// Wrap the panel in a scroll area (false for viewports).
    fn scroll(&self, _tab: &Self::Tab) -> bool {
        true
    }
    fn padding(&self, _tab: &Self::Tab) -> Insets {
        Insets::all(12.0)
    }
}

pub struct Leaf<T> {
    pub id: u64,
    pub tabs: Vec<T>,
    pub active: usize,
}

pub struct Split<T> {
    pub id: u64,
    pub axis: Axis,
    /// Share of the first child, 0..1.
    pub fraction: f32,
    pub first: Box<DockNode<T>>,
    pub second: Box<DockNode<T>>,
}

pub enum DockNode<T> {
    Leaf(Leaf<T>),
    Split(Split<T>),
}

impl<T> DockNode<T> {
    fn leaf_mut(&mut self, id: u64) -> Option<&mut Leaf<T>> {
        match self {
            DockNode::Leaf(l) if l.id == id => Some(l),
            DockNode::Leaf(_) => None,
            DockNode::Split(s) => s.first.leaf_mut(id).or_else(|| s.second.leaf_mut(id)),
        }
    }

    fn first_leaf_mut(&mut self) -> &mut Leaf<T> {
        match self {
            DockNode::Leaf(l) => l,
            DockNode::Split(s) => s.first.first_leaf_mut(),
        }
    }

    fn tab_count(&self) -> usize {
        match self {
            DockNode::Leaf(l) => l.tabs.len(),
            DockNode::Split(s) => s.first.tab_count() + s.second.tab_count(),
        }
    }

    fn for_each_tab<'a>(&'a self, f: &mut dyn FnMut(&'a T)) {
        match self {
            DockNode::Leaf(l) => l.tabs.iter().for_each(|t| f(t)),
            DockNode::Split(s) => {
                s.first.for_each_tab(f);
                s.second.for_each_tab(f);
            }
        }
    }

    fn drain_tabs(self, out: &mut Vec<T>) {
        match self {
            DockNode::Leaf(l) => out.extend(l.tabs),
            DockNode::Split(s) => {
                s.first.drain_tabs(out);
                s.second.drain_tabs(out);
            }
        }
    }
}

/// Remove empty leaves and collapse splits with a missing side.
fn prune<T>(node: DockNode<T>) -> Option<DockNode<T>> {
    match node {
        DockNode::Leaf(mut l) => {
            if l.tabs.is_empty() {
                None
            } else {
                l.active = l.active.min(l.tabs.len() - 1);
                Some(DockNode::Leaf(l))
            }
        }
        DockNode::Split(s) => match (prune(*s.first), prune(*s.second)) {
            (Some(a), Some(b)) => Some(DockNode::Split(Split { first: Box::new(a), second: Box::new(b), ..s })),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        },
    }
}

fn replace_leaf<T>(node: DockNode<T>, id: u64, f: &mut Option<impl FnOnce(DockNode<T>) -> DockNode<T>>) -> DockNode<T> {
    match node {
        DockNode::Leaf(l) if l.id == id => match f.take() {
            Some(f) => f(DockNode::Leaf(l)),
            None => DockNode::Leaf(l),
        },
        DockNode::Split(mut s) => {
            s.first = Box::new(replace_leaf(*s.first, id, f));
            s.second = Box::new(replace_leaf(*s.second, id, f));
            DockNode::Split(s)
        }
        other => other,
    }
}

/// Last-frame geometry of a leaf, in the surface's logical coordinates.
#[derive(Clone, Debug, Default)]
struct LeafGeom {
    id: u64,
    rect: Rect,
    bar: Rect,
    tabs: Vec<Rect>,
}

pub struct Surface<T> {
    pub id: SurfaceId,
    pub root: Option<DockNode<T>>,
    pub floating: bool,
    /// Requested inner top-left of the window, physical screen px. `Some` while
    /// libgui is moving the window (dragging); the host applies it every update.
    pub window_pos: Option<Vec2>,
    /// Initial inner size (logical px) for creating the OS window.
    pub window_size: Vec2,
    pub visible: bool,
    origin: Vec2,
    scale: f32,
    has_frame: bool,
    root_rect: Rect,
    leaves: Vec<LeafGeom>,
}

impl<T> Surface<T> {
    fn new(id: SurfaceId, root: Option<DockNode<T>>, floating: bool) -> Self {
        Self {
            id,
            root,
            floating,
            window_pos: None,
            window_size: Vec2::new(480.0, 360.0),
            visible: true,
            origin: Vec2::ZERO,
            scale: 1.0,
            has_frame: false,
            root_rect: Rect::default(),
            leaves: Vec::new(),
        }
    }

    pub fn tab_count(&self) -> usize {
        self.root.as_ref().map_or(0, |r| r.tab_count())
    }

    /// First tab in tree order (e.g. for the window title).
    pub fn first_tab(&self) -> Option<&T> {
        let mut first = None;
        if let Some(r) = &self.root {
            r.for_each_tab(&mut |t| {
                if first.is_none() {
                    first = Some(t);
                }
            });
        }
        first
    }

    fn to_local(&self, screen: Vec2) -> Vec2 {
        (screen - self.origin) * (1.0 / self.scale)
    }

    fn is_single_tab(&self) -> bool {
        matches!(&self.root, Some(DockNode::Leaf(l)) if l.tabs.len() == 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropKind {
    /// Into a tab stack at `index`.
    Tab { leaf: u64, index: usize },
    /// Split a pane.
    Split { leaf: u64, side: Side },
    /// Along a whole window edge.
    Root { side: Side },
    /// Into an empty window.
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropTarget {
    pub surface: SurfaceId,
    pub kind: DropKind,
    /// Preview rect in the target surface's logical coordinates.
    pub preview: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Pressed, not yet past the drag threshold.
    Pending,
    /// Reordering inside the tab bar.
    Reorder,
    /// A floating window follows the pointer. `grab` = pointer offset inside that window.
    Floating { surface: SurfaceId, grab: Vec2 },
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    source: SurfaceId,
    leaf: u64,
    index: usize,
    press: Vec2,
    /// Pointer offset inside the pressed tab (logical).
    grab: Vec2,
    phase: Phase,
    target: Option<DropTarget>,
}

pub struct DockState<T> {
    surfaces: Vec<Surface<T>>,
    next_id: u64,
    pub config: DockConfig,
    pointer: Vec2,
    pointer_down: bool,
    drag: Option<Drag>,
    focused_leaf: Option<u64>,
    /// Tab that just got displaced by a live reorder: (leaf, index, start offset).
    reorder_shift: Option<(u64, usize, f32)>,
}

enum Action {
    StartDrag { leaf: u64, index: usize, grab: Vec2 },
}

impl<T> DockState<T> {
    pub fn new() -> Self {
        Self {
            surfaces: vec![Surface::new(SurfaceId::MAIN, None, false)],
            next_id: 1,
            config: DockConfig::default(),
            pointer: Vec2::ZERO,
            pointer_down: false,
            drag: None,
            focused_leaf: None,
            reorder_shift: None,
        }
    }

    fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // ---- building layouts -----------------------------------------------

    pub fn leaf(&mut self, tabs: Vec<T>) -> DockNode<T> {
        DockNode::Leaf(Leaf { id: self.next(), tabs, active: 0 })
    }

    pub fn split(&mut self, axis: Axis, fraction: f32, first: DockNode<T>, second: DockNode<T>) -> DockNode<T> {
        DockNode::Split(Split { id: self.next(), axis, fraction, first: Box::new(first), second: Box::new(second) })
    }

    pub fn set_root(&mut self, surface: SurfaceId, root: DockNode<T>) {
        if let Some(s) = self.surface_mut(surface) {
            s.root = Some(root);
        }
    }

    // ---- queries --------------------------------------------------------

    pub fn surfaces(&self) -> &[Surface<T>] {
        &self.surfaces
    }

    pub fn surface(&self, id: SurfaceId) -> Option<&Surface<T>> {
        self.surfaces.iter().find(|s| s.id == id)
    }

    fn surface_mut(&mut self, id: SurfaceId) -> Option<&mut Surface<T>> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    fn index_of(&self, id: SurfaceId) -> Option<usize> {
        self.surfaces.iter().position(|s| s.id == id)
    }

    /// A tab drag is in progress (past the threshold).
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some_and(|d| d.phase != Phase::Pending)
    }

    /// Abort a drag. A torn-off window stays floating where it is; nothing docks.
    pub fn cancel_drag(&mut self) {
        if let Some(Drag { phase: Phase::Floating { surface, .. }, .. }) = self.drag {
            if let Some(s) = self.surface_mut(surface) {
                s.visible = true;
                s.window_pos = None;
            }
        }
        self.drag = None;
    }

    /// Current drop target while dragging, if any.
    pub fn drop_target(&self) -> Option<DropTarget> {
        self.drag.and_then(|d| d.target)
    }

    // ---- host input -----------------------------------------------------

    /// Global pointer in physical screen px, from whichever window got the event.
    pub fn set_pointer(&mut self, screen: Vec2, down: bool) {
        self.pointer = screen;
        self.pointer_down = down;
    }

    pub fn set_pointer_down(&mut self, down: bool) {
        self.pointer_down = down;
    }

    /// Where a surface's window content sits on screen (physical px) and its DPI scale.
    pub fn set_surface_frame(&mut self, id: SurfaceId, origin: Vec2, scale: f32) {
        if let Some(s) = self.surface_mut(id) {
            s.origin = origin;
            s.scale = scale.max(0.5);
            s.has_frame = true;
        }
    }

    /// The OS window was closed: move its tabs back into the main window.
    pub fn close_surface(&mut self, id: SurfaceId) {
        if id == SurfaceId::MAIN {
            return;
        }
        let Some(i) = self.index_of(id) else { return };
        let surface = self.surfaces.remove(i);
        if self.drag.is_some_and(|d| d.source == id || d.phase == (Phase::Floating { surface: id, grab: Vec2::ZERO })) {
            self.drag = None;
        }
        let mut tabs = Vec::new();
        if let Some(root) = surface.root {
            root.drain_tabs(&mut tabs);
        }
        if tabs.is_empty() {
            return;
        }
        let new_leaf = self.next();
        let main = &mut self.surfaces[0];
        match &mut main.root {
            Some(root) => root.first_leaf_mut().tabs.extend(tabs),
            None => main.root = Some(DockNode::Leaf(Leaf { id: new_leaf, tabs, active: 0 })),
        }
    }

    // ---- drag state machine ---------------------------------------------

    /// Advance dragging. Call once per loop iteration after input, before rendering.
    pub fn update(&mut self) {
        let Some(mut d) = self.drag else { return };
        let cfg = self.config.clone();

        if !self.pointer_down {
            if let Phase::Floating { surface, .. } = d.phase {
                if let Some(t) = d.target {
                    self.dock_into(surface, t);
                } else if let Some(s) = self.surface_mut(surface) {
                    s.visible = true;
                }
                if let Some(s) = self.surface_mut(surface) {
                    s.window_pos = None;
                }
            }
            self.drag = None;
            return;
        }

        let Some(si) = self.index_of(d.source) else {
            self.drag = None;
            return;
        };

        if d.phase == Phase::Pending {
            let moved = (self.pointer - d.press) * (1.0 / self.surfaces[si].scale);
            if moved.x.hypot(moved.y) < cfg.drag_threshold {
                return;
            }
            d.phase = Phase::Reorder;
        }

        if d.phase == Phase::Reorder {
            let geom = self.surfaces[si].leaves.iter().find(|g| g.id == d.leaf).cloned().unwrap_or_default();
            let local = self.surfaces[si].to_local(self.pointer);
            let tab_rect = geom.tabs.get(d.index).copied().unwrap_or_default();
            if self.surfaces[si].floating && self.surfaces[si].is_single_tab() {
                // The only tab of a floating window: drag moves the window.
                let grab = Vec2::new(tab_rect.x + d.grab.x, tab_rect.y + d.grab.y);
                d.phase = Phase::Floating { surface: d.source, grab };
            } else {
                let t = cfg.tear_off_distance;
                let b = geom.bar;
                let in_band = local.x >= b.x - t && local.x <= b.right() + t && local.y >= b.y - t && local.y <= b.bottom() + t;
                if in_band {
                    self.reorder(&mut d, &geom, local);
                } else {
                    self.tear_off(&mut d, si, &geom);
                }
            }
        }

        if let Phase::Floating { surface, grab } = d.phase {
            let target = self.find_target(self.pointer, surface);
            let hide = target.is_some() && cfg.hide_window_over_target && surface != d.source;
            let pointer = self.pointer;
            if let Some(s) = self.surface_mut(surface) {
                s.window_pos = Some(pointer - grab * s.scale);
                s.visible = !hide;
            }
            d.target = target;
        }
        self.drag = Some(d);
    }

    fn reorder(&mut self, d: &mut Drag, geom: &LeafGeom, local: Vec2) {
        let Some(j) = geom.tabs.iter().position(|r| local.x >= r.x && local.x < r.right()) else { return };
        let i = d.index;
        let Some(s) = self.surface_mut(d.source) else { return };
        let Some(leaf) = s.root.as_mut().and_then(|r| r.leaf_mut(d.leaf)) else { return };
        if j == i || j >= leaf.tabs.len() {
            return;
        }
        let tab = leaf.tabs.remove(i);
        leaf.tabs.insert(j, tab);
        leaf.active = j;
        let w = geom.tabs[i].w;
        self.reorder_shift = Some((d.leaf, i, if j > i { w } else { -w }));
        d.index = j;
    }

    fn tear_off(&mut self, d: &mut Drag, si: usize, geom: &LeafGeom) {
        let cfg = &self.config;
        let size = Vec2::new(
            geom.rect.w.clamp(cfg.floating_min_size.x, cfg.floating_max_size.x),
            geom.rect.h.clamp(cfg.floating_min_size.y, cfg.floating_max_size.y),
        );
        let grab = Vec2::new(cfg.tab_bar_padding + d.grab.x, d.grab.y);
        let source = &mut self.surfaces[si];
        let scale = source.scale;
        let Some(tab) = source.root.as_mut().and_then(|r| r.leaf_mut(d.leaf)).and_then(|l| {
            (d.index < l.tabs.len()).then(|| l.tabs.remove(d.index))
        }) else {
            return;
        };
        source.root = source.root.take().and_then(prune);

        let sid = SurfaceId(self.next());
        let leaf = self.next();
        let mut s = Surface::new(sid, Some(DockNode::Leaf(Leaf { id: leaf, tabs: vec![tab], active: 0 })), true);
        s.window_size = size;
        s.scale = scale;
        s.origin = self.pointer - grab * scale;
        s.window_pos = Some(s.origin);
        self.surfaces.push(s);
        self.focused_leaf = Some(leaf);
        d.phase = Phase::Floating { surface: sid, grab };
    }

    fn find_target(&self, pointer: Vec2, exclude: SurfaceId) -> Option<DropTarget> {
        let cfg = &self.config;
        // Floating windows are usually above the main window; newest on top.
        let order = self.surfaces.iter().skip(1).rev().chain(self.surfaces.iter().take(1));
        for s in order {
            if s.id == exclude || !s.visible || !s.has_frame {
                continue;
            }
            let p = s.to_local(pointer);
            let r = s.root_rect;
            if !r.contains(p) {
                continue;
            }
            let at = |kind, preview| Some(DropTarget { surface: s.id, kind, preview });
            if s.root.is_none() {
                return at(DropKind::Empty, r);
            }
            let e = cfg.root_edge_px;
            let root_side = if p.x < r.x + e {
                Some(Side::Left)
            } else if p.x > r.right() - e {
                Some(Side::Right)
            } else if p.y < r.y + e {
                Some(Side::Top)
            } else if p.y > r.bottom() - e {
                Some(Side::Bottom)
            } else {
                None
            };
            if let Some(side) = root_side {
                return at(DropKind::Root { side }, side.part(r, cfg.root_split_fraction));
            }
            for g in &s.leaves {
                if g.bar.contains(p) {
                    let index = g.tabs.iter().filter(|t| t.center().x < p.x).count();
                    return at(DropKind::Tab { leaf: g.id, index }, g.rect);
                }
                if g.rect.contains(p) {
                    let nx = (p.x - g.rect.x) / g.rect.w.max(1.0);
                    let ny = (p.y - g.rect.y) / g.rect.h.max(1.0);
                    let edges = [(nx, Side::Left), (1.0 - nx, Side::Right), (ny, Side::Top), (1.0 - ny, Side::Bottom)];
                    let (dist, side) = edges.into_iter().fold((f32::MAX, Side::Left), |a, b| if b.0 < a.0 { b } else { a });
                    if dist < cfg.edge_zone {
                        return at(DropKind::Split { leaf: g.id, side }, side.part(g.rect, cfg.split_fraction));
                    }
                    return at(DropKind::Tab { leaf: g.id, index: g.tabs.len() }, g.rect);
                }
            }
            return None;
        }
        None
    }

    /// Move all tabs of `from` into `target`, then remove `from`.
    fn dock_into(&mut self, from: SurfaceId, target: DropTarget) {
        let Some(fi) = self.index_of(from) else { return };
        if self.index_of(target.surface).is_none() || from == target.surface {
            return;
        }
        let mut tabs = Vec::new();
        if let Some(root) = self.surfaces.remove(fi).root {
            root.drain_tabs(&mut tabs);
        }
        if tabs.is_empty() {
            return;
        }
        let new_leaf_id = self.next();
        let split_id = self.next();
        let cfg = self.config.clone();
        let s = self.surface_mut(target.surface).unwrap();
        let new_leaf = |tabs| DockNode::Leaf(Leaf { id: new_leaf_id, tabs, active: 0 });
        let wrap = |side: Side, fraction: f32, existing: DockNode<T>, new: DockNode<T>| {
            let (first, second, fraction) =
                if side.first() { (new, existing, fraction) } else { (existing, new, 1.0 - fraction) };
            DockNode::Split(Split { id: split_id, axis: side.axis(), fraction, first: Box::new(first), second: Box::new(second) })
        };
        let focused = match target.kind {
            DropKind::Tab { leaf, index } => match s.root.as_mut().and_then(|r| r.leaf_mut(leaf)) {
                Some(l) => {
                    let index = index.min(l.tabs.len());
                    for (k, t) in tabs.into_iter().enumerate() {
                        l.tabs.insert(index + k, t);
                    }
                    l.active = index;
                    leaf
                }
                None => return,
            },
            DropKind::Split { leaf, side } => {
                let root = s.root.take().unwrap();
                let mut f = Some(|old| wrap(side, cfg.split_fraction, old, new_leaf(tabs)));
                s.root = Some(replace_leaf(root, leaf, &mut f));
                new_leaf_id
            }
            DropKind::Root { side } => {
                let root = s.root.take().unwrap();
                s.root = Some(wrap(side, cfg.root_split_fraction, root, new_leaf(tabs)));
                new_leaf_id
            }
            DropKind::Empty => {
                s.root = Some(new_leaf(tabs));
                new_leaf_id
            }
        };
        self.focused_leaf = Some(focused);
    }

    // ---- rendering --------------------------------------------------------

    /// Draw one surface's dock tree (fills the remaining space in `ui`).
    pub fn show<V: TabViewer<Tab = T>>(&mut self, ui: &mut Ui, surface: SurfaceId, viewer: &mut V) {
        let Some(si) = self.index_of(surface) else { return };
        let cfg = self.config.clone();
        let local_pointer = self.surfaces[si].to_local(self.pointer);
        let reorder = match self.drag {
            Some(d) if d.source == surface && d.phase == Phase::Reorder => Some((d.leaf, d.index, local_pointer.x - d.grab.x)),
            _ => None,
        };
        let shift = if reorder.is_some() { self.reorder_shift.take() } else { None };
        let mut root = self.surfaces[si].root.take();
        let mut cx = ShowCtx {
            viewer,
            cfg: &cfg,
            reorder,
            shift,
            focused: self.focused_leaf,
            geoms: Vec::new(),
            actions: Vec::new(),
        };
        let root_id = Id::new(("dock_root", surface.0));
        ui.container_id(root_id, Layout::column().shrink(), Frame::none(), |ui| match &mut root {
            Some(node) => show_node(ui, node, &mut cx),
            None => empty_hint(ui),
        });
        let geoms = std::mem::take(&mut cx.geoms);
        let actions = std::mem::take(&mut cx.actions);
        let s = &mut self.surfaces[si];
        s.root = root;
        s.root_rect = ui.rect_of(root_id).unwrap_or_default();
        s.leaves = geoms;

        for a in actions {
            match a {
                Action::StartDrag { leaf, index, grab } => {
                    self.focused_leaf = Some(leaf);
                    self.drag = Some(Drag {
                        source: surface,
                        leaf,
                        index,
                        press: self.pointer,
                        grab,
                        phase: Phase::Pending,
                        target: None,
                    });
                }
            }
        }

        if let Some(d) = self.drag {
            if d.phase != Phase::Pending && d.source == surface {
                ui.cursor = Cursor::Grabbing;
            }
            if let Some(t) = d.target.filter(|t| t.surface == surface) {
                drop_preview(ui, surface, t.preview, &cfg);
            }
        }
    }
}

impl<T> Default for DockState<T> {
    fn default() -> Self {
        Self::new()
    }
}

struct ShowCtx<'a, V: TabViewer> {
    viewer: &'a mut V,
    cfg: &'a DockConfig,
    /// (leaf, index, x) of the tab being reordered: it follows the pointer.
    reorder: Option<(u64, usize, f32)>,
    shift: Option<(u64, usize, f32)>,
    focused: Option<u64>,
    geoms: Vec<LeafGeom>,
    actions: Vec<Action>,
}

fn empty_hint(ui: &mut Ui) {
    let id = Id::new("dock_empty");
    ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, |p, r| {
        let t = p.theme;
        p.rect_bordered(r.shrink(12.0, 12.0, 12.0, 12.0), Color::TRANSPARENT, t.radius_large, 1.0, t.border_strong);
        p.text_centered(r, t.font_size, t.text_faint, "Drag a tab here");
    });
}

fn drop_preview(ui: &mut Ui, surface: crate::dock::SurfaceId, target: Rect, cfg: &DockConfig) {
    let id = Id::new(("dock_preview", surface.0));
    ui.keep_id(id);
    let r = target.shrink(3.0, 3.0, 3.0, 3.0);
    let sp = cfg.preview_speed;
    let x = ui.animate_with_speed(id, 0, r.x, sp);
    let y = ui.animate_with_speed(id, 1, r.y, sp);
    let w = ui.animate_with_speed(id, 2, r.w, sp);
    let h = ui.animate_with_speed(id, 3, r.h, sp);
    let alpha = cfg.preview_alpha;
    ui.overlay(move |p| {
        let t = p.theme;
        let r = Rect::new(x, y, w, h);
        p.rect_bordered(r, t.accent.with_alpha(alpha), t.radius, 2.0, t.accent.with_alpha(0.9));
    });
}

fn show_node<V: TabViewer>(ui: &mut Ui, node: &mut DockNode<V::Tab>, cx: &mut ShowCtx<V>) {
    match node {
        DockNode::Leaf(leaf) => show_leaf(ui, leaf, cx),
        DockNode::Split(split) => show_split(ui, split, cx),
    }
}

fn show_split<V: TabViewer>(ui: &mut Ui, s: &mut Split<V::Tab>, cx: &mut ShowCtx<V>) {
    let id = Id::new(("dock_split", s.id));
    let axis = s.axis;
    let main = |r: Rect| if axis == Axis::X { r.w } else { r.h };
    let total = ui.rect_of(id).map(main).unwrap_or(0.0) - cx.cfg.splitter_size;
    let base = match axis {
        Axis::X => Layout::row(),
        Axis::Y => Layout::column(),
    };
    let pane = |f: f32| match axis {
        Axis::X => Layout::column().width(Size::Grow(f)).height(Size::Grow(1.0)).shrink(),
        Axis::Y => Layout::column().width(Size::Grow(1.0)).height(Size::Grow(f)).shrink(),
    };
    let layout = base.width(Size::Grow(1.0)).height(Size::Grow(1.0)).align(crate::Align::Start, crate::Align::Start).shrink();
    ui.container_id(id, layout, Frame::none(), |ui| {
        let f = s.fraction;
        ui.container_id(Id::new(("dock_pane_a", s.id)), pane(f), Frame::none(), |ui| show_node(ui, &mut s.first, cx));
        splitter(ui, s, total, cx.cfg);
        ui.container_id(Id::new(("dock_pane_b", s.id)), pane(1.0 - f), Frame::none(), |ui| show_node(ui, &mut s.second, cx));
    });
}

fn splitter<T>(ui: &mut Ui, s: &mut Split<T>, total: f32, cfg: &DockConfig) {
    let id = Id::new(("dock_splitter", s.id));
    ui.keep_id(id);
    let resp = ui.interact(id);
    if resp.active && total > 0.0 {
        let d = if s.axis == Axis::X { resp.drag_delta.x } else { resp.drag_delta.y };
        let min = (cfg.min_pane_size / total).min(0.5);
        s.fraction = (s.fraction + d / total).clamp(min, 1.0 - min);
    }
    if resp.hovered || resp.active {
        ui.cursor = if s.axis == Axis::X { Cursor::ResizeHorizontal } else { Cursor::ResizeVertical };
    }
    let hot = ui.animate_bool(id, 0, resp.hovered || resp.active);
    let axis = s.axis;
    let layout = match axis {
        Axis::X => Layout::leaf(Size::Fixed(cfg.splitter_size), Size::Grow(1.0)),
        Axis::Y => Layout::leaf(Size::Grow(1.0), Size::Fixed(cfg.splitter_size)),
    };
    let opts = LeafOptions { interactive: true, hit_pad: cfg.splitter_hit_pad, hit_top: true };
    ui.add_leaf_ex(id, layout, Vec2::ZERO, opts, move |p, r| {
        if hot > 0.01 {
            let line = match axis {
                Axis::X => Rect::new(r.center().x - 1.0, r.y, 2.0, r.h),
                Axis::Y => Rect::new(r.x, r.center().y - 1.0, r.w, 2.0),
            };
            p.rect(line, p.theme.accent.with_alpha(0.85 * hot), 1.0);
        }
    });
}

fn show_leaf<V: TabViewer>(ui: &mut Ui, leaf: &mut Leaf<V::Tab>, cx: &mut ShowCtx<V>) {
    let leaf_id = Id::new(("dock_leaf", leaf.id));
    let bar_id = Id::new(("dock_bar", leaf.id));
    let tab_ids: Vec<Id> = leaf.tabs.iter().map(|t| Id::new(("dock_tab", leaf.id, cx.viewer.id(t)))).collect();
    let mut tab_rects: Vec<Rect> = tab_ids.iter().map(|&t| ui.rect_of(t).unwrap_or_default()).collect();
    tab_rects.sort_by(|a, b| a.x.total_cmp(&b.x));
    cx.geoms.push(LeafGeom {
        id: leaf.id,
        rect: ui.rect_of(leaf_id).unwrap_or_default(),
        bar: ui.rect_of(bar_id).unwrap_or_default(),
        tabs: tab_rects,
    });
    if leaf.tabs.is_empty() {
        return;
    }
    leaf.active = leaf.active.min(leaf.tabs.len() - 1);
    let cfg = cx.cfg;
    let focused = cx.focused == Some(leaf.id);

    ui.container_id(leaf_id, Layout::column().shrink(), Frame { clip: true, ..Frame::none() }, |ui| {
        let bar_layout = Layout::row()
            .height(Size::Fixed(cfg.tab_height))
            .padding(Insets { left: cfg.tab_bar_padding, top: 0.0, right: cfg.tab_bar_padding, bottom: 0.0 })
            .gap(2.0)
            .align(crate::Align::Start, crate::Align::End)
            .shrink();
        ui.container_id(bar_id, bar_layout, Frame::none(), |ui| {
            for i in 0..leaf.tabs.len() {
                let pressed = tab(ui, leaf, i, tab_ids[i], focused, cx);
                if pressed {
                    leaf.active = i;
                }
            }
        });

        let active = leaf.active;
        let tab = &mut leaf.tabs[active];
        let body_id = Id::new(("dock_body", cx.viewer.id(tab)));
        let padding = cx.viewer.padding(tab);
        let scroll = cx.viewer.scroll(tab);
        let body = Frame { fill: ui.theme.bg_panel, border: Color::TRANSPARENT, border_width: 0.0, radius: 0.0, shadow: false, clip: true };
        let viewer = &mut *cx.viewer;
        if scroll {
            ui.container_id(body_id, Layout::column().shrink(), body, |ui| {
                let opts = ScrollOptions { gap: ui.theme.space, padding, ..ScrollOptions::new(Size::Grow(1.0)) };
                ui.scroll_area_with("dock_scroll", opts, |ui| viewer.ui(ui, tab));
            });
        } else {
            let gap = ui.theme.space;
            ui.container_id(body_id, Layout::column().shrink().padding(padding).gap(gap), body, |ui| viewer.ui(ui, tab));
        }
    });
}

/// One tab. Returns true when pressed.
fn tab<V: TabViewer>(ui: &mut Ui, leaf: &Leaf<V::Tab>, i: usize, id: Id, leaf_focused: bool, cx: &mut ShowCtx<V>) -> bool {
    let cfg = cx.cfg;
    let title = cx.viewer.title(&leaf.tabs[i]);
    let size = ui.theme.font_size;
    let m = ui.fonts.measure(ui.font, size, &title);
    ui.keep_id(id);
    let resp = ui.interact(id);
    if resp.pressed {
        let grab = Vec2::new(resp.mouse_pos.x - resp.rect.x, resp.mouse_pos.y - resp.rect.y);
        cx.actions.push(Action::StartDrag { leaf: leaf.id, index: i, grab });
    }
    let active = i == leaf.active;
    let hover = ui.animate_bool(id, 0, resp.hovered);
    let on = ui.animate_bool(id, 1, active);
    let follow = cx.reorder.filter(|&(l, idx, _)| l == leaf.id && idx == i).map(|(_, _, x)| x);
    if let Some((l, at, off)) = cx.shift {
        if l == leaf.id && at == i {
            ui.set_anim(id, 2, off);
        }
    }
    let slide = ui.animate_with_speed(id, 2, 0.0, cfg.reorder_speed);
    let (pad, radius) = (cfg.tab_padding, cfg.tab_radius);
    let content = Vec2::new(m.x.max(cfg.tab_min_width - 2.0 * pad), m.y);
    let layout = Layout::leaf(Size::Fit, Size::Fixed(cfg.tab_height - 4.0)).padding(Insets::xy(pad, 0.0));
    ui.add_leaf(id, layout, content, true, move |p, r| {
        let t = p.theme;
        let mut r = r;
        match follow {
            Some(x) => r.x = x,
            None => r.x += slide,
        }
        let lifted = follow.is_some();
        if lifted {
            p.shadow(r.translate(0.0, 2.0), radius, 8.0, t.shadow);
        }
        // Active tab merges into the panel below: extend it down under the body.
        let fill = t.surface.with_alpha(0.55 * hover).lerp(t.bg_panel, on.max(if lifted { 1.0 } else { 0.0 }));
        p.rect(Rect::new(r.x, r.y, r.w, r.h + radius), fill, radius);
        if on > 0.01 && leaf_focused {
            p.rect(Rect::new(r.x + radius * 0.5, r.y, r.w - radius, 2.0), t.accent.with_alpha(on), 1.0);
        }
        let fg = t.text_muted.lerp(t.text, on.max(hover * 0.6));
        p.text_centered(r, size, fg, &title);
    });
    resp.pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> DockState<&'static str> {
        let mut d = DockState::new();
        let a = d.leaf(vec!["outliner", "inspector"]);
        let b = d.leaf(vec!["viewport"]);
        let root = d.split(Axis::X, 0.25, a, b);
        d.set_root(SurfaceId::MAIN, root);
        d
    }

    fn leaves(n: &DockNode<&'static str>) -> Vec<Vec<&'static str>> {
        match n {
            DockNode::Leaf(l) => vec![l.tabs.clone()],
            DockNode::Split(s) => {
                let mut v = leaves(&s.first);
                v.extend(leaves(&s.second));
                v
            }
        }
    }

    #[test]
    fn prune_collapses_empty_leaves() {
        let mut d = fresh();
        let root = d.surfaces[0].root.take().unwrap();
        let mut f = Some(|n: DockNode<&'static str>| match n {
            DockNode::Leaf(mut l) => {
                l.tabs.clear();
                DockNode::Leaf(l)
            }
            other => other,
        });
        let first_leaf = match &root {
            DockNode::Split(s) => match &*s.first {
                DockNode::Leaf(l) => l.id,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let root = prune(replace_leaf(root, first_leaf, &mut f)).unwrap();
        assert_eq!(leaves(&root), vec![vec!["viewport"]]);
    }

    /// Simulates the host loop: tear a tab off, hover a split zone, drop.
    #[test]
    fn tear_off_then_dock_as_split() {
        let mut d = fresh();
        d.set_surface_frame(SurfaceId::MAIN, Vec2::new(100.0, 100.0), 2.0);
        let (left, right) = match d.surfaces[0].root.as_ref().unwrap() {
            DockNode::Split(s) => match (&*s.first, &*s.second) {
                (DockNode::Leaf(a), DockNode::Leaf(b)) => (a.id, b.id),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        // Geometry as `show` would have recorded it (logical px).
        d.surfaces[0].root_rect = Rect::new(0.0, 0.0, 800.0, 600.0);
        d.surfaces[0].leaves = vec![
            LeafGeom {
                id: left,
                rect: Rect::new(0.0, 0.0, 200.0, 600.0),
                bar: Rect::new(0.0, 0.0, 200.0, 30.0),
                tabs: vec![Rect::new(6.0, 4.0, 80.0, 26.0), Rect::new(88.0, 4.0, 90.0, 26.0)],
            },
            LeafGeom {
                id: right,
                rect: Rect::new(203.0, 0.0, 597.0, 600.0),
                bar: Rect::new(203.0, 0.0, 597.0, 30.0),
                tabs: vec![Rect::new(209.0, 4.0, 90.0, 26.0)],
            },
        ];
        let screen = |x: f32, y: f32| Vec2::new(100.0 + x * 2.0, 100.0 + y * 2.0);

        // Press "inspector" (index 1) and drag it down, out of the bar.
        d.set_pointer(screen(120.0, 15.0), true);
        d.drag = Some(Drag {
            source: SurfaceId::MAIN,
            leaf: left,
            index: 1,
            press: d.pointer,
            grab: Vec2::new(32.0, 11.0),
            phase: Phase::Pending,
            target: None,
        });
        d.set_pointer(screen(120.0, 200.0), true);
        d.update();
        assert_eq!(d.surfaces.len(), 2, "tear-off created a floating surface");
        let floating = d.surfaces[1].id;
        assert_eq!(d.surfaces[1].window_pos, Some(screen(120.0, 200.0) - Vec2::new(6.0 + 32.0, 11.0) * 2.0));
        assert_eq!(leaves(d.surfaces[0].root.as_ref().unwrap()), vec![vec!["outliner"], vec!["viewport"]]);

        // Pretend the floating window exists, then hover the right edge zone of the viewport pane.
        d.set_surface_frame(floating, Vec2::new(5000.0, 5000.0), 2.0);
        d.set_pointer(screen(760.0, 300.0), true);
        d.update();
        let t = d.drop_target().expect("target");
        assert_eq!(t.kind, DropKind::Split { leaf: right, side: Side::Right });
        assert!(!d.surfaces[1].visible, "dragged window hides over a target");

        // Release: docked, floating surface gone.
        d.set_pointer(screen(760.0, 300.0), false);
        d.update();
        assert_eq!(d.surfaces.len(), 1);
        assert_eq!(leaves(d.surfaces[0].root.as_ref().unwrap()), vec![vec!["outliner"], vec!["viewport"], vec!["inspector"]]);
    }

    #[test]
    fn closing_floating_window_returns_tabs() {
        let mut d = fresh();
        let sid = SurfaceId(99);
        let leaf = d.leaf(vec!["console"]);
        d.surfaces.push(Surface::new(sid, Some(leaf), true));
        d.close_surface(sid);
        assert_eq!(d.surfaces.len(), 1);
        assert_eq!(leaves(d.surfaces[0].root.as_ref().unwrap())[0], vec!["outliner", "inspector", "console"]);
    }
}
