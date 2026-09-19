//! Flex-style layout solved *after* the frame is built (Clay / RAD-debugger
//! style), so a container can fit its content and grow children can share the
//! remaining space in a single frame, which a lay-out-as-you-go immediate UI cannot do.

use crate::{Id, Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// Exact size in logical pixels.
    Fixed(f32),
    /// Shrink-wrap the content (plus padding).
    Fit,
    /// Take a share of the parent's leftover space, weighted. Never smaller than Fit.
    Grow(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    X,
    Y,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "theme-toml", derive(serde::Serialize, serde::Deserialize), serde(deny_unknown_fields))]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    pub const fn all(v: f32) -> Self {
        Self { left: v, top: v, right: v, bottom: v }
    }

    pub const fn xy(x: f32, y: f32) -> Self {
        Self { left: x, top: y, right: x, bottom: y }
    }

    fn along(&self, axis: Axis) -> f32 {
        match axis {
            Axis::X => self.left + self.right,
            Axis::Y => self.top + self.bottom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: Size,
    pub height: Size,
    /// Direction children are stacked in.
    pub axis: Axis,
    pub padding: Insets,
    pub gap: f32,
    pub align_main: Align,
    pub align_cross: Align,
    /// Ignore children when computing the minimum size (they get clipped
    /// instead). Needed for split panes whose size comes from a fraction.
    pub shrink: bool,
}

impl Layout {
    pub const fn row() -> Self {
        Self {
            width: Size::Grow(1.0),
            height: Size::Fit,
            axis: Axis::X,
            padding: Insets::all(0.0),
            gap: 0.0,
            align_main: Align::Start,
            align_cross: Align::Center,
            shrink: false,
        }
    }

    pub const fn column() -> Self {
        Self {
            width: Size::Grow(1.0),
            height: Size::Grow(1.0),
            axis: Axis::Y,
            padding: Insets::all(0.0),
            gap: 0.0,
            align_main: Align::Start,
            align_cross: Align::Start,
            shrink: false,
        }
    }

    pub const fn leaf(width: Size, height: Size) -> Self {
        let mut l = Self::row();
        l.width = width;
        l.height = height;
        l
    }

    pub const fn width(mut self, s: Size) -> Self {
        self.width = s;
        self
    }

    pub const fn height(mut self, s: Size) -> Self {
        self.height = s;
        self
    }

    pub const fn padding(mut self, p: Insets) -> Self {
        self.padding = p;
        self
    }

    pub const fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self
    }

    pub const fn shrink(mut self) -> Self {
        self.shrink = true;
        self
    }

    pub const fn align(mut self, main: Align, cross: Align) -> Self {
        self.align_main = main;
        self.align_cross = cross;
        self
    }

    fn size(&self, axis: Axis) -> Size {
        match axis {
            Axis::X => self.width,
            Axis::Y => self.height,
        }
    }
}

/// Handle into the frame's [`PaintArena`](crate::paint_arena::PaintArena).
pub(crate) type PaintFn = u32;

/// Scroll container state for one frame (vertical scrolling).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scroll {
    /// How far the content is scrolled, per axis.
    pub offset: Vec2,
    pub scroll_x: bool,
    pub scroll_y: bool,
    pub bar_x: Id,
    pub bar_y: Id,
    /// Per-bar visibility 0..1 and hover/drag emphasis 0..1 (animated by Ui).
    pub vis_x: f32,
    pub hot_x: f32,
    pub vis_y: f32,
    pub hot_y: f32,
    pub style: crate::ScrollbarStyle,
    /// This scroll is standing still, so its content sits on the pixel grid.
    pub snap: bool,
}

