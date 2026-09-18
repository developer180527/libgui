//! Flex-style layout solved *after* the frame is built (Clay / RAD-debugger
//! style), so a container can fit its content and grow children can share the
//! remaining space in a single frame, which a lay-out-as-you-go immediate UI cannot do.

use crate::painter::Painter;
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

pub(crate) type PaintFn = Box<dyn FnOnce(&mut Painter, Rect)>;

/// Scroll container state for one frame (vertical scrolling).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scroll {
    pub offset: f32,
    pub bar_id: Id,
    /// Scrollbar visibility 0..1 and hover/drag emphasis 0..1 (animated by Ui).
    pub visible: f32,
    pub hover: f32,
    pub style: crate::ScrollbarStyle,
}

pub(crate) struct Node {
    pub id: Id,
    pub layout: Layout,
    /// Content size of a leaf (e.g. measured text), excluding padding.
    pub intrinsic: Vec2,
    pub children: Vec<usize>,
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
    /// Scroll containers: full content height incl. padding, set by `place`.
    pub content: f32,
}

impl Node {
    pub fn new(id: Id, layout: Layout) -> Self {
        Self {
            id,
            layout,
            intrinsic: Vec2::ZERO,
            children: Vec::new(),
            min: Vec2::ZERO,
            rect: Rect::default(),
            interactive: false,
            hit_pad: 0.0,
            hit_top: false,
            clip: false,
            paint: None,
            scroll: None,
            content: 0.0,
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

fn other(axis: Axis) -> Axis {
    match axis {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    }
}

pub(crate) fn solve(nodes: &mut [Node], root: usize, rect: Rect) {
    measure(nodes, root);
    place(nodes, root, rect);
}

/// Bottom-up: minimum (fit) size of every node.
fn measure(nodes: &mut [Node], i: usize) -> Vec2 {
    let children = std::mem::take(&mut nodes[i].children);
    let l = nodes[i].layout;
    let mut content = nodes[i].intrinsic;
    if !children.is_empty() {
        let (mut main, mut cross) = (0.0f32, 0.0f32);
        for &c in &children {
            let m = measure(nodes, c);
            main += get(m, l.axis);
            cross = cross.max(get(m, other(l.axis)));
        }
        main += l.gap * (children.len() - 1) as f32;
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
    if nodes[i].scroll.is_some() {
        if let Size::Grow(_) = l.height {
            min.y = l.padding.along(Axis::Y);
        }
    }
    nodes[i].min = min;
    nodes[i].children = children;
    min
}

/// Top-down: distribute space and assign final rects.
fn place(nodes: &mut [Node], i: usize, rect: Rect) {
    nodes[i].rect = rect;
    let children = std::mem::take(&mut nodes[i].children);
    if children.is_empty() {
        return;
    }
    let l = nodes[i].layout;
    let p = l.padding;
    let inner = rect.shrink(p.left, p.top, p.right, p.bottom);
    let (axis, cross_axis) = (l.axis, other(l.axis));
    let mut inner_main = get(inner.size(), axis);
    let inner_cross = get(inner.size(), cross_axis);
    let gaps = l.gap * (children.len() - 1) as f32;

    let mut fixed = 0.0;
    let mut weight = 0.0;
    for &c in &children {
        match nodes[c].layout.size(axis) {
            Size::Grow(w) => weight += w,
            _ => fixed += get(nodes[c].min, axis),
        }
    }
    let scroll = nodes[i].scroll.map(|s| s.offset);
    if scroll.is_some() {
        // Content may exceed the viewport; lay out at its natural size.
        let content: f32 = children.iter().map(|&c| get(nodes[c].min, axis)).sum::<f32>() + gaps;
        inner_main = inner_main.max(content);
    }
    let free = (inner_main - gaps - fixed).max(0.0);

    let mains: Vec<f32> = children
        .iter()
        .map(|&c| {
            let min = get(nodes[c].min, axis);
            match nodes[c].layout.size(axis) {
                Size::Grow(w) if weight > 0.0 => (free * w / weight).max(min),
                _ => min,
            }
        })
        .collect();
    let total: f32 = mains.iter().sum::<f32>() + gaps;
    let slack = (inner_main - total).max(0.0);
    let mut cursor = get(Vec2::new(inner.x, inner.y), axis)
        + match l.align_main {
            Align::Start => 0.0,
            Align::Center => slack * 0.5,
            Align::End => slack,
        };
    if let Some(offset) = scroll {
        cursor -= offset;
        nodes[i].content = total + l.padding.along(axis);
    }
    let cross_start = get(Vec2::new(inner.x, inner.y), cross_axis);

    for (k, &c) in children.iter().enumerate() {
        let main = mains[k];
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
        place(nodes, c, Rect::new(pos.x, pos.y, size.x, size.y));
        cursor += main + l.gap;
    }
    nodes[i].children = children;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(nodes: &mut Vec<Node>, parent: usize, w: Size, h: Size, content: Vec2) -> usize {
        let mut n = Node::new(Id::new(nodes.len()), Layout::leaf(w, h));
        n.intrinsic = content;
        nodes.push(n);
        let i = nodes.len() - 1;
        nodes[parent].children.push(i);
        i
    }

    #[test]
    fn grow_shares_leftover_space() {
        let mut nodes = vec![Node::new(Id::new("root"), Layout::row().gap(10.0).padding(Insets::all(5.0)))];
        let a = leaf(&mut nodes, 0, Size::Fixed(100.0), Size::Fixed(20.0), Vec2::ZERO);
        let b = leaf(&mut nodes, 0, Size::Grow(1.0), Size::Fixed(20.0), Vec2::ZERO);
        let c = leaf(&mut nodes, 0, Size::Grow(3.0), Size::Fixed(20.0), Vec2::ZERO);
        solve(&mut nodes, 0, Rect::new(0.0, 0.0, 530.0, 100.0));
        // inner 520, minus 100 fixed, minus 2 gaps = 400 → 100 / 300
        assert_eq!(nodes[a].rect, Rect::new(5.0, 40.0, 100.0, 20.0));
        assert_eq!(nodes[b].rect.x, 115.0);
        assert_eq!(nodes[b].rect.w, 100.0);
        assert_eq!(nodes[c].rect.x, 225.0);
        assert_eq!(nodes[c].rect.w, 300.0);
    }

    #[test]
    fn fit_wraps_children_and_padding() {
        let mut nodes = vec![Node::new(Id::new("root"), Layout::column())];
        let col = {
            let n = Node::new(Id::new("col"), Layout::column().width(Size::Fit).height(Size::Fit).gap(4.0).padding(Insets::xy(8.0, 6.0)));
            nodes.push(n);
            nodes[0].children.push(1);
            1
        };
        leaf(&mut nodes, col, Size::Fit, Size::Fit, Vec2::new(50.0, 10.0));
        leaf(&mut nodes, col, Size::Fit, Size::Fit, Vec2::new(80.0, 12.0));
        solve(&mut nodes, 0, Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(nodes[col].rect.size(), Vec2::new(80.0 + 16.0, 10.0 + 12.0 + 4.0 + 12.0));
    }

    #[test]
    fn scroll_container_offsets_children_and_reports_content() {
        let mut nodes = vec![Node::new(Id::new("root"), Layout::column())];
        let mut sc = Node::new(Id::new("sc"), Layout::column().height(Size::Grow(1.0)).gap(2.0));
        sc.scroll = Some(Scroll { offset: 30.0, bar_id: Id::new("bar"), visible: 0.0, hover: 0.0, style: crate::Theme::dark().scrollbar });
        nodes.push(sc);
        nodes[0].children.push(1);
        let rows: Vec<usize> = (0..10).map(|_| leaf(&mut nodes, 1, Size::Grow(1.0), Size::Fixed(20.0), Vec2::ZERO)).collect();
        solve(&mut nodes, 0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(nodes[1].rect.h, 100.0, "viewport keeps parent height");
        assert_eq!(nodes[1].content, 10.0 * 20.0 + 9.0 * 2.0);
        assert_eq!(nodes[rows[0]].rect.y, -30.0);
        assert_eq!(nodes[rows[9]].rect.y, 9.0 * 22.0 - 30.0);
    }

    #[test]
    fn shrink_ignores_children_min() {
        let mut nodes = vec![Node::new(Id::new("root"), Layout::row())];
        let a = {
            nodes.push(Node::new(Id::new("a"), Layout::column().width(Size::Grow(0.25)).shrink()));
            nodes[0].children.push(1);
            1
        };
        let b = {
            nodes.push(Node::new(Id::new("b"), Layout::column().width(Size::Grow(0.75)).shrink()));
            nodes[0].children.push(2);
            2
        };
        leaf(&mut nodes, a, Size::Fixed(500.0), Size::Fixed(10.0), Vec2::ZERO);
        solve(&mut nodes, 0, Rect::new(0.0, 0.0, 400.0, 100.0));
        assert_eq!(nodes[a].rect.w, 100.0, "fraction wins over wide content");
        assert_eq!(nodes[b].rect.w, 300.0);
    }

    #[test]
    fn grow_never_shrinks_below_content() {
        let mut nodes = vec![Node::new(Id::new("root"), Layout::row())];
        let a = leaf(&mut nodes, 0, Size::Grow(1.0), Size::Fit, Vec2::new(300.0, 10.0));
        solve(&mut nodes, 0, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(nodes[a].rect.w, 300.0);
    }
}
