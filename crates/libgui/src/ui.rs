use crate::layout::{self, Node, PaintFn, Scroll};
use crate::text_edit::TextState;
use crate::{Atlas, Color, Cursor, DrawList, Event, FontId, Fonts, Id, Input, Insets, Key, Layout, Painter, Rect, Size, Theme, Vec2};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Result of an interactive widget for this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Response {
    pub id: Id,
    /// Rect from the previous frame's layout (one frame latency, invisible in practice).
    pub rect: Rect,
    pub hovered: bool,
    /// Mouse went down on this widget and has not been released yet.
    pub active: bool,
    pub pressed: bool,
    pub clicked: bool,
    pub drag_delta: Vec2,
    pub scroll: Vec2,
    pub mouse_pos: Vec2,
}

/// Visual style for a container.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub fill: Color,
    pub border: Color,
    pub border_width: f32,
    pub radius: f32,
    pub shadow: bool,
    pub clip: bool,
}

impl Frame {
    pub fn panel(t: &Theme) -> Self {
        Self { fill: t.bg_panel, border: t.border, border_width: 1.0, radius: 0.0, shadow: false, clip: true }
    }

    pub fn card(t: &Theme) -> Self {
        Self { fill: t.bg_panel, border: t.border, border_width: 1.0, radius: t.radius_large, shadow: true, clip: true }
    }

    pub fn none() -> Self {
        Self { fill: Color::TRANSPARENT, border: Color::TRANSPARENT, border_width: 0.0, radius: 0.0, shadow: false, clip: false }
    }
}

/// Options for [`Ui::scroll_area_with`].
#[derive(Clone, Copy, Debug)]
pub struct ScrollOptions {
    pub height: Size,
    pub gap: f32,
    pub padding: Insets,
    /// Keep following the end while scrolled to the bottom (logs, consoles).
    pub stick_to_end: bool,
}

impl ScrollOptions {
    pub fn new(height: Size) -> Self {
        Self { height, gap: 0.0, padding: Insets::all(0.0), stick_to_end: false }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ScrollState {
    target: f32,
    offset: f32,
    content: f32,
    viewport: f32,
}

/// Everything a renderer needs for this frame.
pub struct FrameOutput<'a> {
    pub draw: &'a DrawList,
    pub atlas: &'a Atlas,
    pub screen_size: Vec2,
    pub scale: f32,
    pub clear_color: Color,
}

pub struct Ui {
    pub theme: Theme,
    pub fonts: Fonts,
    pub font: FontId,
    /// Cursor requested by widgets this frame; apply it in the host.
    pub cursor: Cursor,
    pub(crate) input: Input,
    mouse_prev: Vec2,
    pub(crate) mouse_delta: Vec2,
    prev_down: bool,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
    nodes: Vec<Node>,
    stack: Vec<usize>,
    // Retained state
    rects: HashMap<Id, Rect>,
    hits: Vec<(Id, Rect)>,
    hovered: Option<Id>,
    active: Option<Id>,
    anims: HashMap<(Id, u8), f32>,
    seen: HashSet<Id>,
    draw: DrawList,
    pub(crate) time: f64,
    // Keyboard focus
    pub(crate) focused: Option<Id>,
    pub(crate) focus_order: Vec<Id>,
    pending_tab: Option<bool>,
    pub(crate) text_states: HashMap<Id, TextState>,
    pub(crate) copied: Option<String>,
    pub(crate) ime_rect: Option<Rect>,
    // Scrolling
    scroll_states: HashMap<Id, ScrollState>,
    scroll_hits: Vec<(Id, Rect)>,
    scroll_target: Option<Id>,
    /// Hit rects that win over normal widgets (splitters).
    top_hits: Vec<(Id, Rect)>,
    overlays: Vec<Box<dyn FnOnce(&mut Painter)>>,
}

/// Options for [`Ui::add_leaf_ex`].
#[derive(Clone, Copy, Debug, Default)]
pub struct LeafOptions {
    pub interactive: bool,
    /// Grow the hit area by this many px on every side.
    pub hit_pad: f32,
    /// Hit-test above normal widgets regardless of paint order.
    pub hit_top: bool,
}

impl Ui {
    pub fn new(theme: Theme, font_bytes: &[u8]) -> Self {
        let mut fonts = Fonts::new();
        let font = fonts.add_font(font_bytes);
        Self {
            theme,
            fonts,
            font,
            cursor: Cursor::Default,
            input: Input::default(),
            mouse_prev: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            prev_down: false,
            pressed: false,
            released: false,
            nodes: Vec::new(),
            stack: Vec::new(),
            rects: HashMap::new(),
            hits: Vec::new(),
            hovered: None,
            active: None,
            anims: HashMap::new(),
            seen: HashSet::new(),
            draw: DrawList::default(),
            time: 0.0,
            focused: None,
            focus_order: Vec::new(),
            pending_tab: None,
            text_states: HashMap::new(),
            copied: None,
            ime_rect: None,
            scroll_states: HashMap::new(),
            scroll_hits: Vec::new(),
            scroll_target: None,
            top_hits: Vec::new(),
            overlays: Vec::new(),
        }
    }