pub(crate) struct Node {
    pub id: Id,
    pub layout: Layout,
    /// Content size of a leaf (e.g. measured text), excluding padding.
    pub intrinsic: Vec2,
    pub children: Kids,
    pub min: Vec2,
    pub rect: Rect,
    pub interactive: bool,
    /// Extra hit-test margin around the rect (e.g. thin splitters).
    pub hit_pad: f32,
    /// Hit-test before normal widgets regardless of paint order.
    pub hit_top: bool,
    pub clip: bool,
    pub paint: Option<PaintFn>,
    pub scroll: Option<Scroll>,
    /// Registered as a drop zone this frame (only when it accepts the drag in
    /// flight, so rejecting zones do not shadow accepting ones beneath them).
    pub drop_zone: bool,
    /// Scroll containers: full content size incl. padding, set by `place`.
    pub content: Vec2,
    /// Positioned at this rect (window coordinates), outside the parent's flow;
    /// painted and hit-tested above its flow siblings.
    pub absolute: Option<Rect>,
    /// Stacking order among absolute siblings. Ties keep build order.
    pub z: crate::Layer,
    /// A canvas: children are laid out, hit-tested and reported in *canvas*
    /// coordinates, and this maps those to the window.
    pub xform: Option<crate::Transform>,
}

impl Node {
    pub fn new(id: Id, layout: Layout) -> Self {
        Self {
            id,
            layout,
            intrinsic: Vec2::ZERO,
            children: Kids::EMPTY,
            min: Vec2::ZERO,
            rect: Rect::default(),
            interactive: false,
            hit_pad: 0.0,
            hit_top: false,
            clip: false,
            paint: None,
            scroll: None,
            drop_zone: false,
            content: Vec2::ZERO,
            absolute: None,
            z: crate::Layer::Window,
            xform: None,
        }
    }
}

fn get(v: Vec2, axis: Axis) -> f32 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
    }
}

fn from_axes(axis: Axis, main: f32, cross: f32) -> Vec2 {
    match axis {
        Axis::X => Vec2::new(main, cross),
        Axis::Y => Vec2::new(cross, main),
    }
}

/// Does this container scroll along `axis`?
fn scrolls(sc: Scroll, axis: Axis) -> bool {
    match axis {
        Axis::X => sc.scroll_x,
        Axis::Y => sc.scroll_y,
    }
}

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

/// Where a node's children are in the tree's child arena. Children are
/// contiguous, so a node carries eight bytes instead of a `Vec` and its heap
/// allocation — one per container, every frame, for a list that never changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Kids {
    pub start: u32,
    pub len: u32,
}

impl Kids {
    pub const EMPTY: Kids = Kids { start: 0, len: 0 };

    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start as usize + self.len as usize)
    }

}

/// Reusable working memory for one layout pass. Kept across frames so the
/// whole solve allocates nothing in the steady state; `place` and `paint` mark
/// their slice, use it, and truncate back, so recursion shares one buffer.
#[derive(Default)]
pub(crate) struct Scratch {
    /// Flow (non-absolute) children of the containers being placed.
    flow: Vec<u32>,
    /// Their main-axis sizes.
    mains: Vec<f32>,
    /// Absolute children being painted, as `(layer << 32) | index` so one
    /// integer sort puts them in stacking order and keeps build order within
    /// a layer.
    pub floating: Vec<u64>,
}

pub(crate) fn solve(nodes: &mut [Node], kids: &[u32], root: usize, rect: Rect, s: &mut Scratch) {
    measure(nodes, kids, root);
    place(nodes, kids, root, rect, s);
}

