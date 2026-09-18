use crate::layout::{self, Node, PaintFn, Scroll};
use crate::text_edit::TextState;
use crate::hash::{FxMap, FxSet};
use crate::input::UiEvent;
use crate::input_state::InputState;
use crate::{Atlas, Color, Cursor, DrawList, FontError, FontId, Fonts, FrameInfo, FrameInput, Gesture, Id, InputEvent, Insets, Key, Layout, Painter, PlatformOutput, PointerButton, PointerKind, Rect, Size, Theme, Vec2};
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
    /// Movement since last frame while active. Raw (unaccelerated) motion while
    /// the pointer is locked, otherwise the change in pointer position.
    pub drag_delta: Vec2,
    /// Raw `PointerDelta` motion this frame while active, if the host sends it.
    pub raw_delta: Option<Vec2>,
    /// Secondary (right) button pressed over this widget this frame.
    pub secondary_pressed: bool,
    /// Middle button pressed over this widget this frame.
    pub middle_pressed: bool,
    pub scroll: Vec2,
    pub mouse_pos: Vec2,
    /// Two-finger pinch over this widget: zoom ratio minus 1 (0 = none).
    pub pinch: f32,
    /// Two-finger pan over this widget.
    pub pan2: Vec2,
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
        Self { fill: t.panel.fill, border: t.panel.border, border_width: 1.0, radius: t.panel.radius, shadow: false, clip: true }
    }

    pub fn card(t: &Theme) -> Self {
        Self { fill: t.panel.fill, border: t.panel.border, border_width: 1.0, radius: t.metrics.radius_large, shadow: true, clip: true }
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
    /// Touch fling velocity (px/s).
    velocity: f32,
}

/// Everything a renderer needs for this frame.
pub struct FrameOutput<'a> {
    pub draw: &'a DrawList,
    pub atlas: &'a Atlas,
    pub screen_size: Vec2,
    pub scale: f32,
    pub clear_color: Color,
    /// Requests for the host: cursor, clipboard, keyboard/IME, pointer lock, repaint.
    pub platform: PlatformOutput,
}

pub struct Ui {
    pub theme: Theme,
    pub fonts: Fonts,
    pub font: FontId,
    /// Cursor requested by widgets this frame; apply it in the host.
    pub cursor: Cursor,
    pub(crate) input: FrameInput,
    input_state: InputState,
    /// Pointer lock requested this frame / granted for this frame.
    lock_request: bool,
    locked: bool,
    /// Something is still moving: ask the host for another frame.
    animating: bool,
    mouse_prev: Vec2,
    pub(crate) mouse_delta: Vec2,
    prev_down: bool,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
    nodes: Vec<Node>,
    stack: Vec<usize>,
    // Retained state
    rects: FxMap<Id, Rect>,
    hits: Vec<(Id, Rect)>,
    hovered: Option<Id>,
    active: Option<Id>,
    anims: FxMap<(Id, u8), f32>,
    seen: FxSet<Id>,
    /// Next free suffix per colliding base id, so N widgets sharing a key cost
    /// O(N) to disambiguate rather than O(N^2).
    dup_next: FxMap<Id, u32>,
    draw: DrawList,
    pub(crate) time: f64,
    // Keyboard focus
    pub(crate) focused: Option<Id>,
    pub(crate) focus_order: Vec<Id>,
    pending_tab: Option<bool>,
    pub(crate) text_states: FxMap<Id, TextState>,
    pub(crate) copied: Option<String>,
    pub(crate) ime_rect: Option<Rect>,
    // Scrolling
    scroll_states: FxMap<Id, ScrollState>,
    scroll_hits: Vec<(Id, Rect)>,
    scroll_target: Option<Id>,
    /// Hit rects that win over normal widgets (splitters).
    top_hits: Vec<(Id, Rect)>,
    overlays: Vec<Box<dyn FnOnce(&mut Painter)>>,
    // Touch
    /// Finger travel (logical px) before a tap turns into a scroll.
    pub touch_slop: f32,
    /// Fling deceleration (1/s): higher stops sooner.
    pub scroll_friction: f32,
    active_drag: bool,
    touch_press: Option<Vec2>,
    touch_candidate: Option<Id>,
    touch_scroll: Option<Id>,
    multi_lock: bool,
    prev_two: Option<(Vec2, f32)>,
    gesture: Gesture,
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
    /// Build a `Ui` with `font_bytes` as its default font. Fails if the bytes
    /// are not a readable font.
    pub fn new(theme: Theme, font_bytes: &[u8]) -> Result<Self, FontError> {
        let mut fonts = Fonts::new();
        let font = fonts.add_font(font_bytes)?;
        Ok(Self {
            theme,
            fonts,
            font,
            cursor: Cursor::Default,
            input: FrameInput::default(),
            input_state: InputState::new(),
            lock_request: false,
            locked: false,
            animating: false,
            mouse_prev: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            prev_down: false,
            pressed: false,
            released: false,
            nodes: Vec::new(),
            stack: Vec::new(),
            rects: FxMap::default(),
            hits: Vec::new(),
            hovered: None,
            active: None,
            anims: FxMap::default(),
            seen: FxSet::default(),
            dup_next: FxMap::default(),
            draw: DrawList::default(),
            time: 0.0,
            focused: None,
            focus_order: Vec::new(),
            pending_tab: None,
            text_states: FxMap::default(),
            copied: None,
            ime_rect: None,
            scroll_states: FxMap::default(),
            scroll_hits: Vec::new(),
            scroll_target: None,
            top_hits: Vec::new(),
            overlays: Vec::new(),
            touch_slop: 8.0,
            scroll_friction: 3.2,
            active_drag: false,
            touch_press: None,
            touch_candidate: None,
            touch_scroll: None,
            multi_lock: false,
            prev_two: None,
            gesture: Gesture::default(),
        })
    }