    pub fn input(&self) -> &Input {
        &self.input
    }

    /// True while any widget is being dragged or pressed; hosts can use this to
    /// avoid forwarding the mouse to the game.
    pub fn wants_mouse(&self) -> bool {
        self.active.is_some() || self.hovered.is_some()
    }

    /// True while a text field has focus: don't route keys to the game.
    pub fn wants_keyboard(&self) -> bool {
        self.focused.is_some()
    }

    /// Text the user copied/cut this frame; write it to the OS clipboard.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    /// Caret rect of the focused text field, for positioning an IME window.
    pub fn ime_rect(&self) -> Option<Rect> {
        self.ime_rect
    }

    pub fn focused(&self) -> Option<Id> {
        self.focused
    }

    pub fn set_focus(&mut self, id: Option<Id>) {
        self.focused = id;
    }

    pub fn begin_frame(&mut self, input: Input) {
        self.pressed = input.mouse_down && !self.prev_down;
        self.released = !input.mouse_down && self.prev_down;
        self.prev_down = input.mouse_down;
        self.mouse_delta = input.mouse_pos - self.mouse_prev;
        self.mouse_prev = input.mouse_pos;
        self.fonts.set_scale(input.scale);
        // Hit-test against last frame's layout; later entries were painted on top.
        self.hovered = if input.mouse_inside {
            let top = self.top_hits.iter().rev().find(|(_, r)| r.contains(input.mouse_pos));
            top.or_else(|| self.hits.iter().rev().find(|(_, r)| r.contains(input.mouse_pos))).map(|(id, _)| *id)
        } else {
            None
        };
        // Scroll goes to the innermost scroll area under the mouse.
        self.scroll_target = if input.mouse_inside {
            self.scroll_hits.iter().rev().find(|(_, r)| r.contains(input.mouse_pos)).map(|(id, _)| *id)
        } else {
            None
        };
        // Clicking anywhere drops focus; a text field re-takes it if it was the target.
        if self.pressed {
            self.focused = None;
        }
        self.pending_tab = input.events.iter().rev().find_map(|e| match e {
            Event::Key(Key::Tab, m) => Some(m.shift),
            _ => None,
        });
        self.time += input.dt as f64;
        self.focus_order.clear();
        self.overlays.clear();
        self.ime_rect = None;
        self.cursor = Cursor::Default;
        self.input = input;
        self.nodes.clear();
        self.stack.clear();
        self.seen.clear();
        let s = self.input.screen_size;
        let root = Node::new(Id::new("root"), Layout::column().width(Size::Fixed(s.x)).height(Size::Fixed(s.y)));
        self.nodes.push(root);
        self.stack.push(0);
    }

    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        debug_assert_eq!(self.stack.len(), 1, "unbalanced containers");
        let s = self.input.screen_size;
        let screen = Rect::new(0.0, 0.0, s.x, s.y);
        layout::solve(&mut self.nodes, 0, screen);