/// Bottom-up: minimum (fit) size of every node.
fn measure(nodes: &mut [Node], kids: &[u32], i: usize) -> Vec2 {
    let children = nodes[i].children;
    let l = nodes[i].layout;
    let mut content = nodes[i].intrinsic;
    let mut flow = 0;
    let (mut main, mut cross) = (0.0f32, 0.0f32);
    for k in children.range() {
        let c = kids[k] as usize;
        let m = measure(nodes, kids, c);
        if nodes[c].absolute.is_none() {
            main += get(m, l.axis);
            cross = cross.max(get(m, other(l.axis)));
            flow += 1;
        }
    }
    if flow > 0 {
        main += l.gap * (flow - 1) as f32;
        let c = from_axes(l.axis, main, cross);
        content = Vec2::new(content.x.max(c.x), content.y.max(c.y));
    }
    let mut min = Vec2::ZERO;
    for axis in [Axis::X, Axis::Y] {
        let v = match l.size(axis) {
            Size::Fixed(v) => v,
            Size::Fit | Size::Grow(_) if l.shrink => l.padding.along(axis),
            Size::Fit | Size::Grow(_) => get(content, axis) + l.padding.along(axis),
        };
        match axis {
            Axis::X => min.x = v,
            Axis::Y => min.y = v,
        }
    }
    // A growing scroll container can shrink below its content; that's the point.
    if let Some(sc) = nodes[i].scroll {
        if sc.scroll_y && matches!(l.height, Size::Grow(_)) {
            min.y = l.padding.along(Axis::Y);
        }
        if sc.scroll_x && matches!(l.width, Size::Grow(_)) {
            min.x = l.padding.along(Axis::X);
        }
    }
    nodes[i].min = min;
    min
}