    /// Queue an input event; it is applied when the next frame begins. Call
    /// as events arrive, from any input source.
    pub fn push(&mut self, event: InputEvent) {
        self.input_state.push(event);
    }

    /// Use Apple-style shortcuts (Cmd; Option for words) instead of Ctrl.
    /// Defaults to the platform libgui was built for.
    pub fn set_mac_shortcuts(&mut self, mac: bool) {
        self.input_state.mac = mac;
    }

    /// This frame's input (after `begin_frame`).
    pub fn input(&self) -> &FrameInput {
        &self.input
    }

    /// True while the pointer is over, or dragging, UI: don't route it to the game.
    pub fn wants_pointer(&self) -> bool {
        self.active.is_some() || self.hovered.is_some()
    }

    /// True while a text field has focus: don't route keys to the game.
    pub fn wants_keyboard(&self) -> bool {
        self.focused.is_some()
    }

    /// `key` went down this frame (not a repeat). For shortcuts; check
    /// `wants_keyboard()` first if typing should win.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.input.keys_pressed.contains(&key)
    }

    pub fn key_down(&self, key: Key) -> bool {
        self.input.keys_down.contains(&key)
    }

    pub fn button_down(&self, button: PointerButton) -> bool {
        self.input.buttons_down[button.index()]
    }

    /// Ask for relative pointer mode this frame (hide + lock the cursor; drags
    /// use raw deltas). Typically while a viewport is being orbited.
    pub fn request_pointer_lock(&mut self) {
        // On the release frame the dragged widget is still `active` (so it can
        // report `clicked`); the lock must already end there.
        if !self.released {
            self.lock_request = true;
        }
    }

    /// Ask the host for another frame soon (custom animations).
    pub fn request_repaint(&mut self) {
        self.animating = true;
    }

    pub fn focused(&self) -> Option<Id> {
        self.focused
    }

    pub fn set_focus(&mut self, id: Option<Id>) {
        self.focused = id;
    }

    /// This frame's two-finger gesture (touch only).
    pub fn gesture(&self) -> Gesture {
        self.gesture
    }

    /// Derive the primary pointer and gestures from touches.
    fn apply_touches(&mut self, input: &mut FrameInput) {
        self.gesture = Gesture::default();
        let n = input.touches.len();
        if n >= 2 {
            let (a, b) = (input.touches[0].pos, input.touches[1].pos);
            let center = (a + b) * 0.5;
            let d = a - b;
            let dist = d.x.hypot(d.y).max(1.0);
            let (pan, zoom) = match self.prev_two {
                Some((pc, pd)) => (center - pc, dist / pd),
                None => (Vec2::ZERO, 1.0),
            };
            self.gesture = Gesture { active: true, center, pan, zoom };
            self.prev_two = Some((center, dist));
            // A second finger cancels single-finger interaction without clicking.
            self.multi_lock = true;
            self.active = None;
            self.active_drag = false;
            self.touch_scroll = None;
        } else {
            self.prev_two = None;
        }
        if n == 0 {
            self.multi_lock = false;
        }
        // After lifting, the pointer stays where the finger left (release lands there).
        input.mouse_pos = input.touches.first().map_or(self.mouse_prev, |t| t.pos);
        input.mouse_down = n == 1 && !self.multi_lock;
        // No hover on touch: "inside" only while touching (and on the release frame).
        input.mouse_inside = n > 0 || self.prev_down;
    }

    /// Start a frame: applies the events pushed since the last one.
    pub fn begin_frame(&mut self, info: FrameInfo) {
        let mut input = self.input_state.frame(info);
        self.locked = std::mem::take(&mut self.lock_request);
        self.animating = false;
        let touch = input.pointer_kind == PointerKind::Touch;
        if touch {
            self.apply_touches(&mut input);
        }
        self.pressed = input.mouse_down && !self.prev_down;
        self.released = !input.mouse_down && self.prev_down;
        self.prev_down = input.mouse_down;
        self.mouse_delta = if touch && self.pressed { Vec2::ZERO } else { input.mouse_pos - self.mouse_prev };
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
        if touch {
            // Tap vs. scroll: once a finger travels past the slop on something
            // that isn't a drag widget, the innermost scroll area takes over.
            if self.pressed {
                self.touch_press = Some(input.mouse_pos);
                self.touch_candidate = self.scroll_target;
                self.touch_scroll = None;
            }
            if input.mouse_down && self.touch_scroll.is_none() && !self.active_drag {
                if let (Some(p0), Some(c)) = (self.touch_press, self.touch_candidate) {
                    let d = input.mouse_pos - p0;
                    if d.x.hypot(d.y) > self.touch_slop {
                        self.touch_scroll = Some(c);
                        self.active = None;
                    }
                }
            }
            if !input.mouse_down {
                self.touch_scroll = None;
                self.touch_press = None;
            }
        }
        self.pending_tab = input.events.iter().rev().find_map(|e| match e {
            UiEvent::Key(Key::Tab, m) => Some(m.shift),
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
        self.dup_next.clear();
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
            self.active_drag = false;
        }
        let seen = &self.seen;
        self.anims.retain(|(id, _), _| seen.contains(id));
        self.text_states.retain(|id, _| seen.contains(id));
        self.scroll_states.retain(|id, _| seen.contains(id));
        if self.focused.is_some_and(|f| !seen.contains(&f)) {
            self.focused = None;
        }

        let busy = self.active.is_some() || self.touch_scroll.is_some() || self.animating;
        let platform = PlatformOutput {
            cursor: self.cursor,
            copied_text: self.copied.take(),
            paste_requested: self.input_state.paste_requested && self.focused.is_some(),
            text_input: self.focused.and(self.ime_rect),
            wants_pointer: self.wants_pointer(),
            wants_keyboard: self.wants_keyboard(),
            pointer_lock: self.lock_request,
            repaint_after: if busy {
                Some(0.0)
            } else if self.focused.is_some() {
                Some(0.5) // caret blink
            } else {
                None
            },
        };
        FrameOutput {
            draw: &self.draw,
            atlas: self.fonts.atlas(),
            screen_size: s,
            scale: self.input.scale,
            clear_color: self.theme.palette.bg_app,
            platform,
        }
    }

    // ---- building blocks for widgets -------------------------------------

    /// Stable id derived from the current container and `src`. Duplicates in
    /// the same container are disambiguated automatically, by build order: the
    /// k-th widget sharing a key gets `base.with(k - 1)`.
    ///
    /// Resolving a collision resumes from the last suffix handed out for that
    /// base. Rescanning from 1 each time made a container of N widgets sharing
    /// one key (`space`, `flex`, `separator`, or a list of equal labels)
    /// quadratic: 2000 spacers cost 25 ms/frame.
    pub fn make_id(&mut self, src: impl Hash) -> Id {
        let parent = self.nodes[*self.stack.last().unwrap()].id;
        let base = parent.with(&src);
        if self.seen.insert(base) {
            return base;
        }
        let mut n = *self.dup_next.get(&base).unwrap_or(&1);
        let mut id = base.with(n);
        // Loops only on a genuine hash collision with an unrelated id.
        while !self.seen.insert(id) {
            n += 1;
            id = base.with(n);
        }
        self.dup_next.insert(base, n + 1);
        id
    }

    /// Resolve hover/press/click for `id` using last frame's rect. On touch,
    /// dragging past the slop scrolls instead (and cancels the click).
    pub fn interact(&mut self, id: Id) -> Response {
        self.interact_sense(id, false)
    }

    /// Like `interact`, for widgets that own drags (sliders, splitters,
    /// viewports, text selection): on touch they keep the finger instead of
    /// letting the surrounding scroll area take it.
    pub fn interact_drag(&mut self, id: Id) -> Response {
        self.interact_sense(id, true)
    }

    fn interact_sense(&mut self, id: Id, drag: bool) -> Response {
        let rect = self.rects.get(&id).copied().unwrap_or_default();
        let hovered = self.hovered == Some(id) && (self.active.is_none() || self.active == Some(id));
        if hovered && self.pressed {
            self.active = Some(id);
            self.active_drag = drag;
        }
        let over = self.gesture.active && rect.contains(self.gesture.center);
        let active = self.active == Some(id);
        let over_now = self.hovered == Some(id);
        let delta = match (self.locked, self.input.raw_delta) {
            (true, Some(raw)) => raw,
            _ => self.mouse_delta,
        };
        Response {
            id,
            rect,
            hovered,
            active,
            pressed: hovered && self.pressed,
            clicked: active && hovered && self.released,
            drag_delta: if active { delta } else { Vec2::ZERO },
            raw_delta: if active { self.input.raw_delta } else { None },
            secondary_pressed: over_now && self.input.buttons_pressed[PointerButton::Secondary.index()],
            middle_pressed: over_now && self.input.buttons_pressed[PointerButton::Middle.index()],
            scroll: if hovered { self.input.scroll } else { Vec2::ZERO },
            mouse_pos: self.input.mouse_pos,
            pinch: if over { self.gesture.zoom - 1.0 } else { 0.0 },
            pan2: if over { self.gesture.pan } else { Vec2::ZERO },
        }
    }

    /// Retained animation value: eases towards `target` each frame.
    pub fn animate(&mut self, id: Id, slot: u8, target: f32) -> f32 {
        let k = 1.0 - (-self.theme.metrics.anim_speed * self.input.dt).exp();
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.001 {
            *v = target;
        }
        let v = *v;
        self.animating |= v != target;
        v
    }

    /// Like [`Ui::animate`] with an explicit rate (1/s).
    pub fn animate_with_speed(&mut self, id: Id, slot: u8, target: f32, speed: f32) -> f32 {
        let k = 1.0 - (-speed * self.input.dt).exp();
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.01 {
            *v = target;
        }
        let v = *v;
        self.animating |= v != target;
        v
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
                    p.shadow(r.translate(0.0, 4.0), frame.radius, 16.0, p.theme.palette.shadow);
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

    /// A floating layer at `rect` (window coordinates), drawn and hit-tested
    /// above everything built before it: in-app windows, popovers, palettes.
    pub fn layer<R>(&mut self, id: Id, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.seen.insert(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.absolute = Some(rect);
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 || frame.shadow {
            n.paint = Some(Box::new(move |p: &mut Painter, r: Rect| {
                if frame.shadow {
                    p.shadow(r.translate(0.0, 8.0), frame.radius, 28.0, p.theme.palette.shadow);
                }
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        // Layers hang off the root so they sit above all flow content.
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.nodes[0].children.push(idx);
        self.stack.push(idx);
        let r = body(self);
        self.stack.pop();
        r
    }

    /// Leaf at an absolute rect inside the current container (resize grips, badges).
    pub fn add_leaf_at(&mut self, id: Id, rect: Rect, opts: LeafOptions, paint: impl FnOnce(&mut Painter, Rect) + 'static) {
        self.seen.insert(id);
        let mut n = Node::new(id, Layout::leaf(Size::Fixed(rect.w), Size::Fixed(rect.h)));
        n.absolute = Some(rect);
        n.interactive = opts.interactive;
        n.hit_pad = opts.hit_pad;
        n.hit_top = opts.hit_top;
        n.paint = Some(Box::new(paint) as PaintFn);
        self.attach(n);
    }

    /// Vertical scroll area that fills the remaining height.
    pub fn scroll_area<R>(&mut self, key: &str, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.metrics.space;
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
            st.velocity = 0.0;
        }
        let dt = self.input.dt.max(1e-4);
        let touching = self.touch_scroll == Some(id);
        let mut direct = false;
        if touching {
            // Content follows the finger 1:1; remember velocity for the fling.
            let dy = self.mouse_delta.y;
            st.target = (st.target - dy).clamp(0.0, max);
            st.velocity = st.velocity + (-dy / dt - st.velocity) * 0.4;
            direct = true;
        } else if st.velocity.abs() > 5.0 {
            st.target += st.velocity * dt;
            st.velocity *= (-self.scroll_friction * dt).exp();
            if st.target <= 0.0 || st.target >= max {
                st.velocity = 0.0;
            }
            direct = true;
        } else {
            st.velocity = 0.0;
        }
        if opts.stick_to_end && at_end && max > 0.0 {
            st.target = f32::INFINITY;
        }

        let bar = self.interact_drag(bar_id);
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
        if dragging || direct {
            st.offset = st.target;
        } else {
            let k = 1.0 - (-20.0 * self.input.dt).exp();
            st.offset += (st.target - st.offset) * k;
            if (st.target - st.offset).abs() < 0.5 {
                st.offset = st.target;
            }
        }
        st.offset = st.offset.clamp(0.0, max);
        self.animating |= st.offset != st.target || st.velocity != 0.0;
        self.scroll_states.insert(id, st);
        if bar.hovered || bar.active {
            self.cursor = Cursor::Default;
        }

        let visible = self.animate_bool(bar_id, 0, inside || bar.active);
        let hover = self.animate_bool(bar_id, 1, bar.hovered || bar.active);

        let layout = Layout::column().height(opts.height).gap(opts.gap).padding(opts.padding);
        let mut n = Node::new(id, layout);
        n.clip = true;
        n.scroll = Some(Scroll { offset: st.offset, bar_id, visible, hover, style: self.theme.scrollbar });
        let i = self.attach(n);
        self.stack.push(i);
        let r = body(self);
        self.stack.pop();
        r
    }

    pub fn row<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.metrics.space;
        self.container(Layout::row().gap(gap), Frame::none(), body)
    }

    pub fn column<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        let gap = self.theme.metrics.space;
        self.container(Layout::column().gap(gap).height(Size::Fit), Frame::none(), body)
    }
}

struct HitSink<'a> {
    hits: &'a mut Vec<(Id, Rect)>,
    top_hits: &'a mut Vec<(Id, Rect)>,
    rects: &'a mut FxMap<Id, Rect>,
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
    // Flow children first, then absolute ones on top.
    for pass in [false, true] {
        for &c in &children {
            if nodes[c].absolute.is_some() == pass {
                paint(nodes, c, p, sink);
            }
        }
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
    let s = sc.style;
    let w = s.width + (s.width_hover - s.width) * sc.hover;
    let thumb = Rect::new(track.right() - w - 3.0, y, w, thumb_h);
    let alpha = s.rest_alpha + (1.0 - s.rest_alpha) * sc.visible.max(sc.hover) * 0.8;
    let color = s.thumb.lerp(s.thumb_hover, sc.hover);
    p.rect(thumb, color.with_alpha(color.a * alpha), w * 0.5);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap()
    }

    /// Mouse state for one test frame, pushed as events.
    #[derive(Clone)]
    struct M {
        mouse_pos: Vec2,
        mouse_inside: bool,
        scroll: Vec2,
        dt: f32,
    }

    impl Default for M {
        fn default() -> Self {
            M { mouse_pos: Vec2::ZERO, mouse_inside: false, scroll: Vec2::ZERO, dt: 1.0 / 60.0 }
        }
    }

    /// 20 buttons (30px + 0 gap) in a 100px scroll area at the top of the screen.
    fn build(ui: &mut Ui, m: M) -> Vec<Response> {
        if m.mouse_inside {
            ui.push(InputEvent::PointerMoved { pos: m.mouse_pos });
        }
        if m.scroll != Vec2::ZERO {
            ui.push(InputEvent::Wheel { delta: m.scroll, unit: crate::WheelUnit::Pixel });
        }
        ui.begin_frame(FrameInfo { dt: m.dt, ..FrameInfo::default() });
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
        let base = M { mouse_inside: true, mouse_pos: Vec2::new(20.0, 50.0), dt: 1.0, ..M::default() };
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

    /// `segmented`'s thumb position is retained across frames, so it can point
    /// past the end of a shorter option list on the next one.
    #[test]
    fn segmented_survives_a_shrinking_option_list() {
        let mut ui = ui();
        let mut sel = 3usize;
        let frame = |ui: &mut Ui, sel: &mut usize, options: &[&str]| {
            ui.begin_frame(FrameInfo { dt: 1.0, ..FrameInfo::default() });
            ui.segmented("mode", sel, options);
            let _ = ui.end_frame();
        };
        for _ in 0..4 {
            frame(&mut ui, &mut sel, &["a", "b", "c", "d"]);
        }
        // Drawing must not panic on the shorter list. Note the widget does not
        // write the clamp back: `sel` is still the caller's out-of-range value.
        frame(&mut ui, &mut sel, &["a", "b"]);
        assert_eq!(sel, 3);
        frame(&mut ui, &mut sel, &["a", "b"]);
        // An empty list draws the frame and nothing else.
        frame(&mut ui, &mut sel, &[]);
    }

    /// Widgets sharing a key are disambiguated by build order. The fast path
    /// for that must hand out exactly the ids the naive rescan did: same
    /// sequence, all distinct, identical from frame to frame.
    #[test]
    fn duplicate_keys_get_stable_distinct_ids() {
        let mut ui = ui();
        let ids = |ui: &mut Ui| -> Vec<Id> {
            ui.begin_frame(FrameInfo::default());
            let v: Vec<Id> = (0..64).map(|_| ui.make_id("same")).collect();
            let _ = ui.end_frame();
            v
        };
        let a = ids(&mut ui);
        let b = ids(&mut ui);
        assert_eq!(a, b, "ids are stable across frames");
        assert_eq!(a.iter().collect::<std::collections::HashSet<_>>().len(), 64, "all distinct");

        // The documented scheme: base, then base.with(1), base.with(2), ...
        let root = Id::new("root");
        let base = root.with("same");
        assert_eq!(a[0], base);
        assert_eq!(a[1], base.with(1u32));
        assert_eq!(a[63], base.with(63u32));

        // A key used once still gets the bare base id.
        ui.begin_frame(FrameInfo::default());
        assert_eq!(ui.make_id("solo"), root.with("solo"));
        let _ = ui.end_frame();
    }

    #[test]
    fn bad_font_bytes_are_an_error_not_a_panic() {
        let Err(e) = Ui::new(Theme::dark(), b"not a font") else { panic!("expected an error") };
        assert!(e.to_string().contains("invalid font data"), "{e}");
    }

    /// A font size larger than the atlas must not abort: the glyph is dropped
    /// but keeps its advance, so measurement and layout stay consistent.
    #[test]
    fn oversized_glyphs_are_skipped_not_fatal() {
        let mut ui = ui();
        let wide = ui.fonts.measure(ui.font, 4000.0, "WW").x;
        assert!(wide > 0.0, "advances still measure");
        ui.begin_frame(FrameInfo::default());
        ui.text_with("WW", 4000.0, Color::WHITE);
        let out = ui.end_frame();
        assert!(out.draw.instances.is_empty(), "nothing is drawn for an unplaceable glyph");
        // Normal text still renders afterwards: the atlas was not left broken.
        ui.begin_frame(FrameInfo::default());
        ui.text_with("ok", 13.0, Color::WHITE);
        let out = ui.end_frame();
        assert!(!out.draw.instances.is_empty());
    }

    /// The fingers wanted on screen this frame (finger id = index).
    fn touch(points: &[(f32, f32)]) -> Vec<Vec2> {
        points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
    }

    /// Push Start/Move/End events turning last frame's fingers into `now`.
    fn push_fingers(ui: &mut Ui, now: &[Vec2]) {
        use crate::TouchPhase::*;
        let prev = ui.input().touches.clone();
        for (i, &pos) in now.iter().enumerate() {
            let phase = if prev.iter().any(|t| t.id == i as u64) { Move } else { Start };
            ui.push(InputEvent::Touch { id: i as u64, phase, pos });
        }
        for t in prev.iter().filter(|t| t.id as usize >= now.len()) {
            ui.push(InputEvent::Touch { id: t.id, phase: End, pos: t.pos });
        }
    }

    /// A 100px scroll area of 20 buttons, then a slider below it.
    fn build_touch(ui: &mut Ui, fingers: Vec<Vec2>, slider: &mut f32) -> (Vec<Response>, Response) {
        push_fingers(ui, &fingers);
        ui.begin_frame(FrameInfo::default());
        let mut out = Vec::new();
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(100.0)), |ui| {
            for i in 0..20 {
                out.push(ui.button(&format!("item {i}")));
            }
        });
        let s = ui.slider("s", slider, 0.0, 1.0);
        let _ = ui.end_frame();
        (out, s)
    }

    #[test]
    fn touch_tap_clicks_but_drag_scrolls_without_clicking() {
        let mut ui = ui();
        let mut v = 0.5;
        build_touch(&mut ui, touch(&[]), &mut v);
        build_touch(&mut ui, touch(&[]), &mut v);

        // Tap item 1 (y 30..60): down, up -> click.
        build_touch(&mut ui, touch(&[(20.0, 45.0)]), &mut v);
        let (r, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert!(r[1].clicked, "tap clicks");
        let (r, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert!(!r[1].hovered, "no hover left behind after lifting the finger");

        // Press item 1 and drag up 60px: the list scrolls and nothing clicks.
        build_touch(&mut ui, touch(&[(20.0, 45.0)]), &mut v);
        for y in [40.0, 30.0, 15.0, 0.0, -15.0] {
            build_touch(&mut ui, touch(&[(20.0, y)]), &mut v);
        }
        let (r, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert!(r.iter().all(|b| !b.clicked), "drag must not click");
        let scrolled = -r[0].rect.y;
        assert!(scrolled > 40.0, "content followed the finger ({scrolled})");

        // Fling: it keeps going after release, then settles.
        let (r, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert!(-r[0].rect.y > scrolled, "momentum continues");
        for _ in 0..240 {
            build_touch(&mut ui, touch(&[]), &mut v);
        }
        let (a, _) = build_touch(&mut ui, touch(&[]), &mut v);
        let (b, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert_eq!(a[0].rect.y, b[0].rect.y, "fling settles");
    }

    #[test]
    fn touch_slider_keeps_the_finger_and_pinch_cancels() {
        let mut ui = ui();
        let mut v = 0.0;
        build_touch(&mut ui, touch(&[]), &mut v);
        build_touch(&mut ui, touch(&[]), &mut v);
        let (_, s) = build_touch(&mut ui, touch(&[]), &mut v);
        let y = s.rect.center().y;
        build_touch(&mut ui, touch(&[(s.rect.x + 10.0, y)]), &mut v);
        build_touch(&mut ui, touch(&[(s.rect.right() - 10.0, y + 40.0)]), &mut v);
        assert!(v > 0.9, "slider follows the finger even off its row: {v}");
        build_touch(&mut ui, touch(&[]), &mut v);

        // Pinch: second finger cancels the tap on item 0; gesture reports zoom.
        build_touch(&mut ui, touch(&[(20.0, 15.0)]), &mut v);
        build_touch(&mut ui, touch(&[(20.0, 15.0), (60.0, 15.0)]), &mut v);
        build_touch(&mut ui, touch(&[(10.0, 15.0), (70.0, 15.0)]), &mut v);
        let g = ui.gesture();
        assert!(g.active && (g.zoom - 1.5).abs() < 0.01, "{g:?}");
        build_touch(&mut ui, touch(&[(10.0, 15.0)]), &mut v);
        let (r, _) = build_touch(&mut ui, touch(&[]), &mut v);
        assert!(!r[0].clicked, "a pinch never clicks");
    }
}