        self.draw.clear(screen);
        self.hits.clear();
        self.rects.clear();
        self.scroll_hits.clear();
        self.top_hits.clear();
        let mut painter = Painter { draw: &mut self.draw, fonts: &mut self.fonts, theme: &self.theme, font: self.font };
        let mut sink = HitSink {
            hits: &mut self.hits,
            rects: &mut self.rects,
            scroll_hits: &mut self.scroll_hits,
            top_hits: &mut self.top_hits,
        };
        paint(&mut self.nodes, 0, &mut painter, &mut sink);
        for overlay in self.overlays.drain(..) {
            overlay(&mut painter);
        }

        // Feed this frame's measured content back into scroll state.
        for n in &self.nodes {
            if n.scroll.is_some() {
                if let Some(st) = self.scroll_states.get_mut(&n.id) {
                    st.content = n.content;
                    st.viewport = n.rect.h;
                }
            }
        }

        // Tab / Shift+Tab cycles focus through text fields in build order.
        if let Some(back) = self.pending_tab.take() {
            let order = &self.focus_order;
            if !order.is_empty() {
                let pos = self.focused.and_then(|f| order.iter().position(|&i| i == f));
                let n = order.len();
                let next = match (pos, back) {
                    (Some(p), false) => (p + 1) % n,
                    (Some(p), true) => (p + n - 1) % n,
                    (None, false) => 0,
                    (None, true) => n - 1,
                };
                self.focused = Some(order[next]);
            }
        }

        if self.released {
            self.active = None;
        }
        let seen = &self.seen;
        self.anims.retain(|(id, _), _| seen.contains(id));
        self.text_states.retain(|id, _| seen.contains(id));
        self.scroll_states.retain(|id, _| seen.contains(id));
        if self.focused.is_some_and(|f| !seen.contains(&f)) {
            self.focused = None;
        }

        FrameOutput {
            draw: &self.draw,
            atlas: self.fonts.atlas(),
            screen_size: s,
            scale: self.input.scale,
            clear_color: self.theme.bg_app,
        }
    }

    // ---- building blocks for widgets -------------------------------------

    /// Stable id derived from the current container and `src`. Duplicates in
    /// the same container are disambiguated automatically.
    pub fn make_id(&mut self, src: impl Hash) -> Id {
        let parent = self.nodes[*self.stack.last().unwrap()].id;
        let base = parent.with(&src);
        let mut id = base;
        let mut n = 1u32;
        while !self.seen.insert(id) {
            id = base.with(n);
            n += 1;
        }
        id
    }

    /// Resolve hover/press/drag for `id` using last frame's rect.
    pub fn interact(&mut self, id: Id) -> Response {
        let rect = self.rects.get(&id).copied().unwrap_or_default();
        let hovered = self.hovered == Some(id) && (self.active.is_none() || self.active == Some(id));
        if hovered && self.pressed {
            self.active = Some(id);
        }
        let active = self.active == Some(id);
        Response {
            id,
            rect,
            hovered,
            active,
            pressed: hovered && self.pressed,
            clicked: active && hovered && self.released,
            drag_delta: if active { self.mouse_delta } else { Vec2::ZERO },
            scroll: if hovered { self.input.scroll } else { Vec2::ZERO },
            mouse_pos: self.input.mouse_pos,
        }
    }

    /// Retained animation value: eases towards `target` each frame.
    pub fn animate(&mut self, id: Id, slot: u8, target: f32) -> f32 {
        let k = 1.0 - (-self.theme.anim_speed * self.input.dt).exp();
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.001 {
            *v = target;
        }
        *v
    }