/// Top-down: distribute space and assign final rects.
fn place(nodes: &mut [Node], kids: &[u32], i: usize, rect: Rect, s: &mut Scratch) {
    nodes[i].rect = rect;
    let all = nodes[i].children;
    for k in all.range() {
        let c = kids[k] as usize;
        if let Some(r) = nodes[c].absolute {
            place(nodes, kids, c, r, s);
        }
    }
    // Flow children, into this call's slice of the shared scratch. Everything
    // below indexes `base + k` rather than holding a slice, so the recursive
    // call at the end can borrow the scratch for its own slice above ours.
    let base = s.flow.len();
    for k in all.range() {
        let c = kids[k] as usize;
        if nodes[c].absolute.is_none() {
            s.flow.push(c as u32);
        }
    }
    let n = s.flow.len() - base;
    if n == 0 {
        return;
    }
    let l = nodes[i].layout;
    let p = l.padding;
    let mut inner = rect.shrink(p.left, p.top, p.right, p.bottom);
    // A canvas's own rect is in its parent's space, but its children live in
    // canvas space: hand them the region in the coordinates they use.
    if let Some(t) = nodes[i].xform {
        inner = t.inv_rect(inner);
    }
    let (axis, cross_axis) = (l.axis, other(l.axis));
    let mut inner_main = get(inner.size(), axis);
    let mut inner_cross = get(inner.size(), cross_axis);
    let gaps = l.gap * (n - 1) as f32;

    let mut fixed = 0.0;
    let mut weight = 0.0;
    for k in 0..n {
        let c = s.flow[base + k] as usize;
        match nodes[c].layout.size(axis) {
            Size::Grow(w) => weight += w,
            _ => fixed += get(nodes[c].min, axis),
        }
    }
    // A scroll container lays its children out in a box that is the larger of
    // the viewport and the content, on each axis it scrolls, then slides that
    // box by the offset. `Grow` children fill the box, not the viewport, so a
    // row inside a horizontally scrolling area spans the whole content width.
    let scroll = nodes[i].scroll;
    if let Some(sc) = scroll {
        let mut natural_main = gaps;
        let mut natural_cross = 0.0f32;
        for k in 0..n {
            let c = s.flow[base + k] as usize;
            natural_main += get(nodes[c].min, axis);
            natural_cross = natural_cross.max(get(nodes[c].min, cross_axis));
        }
        if scrolls(sc, axis) {
            inner_main = inner_main.max(natural_main);
        }
        if scrolls(sc, cross_axis) {
            inner_cross = inner_cross.max(natural_cross);
        }
    }
    let free = (inner_main - gaps - fixed).max(0.0);

    let mbase = s.mains.len();
    let mut total = gaps;
    for k in 0..n {
        let c = s.flow[base + k] as usize;
        let min = get(nodes[c].min, axis);
        let main = match nodes[c].layout.size(axis) {
            Size::Grow(w) if weight > 0.0 => (free * w / weight).max(min),
            _ => min,
        };
        total += main;
        s.mains.push(main);
    }
    let slack = (inner_main - total).max(0.0);
    let mut cursor = get(Vec2::new(inner.x, inner.y), axis)
        + match l.align_main {
            Align::Start => 0.0,
            Align::Center => slack * 0.5,
            Align::End => slack,
        };
    let mut cross_start = get(Vec2::new(inner.x, inner.y), cross_axis);
    if let Some(sc) = scroll {
        cursor -= get(sc.offset, axis);
        cross_start -= get(sc.offset, cross_axis);
        let content_main = total + l.padding.along(axis);
        let content_cross = inner_cross + l.padding.along(cross_axis);
        nodes[i].content = from_axes(axis, content_main, content_cross);
    }

    for k in 0..n {
        let c = s.flow[base + k] as usize;
        let main = s.mains[mbase + k];
        let cross = match nodes[c].layout.size(cross_axis) {
            Size::Grow(_) => inner_cross,
            _ => get(nodes[c].min, cross_axis),
        };
        let cross_off = match l.align_cross {
            Align::Start => 0.0,
            Align::Center => (inner_cross - cross) * 0.5,
            Align::End => inner_cross - cross,
        };
        let pos = from_axes(axis, cursor, cross_start + cross_off);
        let size = from_axes(axis, main, cross);
        place(nodes, kids, c, Rect::new(pos.x, pos.y, size.x, size.y), s);
        cursor += main + l.gap;
    }
    s.flow.truncate(base);
    s.mains.truncate(mbase);
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A tree built the way a test wants to write one — name a parent for each
    /// node, in any order — laid out into the contiguous child arena `solve`
    /// reads, which is what `Ui` produces while building.
    #[derive(Default)]
    struct Tree {
        nodes: Vec<Node>,
        links: Vec<(usize, usize)>,
    }

    impl Tree {
        fn root(layout: Layout) -> Self {
            Self { nodes: vec![Node::new(Id::new("root"), layout)], links: Vec::new() }
        }

        fn add(&mut self, parent: usize, mut n: Node) -> usize {
            let i = self.nodes.len();
            n.id = Id::new(i);
            self.nodes.push(n);
            self.links.push((parent, i));
            i
        }

        fn leaf(&mut self, parent: usize, w: Size, h: Size, content: Vec2) -> usize {
            let mut n = Node::new(Id::new(0u32), Layout::leaf(w, h));
            n.intrinsic = content;
            self.add(parent, n)
        }

        fn solve(&mut self, rect: Rect) -> &[Node] {
            let mut kids = Vec::new();
            for i in 0..self.nodes.len() {
                let start = kids.len() as u32;
                kids.extend(self.links.iter().filter(|&&(p, _)| p == i).map(|&(_, c)| c as u32));
                self.nodes[i].children = Kids { start, len: kids.len() as u32 - start };
            }
            let mut scratch = Scratch::default();
            super::solve(&mut self.nodes, &kids, 0, rect, &mut scratch);
            assert!(scratch.flow.is_empty() && scratch.mains.is_empty(), "place leaked scratch");
            &self.nodes
        }
    }

    #[test]
    fn grow_shares_leftover_space() {
        let mut t = Tree::root(Layout::row().gap(10.0).padding(Insets::all(5.0)));
        let a = t.leaf(0, Size::Fixed(100.0), Size::Fixed(20.0), Vec2::ZERO);
        let b = t.leaf(0, Size::Grow(1.0), Size::Fixed(20.0), Vec2::ZERO);
        let c = t.leaf(0, Size::Grow(3.0), Size::Fixed(20.0), Vec2::ZERO);
        let nodes = t.solve(Rect::new(0.0, 0.0, 530.0, 100.0));
        // inner 520, minus 100 fixed, minus 2 gaps = 400 → 100 / 300
        assert_eq!(nodes[a].rect, Rect::new(5.0, 40.0, 100.0, 20.0));
        assert_eq!(nodes[b].rect.x, 115.0);
        assert_eq!(nodes[b].rect.w, 100.0);
        assert_eq!(nodes[c].rect.x, 225.0);
        assert_eq!(nodes[c].rect.w, 300.0);
    }

    #[test]
    fn fit_wraps_children_and_padding() {
        let mut t = Tree::root(Layout::column());
        let l = Layout::column().width(Size::Fit).height(Size::Fit).gap(4.0).padding(Insets::xy(8.0, 6.0));
        let col = t.add(0, Node::new(Id::new("col"), l));
        t.leaf(col, Size::Fit, Size::Fit, Vec2::new(50.0, 10.0));
        t.leaf(col, Size::Fit, Size::Fit, Vec2::new(80.0, 12.0));
        let nodes = t.solve(Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(nodes[col].rect.size(), Vec2::new(80.0 + 16.0, 10.0 + 12.0 + 4.0 + 12.0));
    }

    #[test]
    fn scroll_container_offsets_children_and_reports_content() {
        let mut t = Tree::root(Layout::column());
        let mut sc = Node::new(Id::new("sc"), Layout::column().height(Size::Grow(1.0)).gap(2.0));
        sc.scroll = Some(Scroll {
            offset: Vec2::new(0.0, 30.0),
            scroll_x: false,
            scroll_y: true,
            bar_x: Id::new("bx"),
            bar_y: Id::new("by"),
            vis_x: 0.0,
            hot_x: 0.0,
            vis_y: 0.0,
            hot_y: 0.0,
            style: crate::Theme::dark().scrollbar,
            snap: true,
        });
        let sc = t.add(0, sc);
        let rows: Vec<usize> = (0..10).map(|_| t.leaf(sc, Size::Grow(1.0), Size::Fixed(20.0), Vec2::ZERO)).collect();
        let nodes = t.solve(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(nodes[sc].rect.h, 100.0, "viewport keeps parent height");
        assert_eq!(nodes[sc].content.y, 10.0 * 20.0 + 9.0 * 2.0);
        assert_eq!(nodes[rows[0]].rect.y, -30.0);
        assert_eq!(nodes[rows[9]].rect.y, 9.0 * 22.0 - 30.0);
    }

    #[test]
    fn shrink_ignores_children_min() {
        let mut t = Tree::root(Layout::row());
        let a = t.add(0, Node::new(Id::new("a"), Layout::column().width(Size::Grow(0.25)).shrink()));
        let b = t.add(0, Node::new(Id::new("b"), Layout::column().width(Size::Grow(0.75)).shrink()));
        t.leaf(a, Size::Fixed(500.0), Size::Fixed(10.0), Vec2::ZERO);
        let nodes = t.solve(Rect::new(0.0, 0.0, 400.0, 100.0));
        assert_eq!(nodes[a].rect.w, 100.0, "fraction wins over wide content");
        assert_eq!(nodes[b].rect.w, 300.0);
    }

    #[test]
    fn absolute_children_skip_flow() {
        let mut t = Tree::root(Layout::column().gap(10.0));
        let a = t.leaf(0, Size::Fixed(50.0), Size::Fixed(20.0), Vec2::ZERO);
        let f = t.leaf(0, Size::Fixed(999.0), Size::Fixed(999.0), Vec2::ZERO);
        t.nodes[f].absolute = Some(Rect::new(300.0, 40.0, 120.0, 80.0));
        let b = t.leaf(0, Size::Fixed(50.0), Size::Fixed(20.0), Vec2::ZERO);
        let nodes = t.solve(Rect::new(0.0, 0.0, 500.0, 500.0));
        assert_eq!(nodes[f].rect, Rect::new(300.0, 40.0, 120.0, 80.0));
        assert_eq!(nodes[b].rect.y, 30.0, "b follows a directly; the absolute node takes no space");
        assert_eq!(nodes[a].rect.y, 0.0);
    }

    #[test]
    fn grow_never_shrinks_below_content() {
        let mut t = Tree::root(Layout::row());
        let a = t.leaf(0, Size::Grow(1.0), Size::Fit, Vec2::new(300.0, 10.0));
        let nodes = t.solve(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(nodes[a].rect.w, 300.0);
    }
}