    /// Like [`Ui::animate`] with an explicit rate (1/s).
    pub fn animate_with_speed(&mut self, id: Id, slot: u8, target: f32, speed: f32) -> f32 {
        let k = 1.0 - (-speed * self.input.dt).exp();
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.01 {
            *v = target;
        }
        *v
    }

    /// Jump an animation to `value` (it then eases towards its next target).
    pub fn set_anim(&mut self, id: Id, slot: u8, value: f32) {
        self.anims.insert((id, slot), value);
    }

    /// Mark an explicitly-constructed id as alive this frame so its retained
    /// state (animations, text/scroll state) is kept.
    pub fn keep_id(&mut self, id: Id) {
        self.seen.insert(id);
    }

    /// Rect of `id` from the previous frame's layout.
    pub fn rect_of(&self, id: Id) -> Option<Rect> {
        self.rects.get(&id).copied()
    }

    /// Paint on top of everything, after the tree (drag previews, tooltips).
    pub fn overlay(&mut self, f: impl FnOnce(&mut Painter) + 'static) {
        self.overlays.push(Box::new(f));
    }

    /// Animation eased towards 1 when `on`, 0 otherwise.
    pub fn animate_bool(&mut self, id: Id, slot: u8, on: bool) -> f32 {
        self.animate(id, slot, if on { 1.0 } else { 0.0 })
    }

    fn attach(&mut self, node: Node) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(node);
        let parent = *self.stack.last().unwrap();
        self.nodes[parent].children.push(idx);
        idx
    }

    /// Add a leaf node. `content` is its intrinsic size excluding padding.
    pub fn add_leaf(
        &mut self,
        id: Id,
        layout: Layout,
        content: Vec2,
        interactive: bool,
        paint: impl FnOnce(&mut Painter, Rect) + 'static,
    ) {
        let opts = LeafOptions { interactive, ..Default::default() };
        self.add_leaf_ex(id, layout, content, opts, paint);
    }

    /// `add_leaf` with hit-area options.
    pub fn add_leaf_ex(
        &mut self,
        id: Id,
        layout: Layout,
        content: Vec2,
        opts: LeafOptions,
        paint: impl FnOnce(&mut Painter, Rect) + 'static,
    ) {
        self.seen.insert(id);
        let mut n = Node::new(id, layout);
        n.intrinsic = content;
        n.interactive = opts.interactive;
        n.hit_pad = opts.hit_pad;
        n.hit_top = opts.hit_top;
        n.paint = Some(Box::new(paint) as PaintFn);
        self.attach(n);
    }

    /// Generic container. Children added inside `body` are laid out by `layout`.
    pub fn container<R>(&mut self, layout: Layout, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        let idx = self.nodes[*self.stack.last().unwrap()].children.len();
        let id = self.make_id(("container", idx));
        self.container_id(id, layout, frame, body)
    }

    /// Container with an explicit id: its rect is queryable via `rect_of`, and
    /// children's ids derive from it, so their state follows it around.
    pub fn container_id<R>(&mut self, id: Id, layout: Layout, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.seen.insert(id);
        let mut n = Node::new(id, layout);
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 || frame.shadow {
            n.paint = Some(Box::new(move |p: &mut Painter, r: Rect| {
                if frame.shadow {
                    p.shadow(r.translate(0.0, 4.0), frame.radius, 16.0, p.theme.shadow);
                }
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        let i = self.attach(n);
        self.stack.push(i);
        let r = body(self);
        self.stack.pop();
        r
    }

    /// Vertical scroll area that fills the remaining height.
    pub fn scroll_area<R>(&mut self, key: &str, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.space;
        let opts = ScrollOptions { gap, ..ScrollOptions::new(Size::Grow(1.0)) };
        self.scroll_area_with(key, opts, body)
    }

    /// Vertical scroll area. Wheel/trackpad scrolls the innermost area under the
    /// mouse; the scrollbar thumb can be dragged, and clicking the track jumps.
    pub fn scroll_area_with<R>(&mut self, key: &str, opts: ScrollOptions, body: impl FnOnce(&mut Self) -> R) -> R {
        let id = self.make_id(("scroll", key));
        let bar_id = id.with("bar");
        self.seen.insert(bar_id);
        let mut st = self.scroll_states.get(&id).copied().unwrap_or_default();
        let max = (st.content - st.viewport).max(0.0);
        let at_end = st.target >= max - 1.0;

        let inside = self.scroll_target == Some(id);
        if inside && self.input.scroll.y != 0.0 {
            st.target -= self.input.scroll.y;
        }
        if opts.stick_to_end && at_end && max > 0.0 {
            st.target = f32::INFINITY;
        }

        let bar = self.interact(bar_id);
        let mut dragging = false;
        if max > 0.0 && bar.rect.h > 0.0 {
            let thumb_h = (bar.rect.h * st.viewport / st.content).max(24.0).min(bar.rect.h);
            let travel = (bar.rect.h - thumb_h).max(1.0);
            if bar.pressed {
                let thumb_y = bar.rect.y + travel * (st.offset / max);
                let on_thumb = bar.mouse_pos.y >= thumb_y && bar.mouse_pos.y <= thumb_y + thumb_h;
                if !on_thumb {
                    // Jump so the thumb centres on the click.
                    let t = (bar.mouse_pos.y - bar.rect.y - thumb_h * 0.5) / travel;
                    st.target = t.clamp(0.0, 1.0) * max;
                    st.offset = st.target;
                }
            }
            if bar.active {
                st.target += bar.drag_delta.y * max / travel;
                dragging = true;
            }
        }
        st.target = st.target.clamp(0.0, max);
        if dragging {
            st.offset = st.target;
        } else {
            let k = 1.0 - (-20.0 * self.input.dt).exp();
            st.offset += (st.target - st.offset) * k;
            if (st.target - st.offset).abs() < 0.5 {
                st.offset = st.target;
            }
        }
        st.offset = st.offset.clamp(0.0, max);
        self.scroll_states.insert(id, st);
        if bar.hovered || bar.active {
            self.cursor = Cursor::Default;
        }

        let visible = self.animate_bool(bar_id, 0, inside || bar.active);
        let hover = self.animate_bool(bar_id, 1, bar.hovered || bar.active);

        let layout = Layout::column().height(opts.height).gap(opts.gap).padding(opts.padding);
        let mut n = Node::new(id, layout);
        n.clip = true;
        n.scroll = Some(Scroll { offset: st.offset, bar_id, visible, hover });
        let i = self.attach(n);
        self.stack.push(i);
        let r = body(self);
        self.stack.pop();
        r
    }

    pub fn row<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.space;
        self.container(Layout::row().gap(gap), Frame::none(), body)
    }

    pub fn column<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.space;
        self.container(Layout::column().gap(gap).height(Size::Fit), Frame::none(), body)
    }
}

struct HitSink<'a> {
    hits: &'a mut Vec<(Id, Rect)>,
    top_hits: &'a mut Vec<(Id, Rect)>,
    rects: &'a mut HashMap<Id, Rect>,
    scroll_hits: &'a mut Vec<(Id, Rect)>,
}

fn paint(nodes: &mut [Node], i: usize, p: &mut Painter, sink: &mut HitSink) {
    let rect = nodes[i].rect;
    let id = nodes[i].id;
    sink.rects.insert(id, rect);
    let visible = p.draw.clip().intersect(&rect);
    if nodes[i].interactive {
        let pad = nodes[i].hit_pad;
        if let Some(r) = p.draw.clip().expand(pad).intersect(&rect.expand(pad)) {
            if nodes[i].hit_top {
                sink.top_hits.push((id, r));
            } else {
                sink.hits.push((id, r));
            }
        }
    }
    if let (Some(_), Some(r)) = (nodes[i].scroll, visible) {
        sink.scroll_hits.push((id, r));
    }
    if let Some(f) = nodes[i].paint.take() {
        f(p, rect);
    }
    let clip = nodes[i].clip;
    if clip {
        p.draw.push_clip(rect);
    }
    let children = std::mem::take(&mut nodes[i].children);
    for &c in &children {
        paint(nodes, c, p, sink);
    }
    nodes[i].children = children;
    if let Some(sc) = nodes[i].scroll {
        scrollbar(p, sink, rect, nodes[i].content, sc);
    }
    if clip {
        p.draw.pop_clip();
    }
}

/// Overlay scrollbar: thin, fades in while the mouse is over the area, widens on hover.
fn scrollbar(p: &mut Painter, sink: &mut HitSink, rect: Rect, content: f32, sc: Scroll) {
    let max = content - rect.h;
    if max <= 0.5 {
        return;
    }
    let track = Rect::new(rect.right() - 12.0, rect.y + 2.0, 12.0, rect.h - 4.0);
    if let Some(r) = p.draw.clip().intersect(&track) {
        sink.hits.push((sc.bar_id, r));
        sink.rects.insert(sc.bar_id, track);
    }
    let thumb_h = (track.h * rect.h / content).max(24.0).min(track.h);
    let y = track.y + (track.h - thumb_h) * (sc.offset / max).clamp(0.0, 1.0);
    let w = 4.0 + 3.0 * sc.hover;
    let thumb = Rect::new(track.right() - w - 3.0, y, w, thumb_h);
    let t = p.theme;
    let color = t.text_faint.lerp(t.text_muted, sc.hover).with_alpha(0.25 + 0.55 * sc.visible.max(sc.hover));
    p.rect(thumb, color, w * 0.5);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf"))
    }

    /// 20 buttons (30px + 0 gap) in a 100px scroll area at the top of the screen.
    fn build(ui: &mut Ui, input: Input) -> Vec<Response> {
        ui.begin_frame(input);
        let mut out = Vec::new();
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(100.0)), |ui| {
            for i in 0..20 {
                out.push(ui.button(&format!("item {i}")));
            }
        });
        let _ = ui.end_frame();
        out
    }

    #[test]
    fn wheel_scrolls_clamps_and_clips_hit_testing() {
        let mut ui = ui();
        let base = Input { mouse_inside: true, mouse_pos: Vec2::new(20.0, 50.0), dt: 1.0, ..Input::default() };
        build(&mut ui, base.clone());
        build(&mut ui, base.clone());

        // Scroll down 90px; dt = 1s so smoothing settles in one frame.
        let mut wheel = base.clone();
        wheel.scroll = Vec2::new(0.0, -90.0);
        build(&mut ui, wheel);
        let r = build(&mut ui, base.clone());
        assert_eq!(r[3].rect.y, 0.0, "item 3 now at the top");

        // Items scrolled out of the viewport are not hoverable/clickable.
        let mut hover_top = base.clone();
        hover_top.mouse_pos = Vec2::new(20.0, 5.0);
        let r = build(&mut ui, hover_top);
        assert!(r[3].hovered && !r[0].hovered && !r[2].hovered);

        // Over-scrolling clamps at content - viewport = 600 - 100.
        let mut far = base.clone();
        far.scroll = Vec2::new(0.0, -10_000.0);
        build(&mut ui, far);
        let r = build(&mut ui, base.clone());
        assert_eq!(r[19].rect.bottom(), 100.0);

        // Wheel outside the area does nothing.
        let mut outside = base.clone();
        outside.mouse_pos = Vec2::new(20.0, 300.0);
        outside.scroll = Vec2::new(0.0, 500.0);
        build(&mut ui, outside);
        let r = build(&mut ui, base);
        assert_eq!(r[19].rect.bottom(), 100.0);
    }
}
