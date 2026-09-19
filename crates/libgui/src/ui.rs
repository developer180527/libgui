use crate::layout::{self, Node, PaintFn, Scroll};
use crate::text_edit::TextState;
use crate::hash::{FxMap, FxSet};
use crate::input::UiEvent;
use crate::input_state::InputState;
use crate::{Align, Axis, Atlas, Color, Cursor, DrawList, FontId, Fonts, FrameInfo, FrameInput, Gesture, Id, InputEvent, Insets, Key, Layout, Painter, PlatformOutput, PointerButton, PointerKind, Rect, Shortcut, Size, Theme, Transform, Vec2};
use std::hash::Hash;

/// Paint callback run after the tree (drag previews, tooltips).
type OverlayFn = Box<dyn FnOnce(&mut Painter)>;
use std::ops::Range;

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
    pub width: Size,
    pub height: Size,
    /// Scroll sideways. Off by default: a column of rows that each fill the
    /// width should not suddenly be scrollable because one is wide.
    pub scroll_x: bool,
    pub scroll_y: bool,
    pub gap: f32,
    pub padding: Insets,
    /// Keep following the end while scrolled to the bottom (logs, consoles).
    pub stick_to_end: bool,
}

impl ScrollOptions {
    /// Vertical scrolling, filling the width.
    pub fn new(height: Size) -> Self {
        Self {
            width: Size::Grow(1.0),
            height,
            scroll_x: false,
            scroll_y: true,
            gap: 0.0,
            padding: Insets::all(0.0),
            stick_to_end: false,
        }
    }

    /// Scrolls both ways: timelines, wide tables, large canvases of widgets.
    pub fn both(width: Size, height: Size) -> Self {
        Self { width, scroll_x: true, ..Self::new(height) }
    }

    /// Sideways only.
    pub fn horizontal(width: Size, height: Size) -> Self {
        Self { width, scroll_x: true, scroll_y: false, ..Self::new(height) }
    }
}

/// Where a virtual list's row heights come from.
enum Heights<'a> {
    /// Every row the same: locating a row is arithmetic, O(1) for any length.
    Uniform(f32),
    /// Per row: locating a row means summing the ones above it, O(rows).
    PerRow(&'a dyn Fn(usize) -> f32),
}

/// The visible window of a virtual list, in rows and in pixels.
struct Span {
    first: usize,
    /// Top of row `first`, i.e. the space the rows above it occupy.
    first_y: f32,
    end: usize,
    /// Top of row `end`.
    end_y: f32,
    /// Sum over every row of `height + gap` (so one trailing gap too many).
    stride_total: f32,
}

/// Overscan is bounded so the backward search can use a fixed-size ring.
const MAX_OVERSCAN: usize = 8;

impl Heights<'_> {
    fn at(&self, i: usize) -> f32 {
        match self {
            Heights::Uniform(h) => *h,
            Heights::PerRow(f) => f(i).max(0.0),
        }
    }

    fn locate(&self, rows: usize, gap: f32, offset: f32, viewport: f32, overscan: usize) -> Span {
        let overscan = overscan.min(MAX_OVERSCAN);
        match *self {
            Heights::Uniform(h) => {
                let pitch = (h + gap).max(0.5);
                let first = (offset / pitch).floor().max(0.0) as usize;
                let first = first.saturating_sub(overscan).min(rows);
                let span = (viewport / pitch).ceil() as usize + 1 + 2 * overscan;
                let end = first.saturating_add(span).min(rows);
                Span {
                    first,
                    first_y: first as f32 * pitch,
                    end,
                    end_y: end as f32 * pitch,
                    stride_total: rows as f32 * pitch,
                }
            }
            Heights::PerRow(f) => {
                // One pass over the heights. Rows above the viewport are added
                // up, never built; `ring` remembers the last few tops so the
                // overscan can step back without a second pass.
                let bottom = offset + viewport;
                let mut ring = [0.0f32; MAX_OVERSCAN + 1];
                let (mut first, mut first_y) = (usize::MAX, 0.0);
                let (mut want_end, mut end, mut end_y) = (usize::MAX, usize::MAX, 0.0);
                let mut y = 0.0f32;
                for i in 0..rows {
                    ring[i % ring.len()] = y;
                    if i == want_end {
                        end = i;
                        end_y = y;
                    }
                    let h = f(i).max(0.0);
                    if first == usize::MAX && y + h > offset {
                        first = i.saturating_sub(overscan);
                        first_y = ring[first % ring.len()];
                    }
                    if want_end == usize::MAX && first != usize::MAX && y >= bottom {
                        want_end = (i + overscan).min(rows);
                        if i == want_end {
                            end = i;
                            end_y = y;
                        }
                    }
                    y += h + gap;
                }
                // Scrolled past the end, or the list never filled the viewport.
                if first == usize::MAX {
                    first = rows;
                    first_y = y;
                }
                if end == usize::MAX {
                    end = rows;
                    end_y = y;
                }
                Span { first, first_y, end, end_y, stride_total: y }
            }
        }
    }
}

/// Pan/zoom state of a [`Ui::canvas`]. The app owns it, so it can be saved,
/// animated, or driven from somewhere else entirely.
#[derive(Clone, Copy, Debug)]
pub struct CanvasState {
    /// Offset of the canvas origin from the canvas widget's top-left, in
    /// window pixels.
    pub pan: Vec2,
    pub zoom: f32,
    pub min_zoom: f32,
    pub max_zoom: f32,
    /// Wheel zooms (node editors) rather than scrolls (timelines).
    pub wheel_zooms: bool,
    /// The part of the canvas on screen, in canvas coordinates. Written each
    /// frame: use it to skip building what cannot be seen.
    pub visible: Rect,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            min_zoom: 0.1,
            max_zoom: 8.0,
            wheel_zooms: true,
            visible: Rect::default(),
        }
    }
}

impl CanvasState {
    /// Zoom by `factor`, keeping the canvas point under `window_pos` still.
    pub fn zoom_at(&mut self, window_pos: Vec2, origin: Vec2, factor: f32) {
        let before = self.zoom;
        let after = (before * factor).clamp(self.min_zoom, self.max_zoom);
        if after == before {
            return;
        }
        // Solve for the pan that leaves the same canvas point under the cursor.
        let local = window_pos - origin;
        let canvas = Vec2::new((local.x - self.pan.x) / before, (local.y - self.pan.y) / before);
        self.pan = Vec2::new(local.x - canvas.x * after, local.y - canvas.y * after);
        self.zoom = after;
    }
}

/// What a [`Ui::canvas`] body needs to know about the view it is drawing into.
/// Passed in, because the canvas borrows its [`CanvasState`] for the call.
#[derive(Clone, Copy, Debug)]
pub struct CanvasView {
    /// The part of the canvas on screen, in canvas coordinates: cull with it.
    pub visible: Rect,
    /// Canvas pixels to window pixels. Divide by it for hairlines that stay
    /// one pixel wide however far you zoom in.
    pub zoom: f32,
    /// The full canvas-to-window mapping.
    pub xform: Transform,
}

/// Options for [`Ui::virtual_list_with`].
#[derive(Clone, Copy, Debug)]
pub struct ListOptions {
    /// Height of the list area itself (not of the content).
    pub height: Size,
    /// Every row is exactly this tall. Rows are clipped to it, so the library
    /// can know where row `n` is without building rows `0..n`.
    pub row_height: f32,
    /// Vertical space between rows.
    pub gap: f32,
    pub padding: Insets,
    /// Rows built above and below the viewport. The viewport is measured one
    /// frame late, so a little slack keeps a fast fling from showing a gap.
    pub overscan: usize,
    /// Keep following the end while scrolled to the bottom (logs, consoles).
    pub stick_to_end: bool,
}

impl ListOptions {
    pub fn new(row_height: f32) -> Self {
        Self {
            height: Size::Grow(1.0),
            row_height,
            gap: 0.0,
            padding: Insets::all(0.0),
            overscan: 2,
            stick_to_end: false,
        }
    }
}

/// One axis of a scroll area.
#[derive(Clone, Copy, Debug, Default)]
struct ScrollAxis {
    target: f32,
    offset: f32,
    content: f32,
    viewport: f32,
    /// Touch fling velocity (px/s).
    velocity: f32,
}

impl ScrollAxis {
    fn max(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// Wheel, fling and smoothing for one axis. Returns whether the offset
    /// should track the target exactly this frame (a finger, a trackpad),
    /// rather than easing towards it (a wheel notch).
    ///
    /// `wheel` is the whole wheel delta; `precise` is the part of it that came
    /// in pixels. Easing a trackpad's already-smooth stream only adds lag.
    fn update(&mut self, wheel: f32, precise: f32, touch: Option<f32>, friction: f32, dt: f32) -> bool {
        let max = self.max();
        if wheel != 0.0 {
            self.target -= wheel;
            self.velocity = 0.0;
        }
        let mut direct = precise != 0.0 && wheel == precise;
        match touch {
            Some(d) => {
                self.target = (self.target - d).clamp(0.0, max);
                self.velocity += (-d / dt - self.velocity) * 0.4;
                direct = true;
            }
            None if self.velocity.abs() > 5.0 => {
                self.target += self.velocity * dt;
                self.velocity *= (-friction * dt).exp();
                if self.target <= 0.0 || self.target >= max {
                    self.velocity = 0.0;
                }
                direct = true;
            }
            None => self.velocity = 0.0,
        }
        direct
    }

    fn settle(&mut self, direct: bool, dt: f32) {
        let max = self.max();
        self.target = self.target.clamp(0.0, max);
        if direct {
            self.offset = self.target;
        } else {
            let k = 1.0 - (-20.0 * dt).exp();
            self.offset += (self.target - self.offset) * k;
            if (self.target - self.offset).abs() < 0.5 {
                self.offset = self.target;
            }
        }
        self.offset = self.offset.clamp(0.0, max);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ScrollState {
    x: ScrollAxis,
    y: ScrollAxis,
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
    /// Active `with_key` scopes as (salt, container depth at push).
    key_salt: Vec<(Id, usize)>,
    /// Shortcut scopes: a shortcut only fires if every enclosing scope is active.
    shortcut_scopes: Vec<bool>,
    /// Keys already claimed this frame, so one press drives one command.
    consumed_keys: Vec<Key>,
    /// A text field had focus when the frame began: its editing keys are its own.
    typing: bool,
    /// The open popup chain: a root menu, then its submenus. Retained.
    open_chain: Vec<Id>,
    /// Anchor rect each open popup was opened against (pointer or widget).
    open_anchors: FxMap<Id, Rect>,
    /// The click-away sheet has been emitted this frame.
    sheet_done: bool,
    /// Popups currently being built, innermost last.
    popup_stack: Vec<Id>,
    /// Canvases currently being built: the composed canvas-to-window transform.
    xform_stack: Vec<Transform>,
    /// (widget, time the pointer arrived) for the tooltip delay.
    hover_since: Option<(Id, f64)>,
    /// Fitted size of each floating node, measured last frame. A popup sizes
    /// itself to its content, but its rect is also what positions it, so the
    /// content size has to come from the previous frame's measure.
    layer_min: FxMap<Id, Vec2>,
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
    overlays: Vec<OverlayFn>,
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

/// Stacking order for floating content. Within one layer, build order decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// In-app floating windows and torn-off dock panels.
    #[default]
    Window,
    /// Menus, popups and context menus, and the invisible sheet that closes
    /// them when you click away.
    Popup,
    /// Tooltips: above popups, and never interactive.
    Tooltip,
    /// Drag previews: above everything.
    Drag,
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
    #[cfg(feature = "fontdue")]
    pub fn new(theme: Theme, font_bytes: &[u8]) -> Result<Self, crate::FontError> {
        let mut fonts = Fonts::new();
        let font = fonts.add_font(font_bytes)?;
        Ok(Self::with_fonts(theme, fonts, font))
    }

    /// Build a `Ui` whose default font is your own [`crate::FontRasterizer`].
    pub fn with_rasterizer(theme: Theme, rasterizer: Box<dyn crate::FontRasterizer>) -> Self {
        let mut fonts = Fonts::new();
        let font = fonts.add_rasterizer(rasterizer);
        Self::with_fonts(theme, fonts, font)
    }

    fn with_fonts(theme: Theme, fonts: Fonts, font: FontId) -> Self {
        Self {
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
            key_salt: Vec::new(),
            shortcut_scopes: Vec::new(),
            consumed_keys: Vec::new(),
            typing: false,
            open_chain: Vec::new(),
            open_anchors: FxMap::default(),
            sheet_done: false,
            popup_stack: Vec::new(),
            xform_stack: Vec::new(),
            hover_since: None,
            layer_min: FxMap::default(),
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
        }
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

    /// Claim a shortcut for this frame.
    ///
    /// Returns true at most once per press: the first caller wins, so one key
    /// cannot drive two commands. Refuses while a text field has focus and the
    /// shortcut is one the field handles itself (`Delete` edits text, but
    /// `Cmd+S` still saves), and refuses inside a
    /// [`shortcut_scope`](Ui::shortcut_scope) that is not active.
    ///
    /// libgui supplies no bindings: what `Cmd+S` means is your app's keymap.
    ///
    /// ```ignore
    /// if ui.consume_shortcut(Shortcut::command(Key::S)) { save(); }
    /// if ui.consume_shortcut(Shortcut::command(Key::Z).shift()) { redo(); }
    /// ```
    pub fn consume_shortcut(&mut self, sc: Shortcut) -> bool {
        if !self.shortcut_scopes.iter().all(|&a| a) {
            return false;
        }
        if self.typing && sc.is_text_editing() {
            return false;
        }
        if !self.input.keys_pressed.contains(&sc.key) || self.consumed_keys.contains(&sc.key) {
            return false;
        }
        if !sc.matches(&self.input.modifiers, self.input_state.mac) {
            return false;
        }
        self.consumed_keys.push(sc.key);
        true
    }

    // ---- popups ----------------------------------------------------------

    /// Open `id` as a root popup, anchored to `anchor` (a widget's rect, or a
    /// zero-size rect at the pointer for a context menu). Closes any other.
    pub fn open_popup(&mut self, id: Id, anchor: Rect) {
        self.open_chain.clear();
        self.open_chain.push(id);
        self.open_anchors.insert(id, anchor);
    }

    /// Open `id` as a child of `parent`, keeping `parent` open (a submenu).
    pub fn open_child_popup(&mut self, parent: Id, id: Id, anchor: Rect) {
        match self.open_chain.iter().position(|&p| p == parent) {
            Some(i) => self.open_chain.truncate(i + 1),
            None => return,
        }
        self.open_chain.push(id);
        self.open_anchors.insert(id, anchor);
    }

    pub fn popup_open(&self, id: Id) -> bool {
        self.open_chain.contains(&id)
    }

    /// True while any popup is open.
    pub fn any_popup_open(&self) -> bool {
        !self.open_chain.is_empty()
    }

    /// Close every open popup.
    pub fn close_popups(&mut self) {
        self.open_chain.clear();
    }

    /// Close `id` and anything it opened.
    pub fn close_popup(&mut self, id: Id) {
        if let Some(i) = self.open_chain.iter().position(|&p| p == id) {
            self.open_chain.truncate(i);
        }
    }

    /// The invisible full-window sheet under the open popups: it stops clicks
    /// reaching the UI behind them, and a press on it closes them. Emitted once
    /// per frame, by whichever popup is shown first.
    fn popup_sheet(&mut self) {
        if self.sheet_done || self.open_chain.is_empty() {
            return;
        }
        self.sheet_done = true;
        let id = Id::new("popup_sheet");
        self.seen.insert(id);
        let s = self.input.screen_size;
        let rect = Rect::new(0.0, 0.0, s.x, s.y);
        let resp = self.interact(id);
        // A press, not a click: the release that opened the popup must not also
        // close it, and pressing away should dismiss immediately.
        if resp.pressed || self.input.buttons_pressed[PointerButton::Secondary.index()] && resp.hovered {
            self.close_popups();
        }
        let mut n = Node::new(id, Layout::leaf(Size::Fixed(rect.w), Size::Fixed(rect.h)));
        n.absolute = Some(rect);
        n.z = Layer::Popup;
        n.interactive = true;
        n.paint = Some(Box::new(|_: &mut Painter, _: Rect| {}) as crate::layout::PaintFn);
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.nodes[0].children.push(idx);
    }

    /// Show `body` in a popup panel if `id` is open, positioned near its anchor
    /// and kept on screen. Returns whether it was shown.
    ///
    /// Open it with [`Ui::open_popup`]; it closes on a click outside, on Escape,
    /// or when you call [`Ui::close_popups`] (what a menu item does).
    pub fn popup<R>(&mut self, id: Id, min_width: f32, body: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if !self.popup_open(id) {
            return None;
        }
        self.popup_sheet();
        if self.input.keys_pressed.contains(&Key::Escape) {
            // Innermost first: Escape backs out one level.
            if self.open_chain.last() == Some(&id) {
                self.open_chain.pop();
                return None;
            }
        }
        let anchor = self.open_anchors.get(&id).copied().unwrap_or_default();
        // The panel sizes itself to its content, but its rect is also what
        // positions it, so the content size comes from last frame's measure.
        // On the first frame it opens at `min_width` and settles on the next,
        // the same one-frame rule as `Response::rect`.
        let fitted = self.layer_min.get(&id).copied().unwrap_or(Vec2::new(min_width, 0.0));
        let rect = self.place_popup(anchor, Vec2::new(fitted.x.max(min_width), fitted.y));
        let s = self.theme.menu;
        let (pad, gap) = (s.padding, s.gap);

        self.seen.insert(id);
        let layout = Layout::column().width(Size::Fit).height(Size::Fit).padding(pad).gap(gap);
        let mut n = Node::new(id, layout);
        n.absolute = Some(rect);
        n.z = Layer::Popup;
        n.clip = true;
        n.paint = Some(Box::new(move |p: &mut Painter, r: Rect| {
            p.shadow(r.translate(0.0, 6.0), s.radius, 24.0, p.theme.palette.shadow);
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
        }) as crate::layout::PaintFn);
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.nodes[0].children.push(idx);

        self.popup_stack.push(id);
        self.stack.push(idx);
        let r = body(self);
        self.stack.pop();
        self.popup_stack.pop();
        Some(r)
    }

    /// How long the pointer has rested on `id`, in seconds. 0 if it is not there.
    pub fn hover_time(&self, id: Id) -> f32 {
        match self.hover_since {
            Some((h, t)) if h == id => (self.time - t) as f32,
            _ => 0.0,
        }
    }

    /// Show `text` beside the pointer once it has rested on `resp`'s widget.
    ///
    /// Tooltips sit above popups and are never interactive, so they cannot
    /// swallow the click they are describing. Nothing is shown while a menu is
    /// open, or while a drag is in progress.
    pub fn tooltip(&mut self, resp: &Response, text: &str) {
        if text.is_empty() || !resp.hovered || self.any_popup_open() || self.active.is_some() {
            return;
        }
        let s = self.theme.tooltip;
        if self.hover_time(resp.id) < s.delay {
            // Ask for the frame that will cross the delay, or an idle UI would
            // never wake up to show it.
            self.request_repaint();
            return;
        }
        let size = self.theme.metrics.font_size;
        let m = self.fonts.measure(self.font, size, text);
        let pad = s.padding;
        let w = m.x + pad.left + pad.right;
        let h = m.y + pad.top + pad.bottom;
        let screen = self.input.screen_size;
        let p = self.input.mouse_pos;
        // Below-right of the pointer, flipped and clamped to stay on screen.
        let x = (p.x + 14.0).min(screen.x - w - 4.0).max(4.0);
        let below = p.y + 20.0;
        let y = if below + h + 4.0 <= screen.y { below } else { (p.y - h - 8.0).max(4.0) };
        let rect = Rect::new(x, y, w, h);

        let id = resp.id.with("tooltip");
        self.seen.insert(id);
        let text = text.to_string();
        let mut n = Node::new(id, Layout::leaf(Size::Fixed(w), Size::Fixed(h)));
        n.absolute = Some(rect);
        n.z = Layer::Tooltip;
        n.paint = Some(Box::new(move |p: &mut Painter, r: Rect| {
            p.shadow(r.translate(0.0, 3.0), s.radius, 12.0, p.theme.palette.shadow);
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
            p.text_left(r.shrink(pad.left, 0.0, pad.right, 0.0), size, s.text, &text);
        }) as crate::layout::PaintFn);
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.nodes[0].children.push(idx);
    }

    /// The popup whose body is being built, if any. A submenu opens as a child
    /// of this one, so the parent stays open.
    pub fn enclosing_popup(&self) -> Option<Id> {
        self.popup_stack.last().copied()
    }

    /// Moving onto a plain item closes any submenu opened from the same menu,
    /// which is what makes a menu feel right when the pointer slides down it.
    pub(crate) fn close_sibling_submenus(&mut self, _item: Id) {
        let Some(parent) = self.enclosing_popup() else { return };
        if let Some(i) = self.open_chain.iter().position(|&p| p == parent) {
            self.open_chain.truncate(i + 1);
        }
    }

    /// Put a popup of `size` next to `anchor`, flipping and clamping so it stays
    /// on screen: below the anchor if it fits, otherwise above.
    fn place_popup(&self, anchor: Rect, size: Vec2) -> Rect {
        let screen = self.input.screen_size;
        let m = 4.0;
        let below = anchor.bottom() + 1.0;
        let above = anchor.y - size.y - 1.0;
        let y = if below + size.y + m <= screen.y || above < m { below } else { above };
        let x = anchor.x.min(screen.x - size.x - m).max(m);
        let y = y.min(screen.y - size.y - m).max(m);
        Rect::new(x, y, size.x, size.y)
    }

    /// Limit the shortcuts declared inside `body` to when `active` is true.
    ///
    /// Scopes nest, and an inactive one disables everything within it, so the
    /// same key can mean different things in different panels. Dock panels are
    /// wrapped in one automatically, active when that pane has focus.
    pub fn shortcut_scope<R>(&mut self, active: bool, body: impl FnOnce(&mut Self) -> R) -> R {
        self.shortcut_scopes.push(active);
        let r = body(self);
        self.shortcut_scopes.pop();
        r
    }

    /// True while an enclosing [`Ui::shortcut_scope`] is inactive.
    pub fn shortcuts_blocked(&self) -> bool {
        !self.shortcut_scopes.iter().all(|&a| a)
    }

    /// How a shortcut should read in a menu on this platform: `⌘S` or `Ctrl+S`.
    pub fn shortcut_label(&self, sc: Shortcut) -> String {
        sc.label(self.input_state.mac)
    }

    /// `key` went down this frame (not a repeat).
    ///
    /// Raw and unrouted: it ignores consumption, focus and scopes, so it will
    /// be true even while the user is typing into a text field. Use it for
    /// held-key state (a viewport's fly controls, gated on
    /// [`wants_keyboard`](Ui::wants_keyboard)); use
    /// [`consume_shortcut`](Ui::consume_shortcut) for commands.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.input.keys_pressed.contains(&key)
    }

    pub fn key_down(&self, key: Key) -> bool {
        self.input.keys_down.contains(&key)
    }

    pub fn button_down(&self, button: PointerButton) -> bool {
        self.input.buttons_down[button.index()]
    }

    /// `button` went down this frame. Raw and unrouted, like
    /// [`Ui::key_pressed`].
    pub fn button_pressed(&self, button: PointerButton) -> bool {
        self.input.buttons_pressed[button.index()]
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
        // Dwell time for tooltips: reset whenever the pointer moves to something else.
        let now = self.time;
        match (self.hovered, self.hover_since) {
            (Some(h), Some((prev, _))) if prev == h => {}
            (Some(h), _) => self.hover_since = Some((h, now)),
            (None, _) => self.hover_since = None,
        }
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
        self.key_salt.clear();
        self.shortcut_scopes.clear();
        self.consumed_keys.clear();
        // Focus is resolved during a frame, so this is last frame's answer —
        // the same one-frame-late rule the rest of the input model uses.
        self.typing = self.focused.is_some();
        self.sheet_done = false;
        self.popup_stack.clear();
        self.xform_stack.clear();
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
        // Floating nodes size themselves from their content, which `measure`
        // has just worked out; keep it for next frame's placement.
        self.layer_min.clear();
        for n in &self.nodes {
            if n.absolute.is_some() {
                self.layer_min.insert(n.id, n.min);
            }
        }

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
                    st.x.content = n.content.x;
                    st.x.viewport = n.rect.w;
                    st.y.content = n.content.y;
                    st.y.viewport = n.rect.h;
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
        let parent = self.nodes[*self.stack.last().expect("libgui: widget built outside begin_frame/end_frame")].id;
        // A `with_key` scope salts the widgets built directly inside it. Nested
        // containers inherit it through their own (already salted) id, so the
        // salt is mixed in exactly once.
        let base = match self.key_salt.last() {
            Some(&(salt, depth)) if depth == self.stack.len() => parent.with(salt.0).with(&src),
            _ => parent.with(&src),
        };
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

    /// Give everything built inside `body` a distinct identity.
    ///
    /// Widget ids come from the label, and duplicates in one container are
    /// separated by *build order*, so hiding the first of two same-labelled
    /// widgets hands its id — and its animation, focus and drag state — to the
    /// second. Wrap each item in a key that does not move:
    ///
    /// ```ignore
    /// for obj in &objects {
    ///     ui.with_key(obj.id, |ui| {
    ///         ui.selectable(&obj.name, obj.id == selected);   // two "Mesh" rows stay distinct
    ///         if obj.removable { ui.button("Delete"); }
    ///     });
    /// }
    /// ```
    ///
    /// This applies to custom widgets too, since they derive ids with
    /// [`Ui::make_id`]. For a single widget, the `*_keyed` variants
    /// ([`Ui::button_keyed`] and friends) are shorter.
    pub fn with_key<R>(&mut self, key: impl Hash, body: impl FnOnce(&mut Self) -> R) -> R {
        let depth = self.stack.len();
        // Nested scopes in the same container combine rather than shadow.
        let salt = match self.key_salt.last() {
            Some(&(prev, d)) if d == depth => prev.with(&key),
            _ => Id::new(&key),
        };
        self.key_salt.push((salt, depth));
        let r = body(self);
        self.key_salt.pop();
        r
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

    /// Canvas-to-window transform where the UI is currently being built.
    pub fn xform(&self) -> Transform {
        self.xform_stack.last().copied().unwrap_or(Transform::IDENTITY)
    }

    fn interact_sense(&mut self, id: Id, drag: bool) -> Response {
        let rect = self.rects.get(&id).copied().unwrap_or_default();
        let hovered = self.hovered == Some(id) && (self.active.is_none() || self.active == Some(id));
        if hovered && self.pressed {
            self.active = Some(id);
            self.active_drag = drag;
        }
        // A widget inside a canvas works in canvas coordinates: its rect, the
        // pointer and its drag deltas are all in the space it was built in, so
        // app logic is the same at any zoom.
        let t = self.xform();
        let over = self.gesture.active && rect.contains(t.inv_point(self.gesture.center));
        let active = self.active == Some(id);
        let started = active && self.pressed;
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
            // Zero on the frame the drag starts: the pointer movement that
            // brought it onto the widget happened *before* the press, and with
            // a teleporting pointer (a pen, synthetic input) that jump is large.
            drag_delta: if active && !started { delta * (1.0 / t.zoom) } else { Vec2::ZERO },
            raw_delta: if active { self.input.raw_delta } else { None },
            secondary_pressed: over_now && self.input.buttons_pressed[PointerButton::Secondary.index()],
            middle_pressed: over_now && self.input.buttons_pressed[PointerButton::Middle.index()],
            scroll: if hovered { self.input.scroll } else { Vec2::ZERO },
            mouse_pos: t.inv_point(self.input.mouse_pos),
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

    /// A rect in physical pixels, rounded: what an embedded renderer should
    /// size its target to.
    ///
    /// ```ignore
    /// let vp = ui.viewport("scene", scene_texture, |_, _| {});
    /// let (w, h) = ui.physical_px(vp.rect);
    /// if (w, h) != scene.size() { scene.resize(w, h); }
    /// ```
    pub fn physical_px(&self, rect: Rect) -> (u32, u32) {
        let s = self.input.scale.max(0.01);
        (((rect.w * s).round().max(0.0)) as u32, ((rect.h * s).round().max(0.0)) as u32)
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
        let parent = *self.stack.last().expect("libgui: widget built outside begin_frame/end_frame");
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
        let idx = self.nodes[*self.stack.last().expect("libgui: widget built outside begin_frame/end_frame")].children.len();
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

    /// A floating layer at `rect`, drawn and hit-tested above everything built
    /// before it: in-app windows, popovers, palettes.
    ///
    /// A layer hangs off the **root**, not the current container, which is what
    /// puts it above everything — but it also means it ignores any enclosing
    /// [`Ui::canvas`]: its rect is in window coordinates and it is not clipped
    /// to the container it was written inside. That is right for a menu, which
    /// should not scale with a canvas's zoom. For a box positioned *within* the
    /// current container — a node in a graph, a clip on a timeline — use
    /// [`Ui::container_at`].
    pub fn layer<R>(&mut self, id: Id, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.layer_in(id, Layer::Window, rect, frame, body)
    }

    /// [`Ui::layer`] in an explicit stacking [`Layer`].
    pub fn layer_in<R>(&mut self, id: Id, z: Layer, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.seen.insert(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.absolute = Some(rect);
        n.z = z;
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

    /// A container at an explicit rect **inside the current container**, so it
    /// follows any enclosing [`Ui::canvas`]'s pan and zoom and is clipped to it.
    ///
    /// This is what positions content on a canvas: a node in a graph, a clip on
    /// a timeline, a key on a curve editor. Unlike [`Ui::layer_in`] it does not
    /// escape to the root; unlike [`Ui::add_leaf_at`] it can hold widgets.
    ///
    /// ```ignore
    /// ui.canvas("graph", &mut view, |ui, _| {
    ///     ui.container_at(node_id, node.rect, frame, |ui| {
    ///         ui.slider("Amount", &mut node.amount, 0.0, 1.0);
    ///     });
    /// });
    /// ```
    ///
    /// Siblings stack in build order, or by [`Layer`] with
    /// [`Ui::container_at_in`].
    pub fn container_at<R>(&mut self, id: Id, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.container_at_in(id, Layer::Window, rect, frame, body)
    }

    /// [`Ui::container_at`] with an explicit stacking [`Layer`] among its
    /// positioned siblings.
    pub fn container_at_in<R>(&mut self, id: Id, z: Layer, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.seen.insert(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.absolute = Some(rect);
        n.z = z;
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
        self.scroll_area_id(id, opts, body)
    }

    /// A scroll area with an id the caller already made (so it can read the
    /// area's retained state first, as [`Ui::virtual_list_with`] does).
    fn scroll_area_id<R>(&mut self, id: Id, opts: ScrollOptions, body: impl FnOnce(&mut Self) -> R) -> R {
        let bar_y = id.with("bar");
        let bar_x = id.with("bar_x");
        self.seen.insert(bar_y);
        self.seen.insert(bar_x);
        let mut st = self.scroll_states.get(&id).copied().unwrap_or_default();
        let at_end = st.y.target >= st.y.max() - 1.0;

        let inside = self.scroll_target == Some(id);
        let dt = self.input.dt.max(1e-4);
        let touching = self.touch_scroll == Some(id);
        let friction = self.scroll_friction;

        // A wheel with no sideways component still scrolls sideways when the
        // area only scrolls that way, which is what a trackpad user expects
        // on a timeline.
        let (wheel, precise) = if inside { (self.input.scroll, self.input.scroll_precise) } else { (Vec2::ZERO, Vec2::ZERO) };
        let sideways = opts.scroll_x && !opts.scroll_y && wheel.x == 0.0;
        let (wheel_x, precise_x) = if sideways { (wheel.y, precise.y) } else { (wheel.x, precise.x) };
        let touch = touching.then_some(self.mouse_delta);

        let mut direct_x = false;
        let mut direct_y = false;
        if opts.scroll_x {
            direct_x = st.x.update(wheel_x, precise_x, touch.map(|d| d.x), friction, dt);
        }
        if opts.scroll_y {
            direct_y = st.y.update(wheel.y, precise.y, touch.map(|d| d.y), friction, dt);
        }
        if opts.stick_to_end && at_end && st.y.max() > 0.0 {
            st.y.target = f32::INFINITY;
        }

        let by = self.interact_drag(bar_y);
        let bx = self.interact_drag(bar_x);
        direct_y |= drag_bar(&mut st.y, &by, Axis::Y);
        direct_x |= drag_bar(&mut st.x, &bx, Axis::X);

        st.x.settle(direct_x, self.input.dt);
        st.y.settle(direct_y, self.input.dt);
        self.animating |= st.x.offset != st.x.target || st.x.velocity != 0.0;
        self.animating |= st.y.offset != st.y.target || st.y.velocity != 0.0;
        self.scroll_states.insert(id, st);
        if by.hovered || by.active || bx.hovered || bx.active {
            self.cursor = Cursor::Default;
        }

        let vis_y = self.animate_bool(bar_y, 0, inside || by.active);
        let hot_y = self.animate_bool(bar_y, 1, by.hovered || by.active);
        let vis_x = self.animate_bool(bar_x, 0, inside || bx.active);
        let hot_x = self.animate_bool(bar_x, 1, bx.hovered || bx.active);

        let layout = Layout::column()
            .width(opts.width)
            .height(opts.height)
            .gap(opts.gap)
            .padding(opts.padding);
        let scale = self.input.scale.max(0.01);
        let snap = |v: f32| (v * scale).round() / scale;
        let mut n = Node::new(id, layout);
        n.clip = true;
        n.scroll = Some(Scroll {
            // Whole physical pixels: text snaps its baseline to the pixel
            // grid, so a fractional offset would slide row boxes under their
            // labels and the two would jitter against each other.
            offset: Vec2::new(snap(st.x.offset), snap(st.y.offset)),
            scroll_x: opts.scroll_x,
            scroll_y: opts.scroll_y,
            bar_x,
            bar_y,
            vis_x,
            hot_x,
            vis_y,
            hot_y,
            style: self.theme.scrollbar,
        });
        let i = self.attach(n);
        self.stack.push(i);
        let r = body(self);
        self.stack.pop();
        r
    }

    /// Scrolling list that only builds the rows you can see.
    ///
    /// Cost is proportional to the *visible* rows, not to `rows`, so a list of
    /// a million items costs the same as a list of fifty. The price is that
    /// every row must be exactly `row_height` tall: that is what lets the
    /// library place row `n` without having built rows `0..n`. For rows that
    /// differ in height, see [`Ui::virtual_rows`].
    ///
    /// ```ignore
    /// ui.virtual_list("objects", scene.len(), 24.0, |ui, i| {
    ///     if ui.selectable_keyed(i, &scene[i].name, i == selected).clicked { selected = i; }
    /// });
    /// ```
    ///
    /// Returns the range that was built. Rows are identified by index, so if
    /// your list can reorder or filter, wrap the body in [`Ui::with_key`] with
    /// something stable from the item itself.
    pub fn virtual_list(&mut self, key: &str, rows: usize, row_height: f32, row: impl FnMut(&mut Self, usize)) -> Range<usize> {
        self.virtual_list_with(key, rows, ListOptions::new(row_height), row)
    }

    /// [`Ui::virtual_list`] with explicit options.
    pub fn virtual_list_with(
        &mut self,
        key: &str,
        rows: usize,
        opts: ListOptions,
        mut row: impl FnMut(&mut Self, usize),
    ) -> Range<usize> {
        let h = opts.row_height;
        self.virtual_impl(key, rows, opts, &Heights::Uniform(h), &mut row)
    }

    /// [`Ui::virtual_list`] for rows that differ in height.
    ///
    /// `height(i)` must return the same value for the same `i` within a frame,
    /// and must not depend on whether the row was built. Unlike the uniform
    /// case, locating the first visible row costs one `height` call per row
    /// (**O(rows) per frame**, though only the visible rows are *built*): fine
    /// into the tens of thousands, but prefer [`Ui::virtual_list`] when the
    /// rows really are uniform and the list is huge.
    pub fn virtual_rows(
        &mut self,
        key: &str,
        rows: usize,
        height: impl Fn(usize) -> f32,
        row: impl FnMut(&mut Self, usize),
    ) -> Range<usize> {
        self.virtual_rows_with(key, rows, ListOptions::new(0.0), height, row)
    }

    /// [`Ui::virtual_rows`] with explicit options. `ListOptions::row_height` is
    /// ignored here: `height` supplies it.
    pub fn virtual_rows_with(
        &mut self,
        key: &str,
        rows: usize,
        opts: ListOptions,
        height: impl Fn(usize) -> f32,
        mut row: impl FnMut(&mut Self, usize),
    ) -> Range<usize> {
        self.virtual_impl(key, rows, opts, &Heights::PerRow(&height), &mut row)
    }

    fn virtual_impl(
        &mut self,
        key: &str,
        rows: usize,
        opts: ListOptions,
        heights: &Heights<'_>,
        row: &mut dyn FnMut(&mut Self, usize),
    ) -> Range<usize> {
        let id = self.make_id(("scroll", key));
        let fallback_viewport = self.input.screen_size.y;
        let scroll = ScrollOptions {
            height: opts.height,
            gap: opts.gap,
            padding: opts.padding,
            stick_to_end: opts.stick_to_end,
            ..ScrollOptions::new(opts.height)
        };
        let mut built = 0..0;
        self.scroll_area_id(id, scroll, |ui| {
            // Read the state *inside*, so the range comes from this frame's
            // offset (the one layout will use) rather than last frame's.
            let st = ui.scroll_states.get(&id).copied().unwrap_or_default();
            // The viewport is measured at the end of a frame, so it is 0 on the
            // first one: fall back to the window rather than building nothing.
            let viewport = if st.y.viewport > 1.0 { st.y.viewport } else { fallback_viewport };
            let span = heights.locate(rows, opts.gap, st.y.offset, viewport, opts.overscan);
            built = span.first..span.end;

            // Spacers stand in for the rows that were not built, so layout, the
            // scrollbar and the scroll maths still see the whole list. A row
            // occupies `height + gap`; the spacer replaces `n` of those and the
            // gap that follows it supplies the last one.
            if span.first > 0 {
                ui.list_spacer(0, span.first_y - opts.gap);
            }
            for i in built.clone() {
                // Keyed by index, not by position among the built rows, so a
                // row keeps its identity as the window slides over it.
                let row_id = ui.make_id(("vlist_row", i));
                let layout = Layout::row()
                    .width(Size::Grow(1.0))
                    .height(Size::Fixed(heights.at(i)))
                    .align(Align::Start, Align::Center)
                    .shrink();
                ui.container_id(row_id, layout, Frame { clip: true, ..Frame::none() }, |ui| row(ui, i));
            }
            if span.end < rows {
                ui.list_spacer(1, span.stride_total - span.end_y - opts.gap);
            }
        });
        built
    }

    /// Invisible stand-in for the rows a virtual list did not build.
    fn list_spacer(&mut self, slot: u8, height: f32) {
        let id = self.make_id(("vlist_pad", slot));
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(height.max(0.0)));
        self.add_leaf(id, layout, Vec2::ZERO, false, |_, _| {});
    }

    /// A pan/zoom canvas: an unbounded coordinate space for node graphs,
    /// timelines, piano rolls, curve editors — anything where the content has
    /// its own coordinates and the user moves a viewport over it.
    ///
    /// Ordinary widgets work inside it. They lay out, hit-test and report
    /// their rect, the pointer and drag deltas in **canvas coordinates**, so
    /// app logic is identical at any zoom, and text is rasterised at the zoomed
    /// resolution rather than scaled up.
    ///
    /// ```ignore
    /// ui.canvas("graph", &mut view, |ui, view| {
    ///     for node in graph.nodes_in(view.visible) {        // cull with `visible`
    ///         ui.node_panel(node.id, node.rect, |ui| { /* real widgets */ });
    ///     }
    /// });
    /// ```
    ///
    /// Wheel zooms toward the pointer and the middle button pans, unless the
    /// app drives [`CanvasState`] itself. Returns the background's response:
    /// `clicked` there means "clicked empty canvas".
    pub fn canvas<R>(&mut self, key: &str, st: &mut CanvasState, body: impl FnOnce(&mut Self, CanvasView) -> R) -> (Response, R) {
        let id = self.make_id(("canvas", key));
        let bg_id = id.with("bg");
        // The canvas's own position is last frame's; pan and zoom are the app's
        // and apply immediately. Position only moves when the layout does.
        let area = self.rect_of(id).unwrap_or_default();
        let origin = Vec2::new(area.x, area.y);

        let bg = self.interact_drag(bg_id);
        if bg.hovered {
            let wheel = self.input.scroll;
            if st.wheel_zooms {
                if wheel.y != 0.0 {
                    let pos = self.input.mouse_pos;
                    st.zoom_at(pos, origin, (wheel.y * 0.0015).exp());
                }
            } else {
                st.pan += wheel;
            }
        }
        // Middle-drag pans; so does a primary drag on empty canvas.
        if bg.active {
            st.pan += bg.drag_delta * st.zoom;
        }
        if bg.hovered && self.input.buttons_down[PointerButton::Middle.index()] {
            st.pan += self.mouse_delta;
        }
        // Touch: two fingers anywhere over the canvas (even on its widgets) pan
        // and pinch-zoom around their midpoint, so what is under the fingers
        // stays under them. The gesture is in window space, like `area`.
        let g = self.gesture;
        if g.active && area.contains(g.center) {
            st.pan += g.pan;
            if g.zoom != 1.0 {
                st.zoom_at(g.center, origin, g.zoom);
            }
        }
        st.zoom = st.zoom.clamp(st.min_zoom, st.max_zoom);

        let t = Transform::new(origin + st.pan, st.zoom);
        st.visible = t.inv_rect(area);
        let view = CanvasView { visible: st.visible, zoom: st.zoom, xform: t };
        let visible = st.visible;

        self.seen.insert(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.clip = true;
        n.xform = Some(t);
        let idx = self.attach(n);
        self.stack.push(idx);
        self.xform_stack.push(t);
        // Background first, so everything built after it wins the pointer.
        let opts = LeafOptions { interactive: true, ..Default::default() };
        self.add_leaf_at(bg_id, visible, opts, |_, _| {});
        let r = body(self, view);
        self.xform_stack.pop();
        self.stack.pop();
        (bg, r)
    }

    /// Draw and interact with `body` under an explicit [`Transform`]. The raw
    /// primitive behind [`Ui::canvas`], for a viewport you drive yourself.
    pub fn with_transform<R>(&mut self, id: Id, t: Transform, body: impl FnOnce(&mut Self) -> R) -> R {
        self.seen.insert(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.clip = true;
        n.xform = Some(t);
        let idx = self.attach(n);
        self.stack.push(idx);
        self.xform_stack.push(t);
        let r = body(self);
        self.xform_stack.pop();
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
    // Hit rects are compared against the pointer, so they are stored in window
    // space; `sink.rects` keeps the canvas-space rect a widget reports.
    let t = p.draw.xform();
    let win = t.rect(rect);
    let visible = p.draw.clip().intersect(&win);
    if nodes[i].interactive {
        let pad = nodes[i].hit_pad * t.zoom;
        if let Some(r) = p.draw.clip().expand(pad).intersect(&win.expand(pad)) {
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
    // Enter the canvas *after* clipping to its own (untransformed) rect.
    let xform = nodes[i].xform;
    if let Some(t) = xform {
        p.draw.push_xform(t);
        p.fonts.set_zoom(p.draw.xform().zoom);
    }
    let children = std::mem::take(&mut nodes[i].children);
    // Flow children first, then absolute ones on top of them.
    for &c in &children {
        if nodes[c].absolute.is_none() {
            paint(nodes, c, p, sink);
        }
    }
    // Absolute children stack by layer, and by build order within a layer, so a
    // menu is above a floating panel however early the panel was built.
    let mut floating: Vec<usize> = children.iter().copied().filter(|&c| nodes[c].absolute.is_some()).collect();
    if floating.len() > 1 {
        floating.sort_by_key(|&c| nodes[c].z);
    }
    for c in floating {
        paint(nodes, c, p, sink);
    }
    nodes[i].children = children;
    if xform.is_some() {
        p.draw.pop_xform();
        p.fonts.set_zoom(p.draw.xform().zoom);
    }
    if let Some(sc) = nodes[i].scroll {
        scrollbars(p, sink, rect, nodes[i].content, sc);
    }
    if clip {
        p.draw.pop_clip();
    }
}

/// Drag or click a scrollbar track. Returns true if the offset should follow
/// the target exactly this frame.
fn drag_bar(st: &mut ScrollAxis, bar: &Response, axis: Axis) -> bool {
    let max = st.max();
    let (track_len, track_start, pointer) = match axis {
        Axis::Y => (bar.rect.h, bar.rect.y, bar.mouse_pos.y),
        Axis::X => (bar.rect.w, bar.rect.x, bar.mouse_pos.x),
    };
    if max <= 0.0 || track_len <= 0.0 {
        return false;
    }
    let thumb = (track_len * st.viewport / st.content).max(24.0).min(track_len);
    let travel = (track_len - thumb).max(1.0);
    if bar.pressed {
        let at = track_start + travel * (st.offset / max);
        let on_thumb = pointer >= at && pointer <= at + thumb;
        if !on_thumb {
            // Jump so the thumb centres on the click.
            let t = (pointer - track_start - thumb * 0.5) / travel;
            st.target = t.clamp(0.0, 1.0) * max;
            st.offset = st.target;
        }
    }
    if bar.active {
        let d = match axis {
            Axis::Y => bar.drag_delta.y,
            Axis::X => bar.drag_delta.x,
        };
        st.target += d * max / travel;
        return true;
    }
    false
}

/// Overlay scrollbars: thin, fading in while the pointer is over the area and
/// widening under it. Drawn per axis, and only where the content overflows.
fn scrollbars(p: &mut Painter, sink: &mut HitSink, rect: Rect, content: Vec2, sc: Scroll) {
    if sc.scroll_y {
        bar(p, sink, rect, content.y, sc.offset.y, Axis::Y, sc.bar_y, sc.vis_y, sc.hot_y, sc.style);
    }
    if sc.scroll_x {
        bar(p, sink, rect, content.x, sc.offset.x, Axis::X, sc.bar_x, sc.vis_x, sc.hot_x, sc.style);
    }
}

#[allow(clippy::too_many_arguments)]
fn bar(
    p: &mut Painter,
    sink: &mut HitSink,
    rect: Rect,
    content: f32,
    offset: f32,
    axis: Axis,
    id: Id,
    visible: f32,
    hover: f32,
    s: crate::ScrollbarStyle,
) {
    let viewport = match axis {
        Axis::Y => rect.h,
        Axis::X => rect.w,
    };
    let max = content - viewport;
    if max <= 0.5 {
        return;
    }
    let track = match axis {
        Axis::Y => Rect::new(rect.right() - 12.0, rect.y + 2.0, 12.0, rect.h - 4.0),
        Axis::X => Rect::new(rect.x + 2.0, rect.bottom() - 12.0, rect.w - 4.0, 12.0),
    };
    if let Some(r) = p.draw.clip().intersect(&track) {
        sink.hits.push((id, r));
        sink.rects.insert(id, track);
    }
    let (track_len, along) = match axis {
        Axis::Y => (track.h, track.y),
        Axis::X => (track.w, track.x),
    };
    let thumb_len = (track_len * viewport / content).max(24.0).min(track_len);
    let at = along + (track_len - thumb_len) * (offset / max).clamp(0.0, 1.0);
    let w = s.width + (s.width_hover - s.width) * hover;
    let thumb = match axis {
        Axis::Y => Rect::new(track.right() - w - 3.0, at, w, thumb_len),
        Axis::X => Rect::new(at, track.bottom() - w - 3.0, thumb_len, w),
    };
    let alpha = s.rest_alpha + (1.0 - s.rest_alpha) * visible.max(hover) * 0.8;
    let color = s.thumb.lerp(s.thumb_hover, hover);
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

    /// Trackpad (pixel) deltas are already smooth, so the content follows them
    /// exactly; easing them again only made the list trail the fingers. Wheel
    /// notches (lines) still ease. Either way the applied offset is whole
    /// physical pixels, so row boxes and their pixel-snapped text move together.
    #[test]
    fn trackpad_scroll_is_exact_and_offsets_land_on_pixels() {
        let frame = |ui: &mut Ui, delta: f32, unit: crate::WheelUnit, scale: f32| -> f32 {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 50.0) });
            if delta != 0.0 {
                ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, delta), unit });
            }
            ui.begin_frame(FrameInfo { dt: 1.0 / 60.0, scale, ..FrameInfo::default() });
            let mut first = Rect::default();
            ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(100.0)), |ui| {
                for i in 0..40 {
                    let r = ui.button(&format!("item {i}"));
                    if i == 0 {
                        first = r.rect;
                    }
                }
            });
            let _ = ui.end_frame();
            first.y
        };
        use crate::WheelUnit::{Line, Pixel};

        // Trackpad: each frame's delta is applied in full, the same frame.
        let mut ui = ui();
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Pixel, 1.0)).last().unwrap();
        frame(&mut ui, -30.0, Pixel, 1.0);
        assert_eq!(frame(&mut ui, -30.0, Pixel, 1.0), y0 - 30.0, "trackpad scroll lagged");
        assert_eq!(frame(&mut ui, 0.0, Pixel, 1.0), y0 - 60.0, "trackpad scroll kept moving after the fingers stopped");
        assert_eq!(frame(&mut ui, 0.0, Pixel, 1.0), y0 - 60.0);

        // Wheel notch: eases in over several frames rather than jumping.
        let mut ui = self::ui();
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Line, 1.0)).last().unwrap();
        frame(&mut ui, -1.0, Line, 1.0);
        let first_step = y0 - frame(&mut ui, 0.0, Line, 1.0);
        assert!(first_step > 0.0 && first_step < 24.0, "a wheel notch should ease, moved {first_step}");

        // At a fractional DPI scale, every frame of the ease lands on a
        // physical pixel.
        let mut ui = self::ui();
        let scale = 1.5;
        for _ in 0..3 {
            frame(&mut ui, 0.0, Line, scale);
        }
        frame(&mut ui, -1.0, Line, scale);
        for _ in 0..20 {
            let y = frame(&mut ui, 0.0, Line, scale) * scale;
            assert!((y - y.round()).abs() < 1e-3, "row at {y} physical px, between pixels");
        }
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

    /// The footgun `with_key` and the `*_keyed` variants exist for: hiding a
    /// widget must not hand its identity (and its retained state) to the next
    /// one that happens to share a label.
    #[test]
    fn a_stable_key_survives_a_hidden_sibling() {
        let mut ui = ui();
        // Build order alone: the surviving button inherits the hidden one's id.
        let ids = |ui: &mut Ui, show_first: bool| -> Id {
            ui.begin_frame(FrameInfo::default());
            if show_first {
                let _ = ui.button("Delete");
            }
            let second = ui.button("Delete").id;
            let _ = ui.end_frame();
            second
        };
        let both = ids(&mut ui, true);
        let alone = ids(&mut ui, false);
        assert_ne!(both, alone, "unkeyed: the second button's id moved (the bug)");

        // With a key, the second widget keeps its identity either way.
        let keyed = |ui: &mut Ui, show_first: bool| -> Id {
            ui.begin_frame(FrameInfo::default());
            if show_first {
                let _ = ui.button_keyed("delete-selected", "Delete");
            }
            let second = ui.button_keyed("delete-all", "Delete").id;
            let _ = ui.end_frame();
            second
        };
        assert_eq!(keyed(&mut ui, true), keyed(&mut ui, false), "keyed: identity is stable");

        // `with_key` does the same for whole groups, custom widgets included.
        let scoped = |ui: &mut Ui, show_first: bool| -> Id {
            ui.begin_frame(FrameInfo::default());
            if show_first {
                ui.with_key(1u32, |ui| {
                    let _ = ui.button("Delete");
                });
            }
            let second = ui.with_key(2u32, |ui| ui.button("Delete").id);
            let _ = ui.end_frame();
            second
        };
        assert_eq!(scoped(&mut ui, true), scoped(&mut ui, false), "with_key: identity is stable");
    }

    /// Scopes must separate otherwise-identical subtrees, nest, and leave
    /// widgets outside them untouched.
    #[test]
    fn with_key_scopes_are_distinct_nested_and_bounded() {
        let mut ui = ui();
        ui.begin_frame(FrameInfo::default());
        let a = ui.with_key("row-a", |ui| ui.button("Rename").id);
        let b = ui.with_key("row-b", |ui| ui.button("Rename").id);
        assert_ne!(a, b, "different scopes, different ids");

        let nested = ui.with_key("outer", |ui| ui.with_key("inner", |ui| ui.button("X").id));
        let flat = ui.with_key("inner", |ui| ui.button("X").id);
        assert_ne!(nested, flat, "nested scopes combine rather than shadow");

        // A scope that ends leaves following widgets on the plain path.
        let outside = ui.button("Plain").id;
        let _ = ui.end_frame();
        ui.begin_frame(FrameInfo::default());
        let again = ui.button("Plain").id;
        let _ = ui.end_frame();
        assert_eq!(outside, again);
    }

    /// One frame of a virtual list: the range it built and where each built
    /// row landed.
    fn list_frame(ui: &mut Ui, rows: usize, opts: ListOptions, view: f32) -> (Range<usize>, Vec<(usize, Rect)>) {
        let mut rects = Vec::new();
        let h = opts.row_height;
        ui.begin_frame(FrameInfo::default());
        let opts = ListOptions { height: Size::Fixed(view), ..opts };
        let built = ui.virtual_list_with("objects", rows, opts, |ui, i| {
            let id = ui.make_id("cell");
            ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::ZERO, true, |_, _| {});
            if let Some(r) = ui.rect_of(id) {
                rects.push((i, r));
            }
        });
        let _ = ui.end_frame();
        (built, rects)
    }

    fn wheel(ui: &mut Ui, dy: f32) {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 100.0) });
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, dy), unit: crate::WheelUnit::Pixel });
    }

    /// A virtual list must be indistinguishable from a fully built one: rows on
    /// the same pitch, an honest content height (so the scrollbar is right),
    /// and the last row reachable and flush with the bottom.
    #[test]
    fn virtual_list_places_rows_like_a_full_list() {
        const ROWS: usize = 5_000;
        const VIEW: f32 = 400.0;
        let opts = ListOptions { gap: 4.0, ..ListOptions::new(20.0) };
        let pitch = opts.row_height + opts.gap;
        let mut ui = ui();
        list_frame(&mut ui, ROWS, opts, VIEW);
        let (built, rects) = list_frame(&mut ui, ROWS, opts, VIEW);

        assert!(built.len() < 40, "built {} rows for a {VIEW}px viewport", built.len());
        assert_eq!(built.start, 0, "at the top, the first built row is row 0");
        assert_eq!(rects[0].0, 0);
        assert!(rects[0].1.y.abs() < 0.01, "row 0 starts at {}", rects[0].1.y);
        let step = rects[1].1.y - rects[0].1.y;
        assert!((step - pitch).abs() < 0.01, "row pitch is {step}, expected {pitch}");

        // Run to the very bottom (content is ROWS * pitch tall), then let the
        // scroll smoothing settle.
        let mut last = (built, rects);
        for _ in 0..300 {
            wheel(&mut ui, -1000.0);
            last = list_frame(&mut ui, ROWS, opts, VIEW);
        }
        for _ in 0..30 {
            last = list_frame(&mut ui, ROWS, opts, VIEW);
        }
        let (built, rects) = last;
        let (last_i, last_r) = *rects.last().unwrap();
        assert_eq!(last_i, ROWS - 1, "could not reach the end of the list");
        assert!((last_r.bottom() - VIEW).abs() < 1.0, "last row ends at {} not {VIEW}", last_r.bottom());
        assert!(built.len() < 40, "still only a screenful at the bottom: {}", built.len());

        // ...and back to the top, landing exactly on row 0.
        let mut last = (built, rects);
        for _ in 0..300 {
            wheel(&mut ui, 1000.0);
            last = list_frame(&mut ui, ROWS, opts, VIEW);
        }
        for _ in 0..30 {
            last = list_frame(&mut ui, ROWS, opts, VIEW);
        }
        let (_, rects) = last;
        assert_eq!(rects[0].0, 0);
        assert!(rects[0].1.y.abs() < 0.01, "back at the top, row 0 is at {}", rects[0].1.y);
    }

    /// Variable row heights: rows must land at their true cumulative offsets,
    /// the content height must be honest, and the last row must be reachable.
    #[test]
    fn virtual_rows_place_variable_heights_correctly() {
        const ROWS: usize = 2_000;
        const VIEW: f32 = 300.0;
        const GAP: f32 = 3.0;
        // Heights cycle so the pattern is irregular but exactly predictable.
        let h = |i: usize| -> f32 { [18.0, 40.0, 26.0, 60.0][i % 4] };
        let top_of = |n: usize| -> f32 { (0..n).map(|i| h(i) + GAP).sum::<f32>() };

        let opts = ListOptions { gap: GAP, height: Size::Fixed(VIEW), ..ListOptions::new(0.0) };
        let mut ui = ui();
        let frame = |ui: &mut Ui| -> (Range<usize>, Vec<(usize, Rect)>) {
            let mut rects = Vec::new();
            ui.begin_frame(FrameInfo::default());
            let built = ui.virtual_rows_with("rows", ROWS, opts, h, |ui, i| {
                let id = ui.make_id("cell");
                ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h(i))), Vec2::ZERO, true, |_, _| {});
                if let Some(r) = ui.rect_of(id) {
                    rects.push((i, r));
                }
            });
            let _ = ui.end_frame();
            (built, rects)
        };
        frame(&mut ui);
        let (built, rects) = frame(&mut ui);
        assert!(built.len() < 30, "built {} rows for a {VIEW}px viewport", built.len());
        for (i, r) in &rects {
            assert!((r.y - top_of(*i)).abs() < 0.01, "row {i} at {} not {}", r.y, top_of(*i));
            assert!((r.h - h(*i)).abs() < 0.01, "row {i} is {} tall not {}", r.h, h(*i));
        }

        // To the bottom: the last row must be the last item, flush with the edge.
        let mut last = (built, rects);
        for _ in 0..400 {
            wheel(&mut ui, -1000.0);
            last = frame(&mut ui);
        }
        for _ in 0..30 {
            last = frame(&mut ui);
        }
        let (built, rects) = last;
        let (last_i, last_r) = *rects.last().unwrap();
        assert_eq!(last_i, ROWS - 1, "could not reach the end");
        assert!((last_r.bottom() - VIEW).abs() < 1.0, "last row ends at {} not {VIEW}", last_r.bottom());
        assert!(built.len() < 30, "still only a screenful at the bottom: {}", built.len());

        // Mid-list: the rows shown must be the ones actually under the window.
        for _ in 0..200 {
            wheel(&mut ui, 1000.0);
            frame(&mut ui);
        }
        for _ in 0..30 {
            frame(&mut ui);
        }
        let (built, rects) = frame(&mut ui);
        let (i0, r0) = rects[0];
        assert!(built.contains(&i0));
        assert!((r0.y - top_of(i0)).abs() < 0.01, "mid-list row {i0} at {} not {}", r0.y, top_of(i0));
    }

    /// Rows must keep their identity as the window slides over them, or a hover
    /// or drag would jump to a neighbour mid-scroll.
    #[test]
    fn virtual_rows_keep_their_identity_while_scrolling() {
        let opts = ListOptions::new(20.0);
        let mut ui = ui();
        let ids = |ui: &mut Ui| -> Vec<(usize, Id)> {
            let mut out = Vec::new();
            ui.begin_frame(FrameInfo::default());
            let o = ListOptions { height: Size::Fixed(200.0), ..opts };
            ui.virtual_list_with("rows", 1000, o, |ui, i| {
                out.push((i, ui.make_id("cell")));
            });
            let _ = ui.end_frame();
            out
        };
        ids(&mut ui);
        let before = ids(&mut ui);
        for _ in 0..40 {
            wheel(&mut ui, -2.0);
            ids(&mut ui);
        }
        let after = ids(&mut ui);

        let mut checked = 0;
        for (i, id) in &after {
            if let Some((_, was)) = before.iter().find(|(j, _)| j == i) {
                assert_eq!(id, was, "row {i} changed identity while scrolling");
                checked += 1;
            }
        }
        assert!(checked > 3, "only {checked} rows overlapped; the test proves nothing");
    }

    /// Hold the modifiers and press the key. Modifiers must still be held when
    /// the frame runs (`keys_pressed` records presses; `modifiers` is state),
    /// so releasing is a separate step after the frame.
    fn press(ui: &mut Ui, k: Key, mods: &[Key]) {
        for &m in mods {
            ui.push(InputEvent::Key { key: m, pressed: true, repeat: false });
        }
        ui.push(InputEvent::Key { key: k, pressed: true, repeat: false });
    }

    fn release(ui: &mut Ui, k: Key, mods: &[Key]) {
        ui.push(InputEvent::Key { key: k, pressed: false, repeat: false });
        for &m in mods {
            ui.push(InputEvent::Key { key: m, pressed: false, repeat: false });
        }
    }

    /// One press drives one command, and the modifiers must match exactly.
    #[test]
    fn a_shortcut_fires_once_and_matches_exactly() {
        let mut ui = ui();
        ui.set_mac_shortcuts(false);
        let save = Shortcut::command(Key::S);

        press(&mut ui, Key::S, &[Key::ControlLeft]);
        ui.begin_frame(FrameInfo::default());
        assert!(ui.consume_shortcut(save), "Ctrl+S did not fire");
        assert!(!ui.consume_shortcut(save), "the same press fired twice");
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[Key::ControlLeft]);

        // Ctrl+Shift+S is a different shortcut.
        press(&mut ui, Key::S, &[Key::ControlLeft, Key::ShiftLeft]);
        ui.begin_frame(FrameInfo::default());
        assert!(!ui.consume_shortcut(save), "Ctrl+Shift+S fired a Ctrl+S shortcut");
        assert!(ui.consume_shortcut(save.shift()), "Ctrl+Shift+S did not fire");
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[Key::ControlLeft, Key::ShiftLeft]);

        // A bare key is not a command shortcut.
        press(&mut ui, Key::S, &[]);
        ui.begin_frame(FrameInfo::default());
        assert!(!ui.consume_shortcut(save), "a bare S fired Ctrl+S");
        assert!(ui.consume_shortcut(Shortcut::plain(Key::S)));
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[]);
    }

    /// `command` must resolve to the platform's key, and the *other* one must
    /// not work: Ctrl+S on a Mac is not Save.
    #[test]
    fn command_is_platform_correct() {
        let save = Shortcut::command(Key::S);
        let mut ui = ui();
        ui.set_mac_shortcuts(true);
        press(&mut ui, Key::S, &[Key::SuperLeft]);
        ui.begin_frame(FrameInfo::default());
        assert!(ui.consume_shortcut(save), "Cmd+S did not fire on mac");
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[Key::SuperLeft]);

        press(&mut ui, Key::S, &[Key::ControlLeft]);
        ui.begin_frame(FrameInfo::default());
        assert!(!ui.consume_shortcut(save), "Ctrl+S fired a command shortcut on mac");
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[Key::ControlLeft]);

        assert_eq!(save.label(true), "\u{2318}S");
        assert_eq!(save.shift().label(false), "Ctrl+Shift+S");
        assert_eq!(Shortcut::plain(Key::F2).label(false), "F2");
        assert_eq!(Shortcut::plain(Key::Delete).label(true), "Del");
    }

    /// While the user is typing, the keys the field handles belong to the
    /// field — but a real command shortcut must still get through.
    #[test]
    fn typing_keeps_its_keys_but_not_all_of_them() {
        let mut ui = ui();
        ui.set_mac_shortcuts(false);
        let mut text = String::from("hello");
        let frame = |ui: &mut Ui, text: &mut String| -> (bool, bool) {
            ui.begin_frame(FrameInfo::default());
            let del = ui.consume_shortcut(Shortcut::plain(Key::Delete));
            let save = ui.consume_shortcut(Shortcut::command(Key::S));
            ui.text_input("field", text, "");
            let _ = ui.end_frame();
            (del, save)
        };
        frame(&mut ui, &mut text);

        // Nothing focused: Delete is the app's.
        press(&mut ui, Key::Delete, &[]);
        assert!(frame(&mut ui, &mut text).0, "Delete should reach the app when nothing has focus");
        release(&mut ui, Key::Delete, &[]);

        // Focus the field, then try again.
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(40.0, 15.0) });
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        frame(&mut ui, &mut text);
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        frame(&mut ui, &mut text);
        assert!(ui.wants_keyboard(), "the field did not take focus");

        press(&mut ui, Key::Delete, &[]);
        assert!(!frame(&mut ui, &mut text).0, "Delete fired an app command while typing");
        release(&mut ui, Key::Delete, &[]);

        press(&mut ui, Key::S, &[Key::ControlLeft]);
        assert!(frame(&mut ui, &mut text).1, "Ctrl+S must still save while typing");
        release(&mut ui, Key::S, &[Key::ControlLeft]);
    }

    /// An inactive scope blocks the shortcuts inside it, and scopes nest.
    #[test]
    fn scopes_route_shortcuts_by_focus() {
        let mut ui = ui();
        ui.set_mac_shortcuts(false);
        let sc = Shortcut::plain(Key::F2);
        let run = |ui: &mut Ui, outer: bool, inner: bool| -> (bool, bool, bool) {
            ui.begin_frame(FrameInfo::default());
            let mut got = (false, false, false);
            ui.shortcut_scope(outer, |ui| {
                got.0 = ui.consume_shortcut(sc);
                ui.shortcut_scope(inner, |ui| {
                    got.1 = ui.consume_shortcut(sc);
                });
            });
            got.2 = ui.consume_shortcut(sc);
            let _ = ui.end_frame();
            got
        };

        press(&mut ui, Key::F2, &[]);
        assert_eq!(run(&mut ui, false, true), (false, false, true), "an inactive outer scope must block its inner one");
        release(&mut ui, Key::F2, &[]);
        press(&mut ui, Key::F2, &[]);
        assert_eq!(run(&mut ui, true, false), (true, false, false), "the active outer scope should have claimed it");
        release(&mut ui, Key::F2, &[]);
        press(&mut ui, Key::F2, &[]);
        assert_eq!(run(&mut ui, false, false), (false, false, true), "outside every scope it is still available");
        release(&mut ui, Key::F2, &[]);
    }

    /// Stacking is by layer, not by build order: a popup built first still
    /// wins the pointer against a window built after it.
    #[test]
    fn layers_stack_by_rank_not_build_order() {
        let mut ui = ui();
        let (popup_id, window_id) = (Id::new("pop"), Id::new("win"));
        let overlap = Rect::new(50.0, 50.0, 200.0, 200.0);
        let frame = |ui: &mut Ui| -> (bool, bool) {
            ui.begin_frame(FrameInfo::default());
            // Built first, but in the higher layer.
            let mut on_popup = false;
            ui.layer_in(popup_id, Layer::Popup, overlap, Frame::none(), |ui| {
                let id = ui.make_id("pop_hit");
                ui.add_leaf(id, Layout::leaf(Size::Fixed(200.0), Size::Fixed(200.0)), Vec2::ZERO, true, |_, _| {});
                on_popup = ui.interact(id).hovered;
            });
            let mut on_window = false;
            ui.layer_in(window_id, Layer::Window, overlap, Frame::none(), |ui| {
                let id = ui.make_id("win_hit");
                ui.add_leaf(id, Layout::leaf(Size::Fixed(200.0), Size::Fixed(200.0)), Vec2::ZERO, true, |_, _| {});
                on_window = ui.interact(id).hovered;
            });
            let _ = ui.end_frame();
            (on_popup, on_window)
        };
        frame(&mut ui);
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(100.0, 100.0) });
        frame(&mut ui);
        let (on_popup, on_window) = frame(&mut ui);
        assert!(on_popup, "the popup layer did not get the pointer");
        assert!(!on_window, "the window layer took the pointer from the popup above it");
    }

    /// A tooltip waits for the pointer to settle, and never appears while a
    /// menu is open — it must not cover what you are about to click.
    #[test]
    fn a_tooltip_waits_for_the_delay() {
        let mut ui = ui();
        let delay = ui.theme.tooltip.delay;
        let tip_id = Id::new("root").with(("button", "Save")).with("tooltip");
        let frame = |ui: &mut Ui, dt: f32| {
            ui.begin_frame(FrameInfo { dt, ..FrameInfo::default() });
            let r = ui.button("Save");
            ui.tooltip(&r, "Save the scene");
            let _ = ui.end_frame();
        };
        frame(&mut ui, 0.016);
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 10.0) });
        frame(&mut ui, 0.016);
        let id = tip_id;
        assert!(ui.rect_of(id).is_none(), "the tooltip appeared immediately");

        // Rest on it past the delay.
        for _ in 0..4 {
            frame(&mut ui, delay * 0.5);
        }
        assert!(ui.rect_of(id).is_some(), "the tooltip never appeared after {delay}s");

        // Moving away takes it down again.
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(600.0, 400.0) });
        frame(&mut ui, 0.016);
        frame(&mut ui, 0.016);
        assert!(ui.rect_of(id).is_none(), "the tooltip stayed up after the pointer left");
    }

    /// The whole point: a widget inside a zoomed, panned canvas is hit where it
    /// *appears*, but reports its rect and the pointer in canvas coordinates,
    /// so app logic does not change with the zoom.
    #[test]
    fn canvas_widgets_hit_on_screen_and_report_canvas_space() {
        let mut ui = ui();
        let node = Rect::new(200.0, 100.0, 120.0, 40.0);
        let frame = |ui: &mut Ui, st: &mut CanvasState| -> Response {
            ui.begin_frame(FrameInfo::default());
            let mut inner = Response::default();
            ui.canvas("graph", st, |ui, _| {
                let id = ui.make_id("node");
                let opts = LeafOptions { interactive: true, ..Default::default() };
                ui.add_leaf_at(id, node, opts, |_, _| {});
                inner = ui.interact(id);
            });
            let _ = ui.end_frame();
            inner
        };

        let mut st = CanvasState { wheel_zooms: false, ..CanvasState::default() };
        frame(&mut ui, &mut st);
        frame(&mut ui, &mut st);

        // Zoom 2x and pan; the node is drawn at 2*rect + pan.
        st.zoom = 2.0;
        st.pan = Vec2::new(-100.0, -50.0);
        frame(&mut ui, &mut st);
        // Aim at a point 10 canvas px inside the node, and work out where that
        // lands on screen: canvas * zoom + pan.
        let target = Vec2::new(node.x + 10.0, node.y + 10.0);
        let on_screen = Vec2::new(target.x * 2.0 - 100.0, target.y * 2.0 - 50.0);

        // The canvas coordinates themselves must NOT hit: the node has moved.
        ui.push(InputEvent::PointerMoved { pos: target });
        assert!(!frame(&mut ui, &mut st).hovered, "hit-testing ignored the canvas transform");

        // Pointing where it is drawn must hit.
        ui.push(InputEvent::PointerMoved { pos: on_screen });
        let r = frame(&mut ui, &mut st);
        assert!(r.hovered, "the widget was not hit where it is drawn");

        // ...and what it reports is canvas space, not window space.
        assert_eq!(r.rect, node, "rect should be in canvas coordinates");
        assert!(
            (r.mouse_pos.x - target.x).abs() < 0.01 && (r.mouse_pos.y - target.y).abs() < 0.01,
            "pointer reported at {:?}, expected canvas coords near {target:?}",
            r.mouse_pos
        );
    }

    /// Dragging inside a canvas must move content by the same canvas distance
    /// at any zoom, or nodes would fly away when zoomed in.
    #[test]
    fn canvas_drag_deltas_are_zoom_independent() {
        let mut ui = ui();
        let node = Rect::new(0.0, 0.0, 400.0, 400.0);
        let drag = |ui: &mut Ui, zoom: f32| -> Vec2 {
            let mut st = CanvasState { zoom, wheel_zooms: false, ..CanvasState::default() };
            // Returns this frame's drag delta, in canvas units.
            let frame = |ui: &mut Ui, st: &mut CanvasState| -> Vec2 {
                let mut d = Vec2::ZERO;
                ui.begin_frame(FrameInfo::default());
                ui.canvas("graph", st, |ui, _| {
                    let id = ui.make_id("node");
                    let opts = LeafOptions { interactive: true, ..Default::default() };
                    ui.add_leaf_at(id, node, opts, |_, _| {});
                    d = ui.interact_drag(id).drag_delta;
                });
                let _ = ui.end_frame();
                d
            };
            frame(ui, &mut st);
            frame(ui, &mut st);
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 60.0) });
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
            frame(ui, &mut st);
            // Drag 80 window px to the right.
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(140.0, 60.0) });
            let moved = frame(ui, &mut st);
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
            frame(ui, &mut st);
            moved
        };

        let at_1x = drag(&mut ui, 1.0);
        let mut fresh = self::tests::ui();
        let at_2x = drag(&mut fresh, 2.0);
        assert!((at_1x.x - 80.0).abs() < 0.01, "1x drag reported {:?}", at_1x);
        assert!(
            (at_2x.x - 40.0).abs() < 0.01,
            "at 2x zoom, 80 window px should be 40 canvas px, got {:?}",
            at_2x
        );
    }

    /// Zooming with the wheel keeps the point under the cursor still, which is
    /// what makes a canvas feel attached to the pointer.
    #[test]
    fn zoom_at_keeps_the_point_under_the_cursor() {
        let mut st = CanvasState::default();
        let origin = Vec2::new(30.0, 20.0);
        let cursor = Vec2::new(430.0, 320.0);
        let canvas_before = Vec2::new(
            (cursor.x - origin.x - st.pan.x) / st.zoom,
            (cursor.y - origin.y - st.pan.y) / st.zoom,
        );
        for f in [1.3f32, 1.3, 0.6, 2.2] {
            st.zoom_at(cursor, origin, f);
            let after = Vec2::new(
                (cursor.x - origin.x - st.pan.x) / st.zoom,
                (cursor.y - origin.y - st.pan.y) / st.zoom,
            );
            assert!(
                (after.x - canvas_before.x).abs() < 0.01 && (after.y - canvas_before.y).abs() < 0.01,
                "the canvas point under the cursor drifted to {after:?} from {canvas_before:?}"
            );
        }
        assert!(st.zoom <= st.max_zoom && st.zoom >= st.min_zoom);
    }

    /// Lines are emitted as `Line` instances carrying their endpoints, are
    /// culled like everything else, and follow the canvas transform.
    #[test]
    fn lines_carry_their_endpoints_and_follow_the_canvas() {
        use crate::render_contract::PrimitiveKind;
        let mut ui = ui();
        let seg = |out: &FrameOutput| -> Vec<[f32; 4]> {
            out.draw
                .instances
                .iter()
                .filter(|i| PrimitiveKind::from_code(i.params[3]) == Some(PrimitiveKind::Line))
                .map(|i| i.uv)
                .collect()
        };

        // Plain, untransformed.
        ui.begin_frame(FrameInfo::default());
        let lid = ui.make_id("l");
        ui.add_leaf(lid, Layout::leaf(Size::Fixed(10.0), Size::Fixed(10.0)), Vec2::ZERO, false, |p, _| {
            p.line(Vec2::new(10.0, 20.0), Vec2::new(110.0, 220.0), 2.0, Color::WHITE);
        });
        let out = ui.end_frame();
        // A long diagonal is split to keep each quad tight (see DrawList::line),
        // so check the chain runs end to end rather than counting instances.
        let segs = seg(&out);
        assert!(!segs.is_empty(), "no line instance emitted");
        assert_eq!([segs[0][0], segs[0][1]], [10.0, 20.0], "chain does not start at the line's start");
        let last = segs.last().unwrap();
        assert_eq!([last[2], last[3]], [110.0, 220.0], "chain does not end at the line's end");
        for w in segs.windows(2) {
            assert_eq!([w[0][2], w[0][3]], [w[1][0], w[1][1]], "a gap between split segments");
        }

        // Inside a 2x canvas panned by (30, 40): endpoints map to the window.
        let mut st = CanvasState { zoom: 2.0, pan: Vec2::new(30.0, 40.0), wheel_zooms: false, ..CanvasState::default() };
        let run = |ui: &mut Ui, st: &mut CanvasState| -> Vec<[f32; 4]> {
            ui.begin_frame(FrameInfo::default());
            ui.canvas("c", st, |ui, _| {
                let id = ui.make_id("l");
                ui.add_leaf_at(id, Rect::new(0.0, 0.0, 400.0, 400.0), LeafOptions::default(), |p, _| {
                    p.line(Vec2::new(10.0, 20.0), Vec2::new(110.0, 220.0), 2.0, Color::WHITE);
                });
            });
            let out = ui.end_frame();
            seg(&out)
        };
        run(&mut ui, &mut st);
        let got = run(&mut ui, &mut st);
        assert_eq!([got[0][0], got[0][1]], [10.0 * 2.0 + 30.0, 20.0 * 2.0 + 40.0]);
        let last = got.last().unwrap();
        assert_eq!([last[2], last[3]], [110.0 * 2.0 + 30.0, 220.0 * 2.0 + 40.0]);

        // Far off screen: culled, like any other primitive.
        ui.begin_frame(FrameInfo::default());
        let lid = ui.make_id("l2");
        ui.add_leaf(lid, Layout::leaf(Size::Fixed(10.0), Size::Fixed(10.0)), Vec2::ZERO, false, |p, _| {
            p.line(Vec2::new(-9000.0, -9000.0), Vec2::new(-8000.0, -8000.0), 2.0, Color::WHITE);
        });
        let out = ui.end_frame();
        assert!(seg(&out).is_empty(), "an offscreen line was not culled");
    }

    /// A curve is flattened by how big it is *on screen*, so it stays smooth
    /// when zoomed in without wasting instances when zoomed out.
    #[test]
    fn bezier_detail_follows_the_zoom() {
        use crate::render_contract::PrimitiveKind;
        let mut ui = ui();
        let count = |ui: &mut Ui, st: &mut CanvasState| -> usize {
            ui.begin_frame(FrameInfo::default());
            ui.canvas("c", st, |ui, _| {
                let id = ui.make_id("w");
                ui.add_leaf_at(id, Rect::new(0.0, 0.0, 600.0, 400.0), LeafOptions::default(), |p, _| {
                    p.wire(Vec2::new(0.0, 0.0), Vec2::new(300.0, 200.0), 2.0, Color::WHITE);
                });
            });
            let out = ui.end_frame();
            out.draw
                .instances
                .iter()
                .filter(|i| PrimitiveKind::from_code(i.params[3]) == Some(PrimitiveKind::Line))
                .count()
        };
        let mut far = CanvasState { zoom: 0.25, wheel_zooms: false, ..CanvasState::default() };
        let mut near = CanvasState { zoom: 4.0, wheel_zooms: false, ..CanvasState::default() };
        count(&mut ui, &mut far);
        let at_far = count(&mut ui, &mut far);
        count(&mut ui, &mut near);
        let at_near = count(&mut ui, &mut near);
        assert!(at_far >= 3, "a curve should still be a curve when zoomed out: {at_far}");
        assert!(at_near > at_far * 2, "zoomed in {at_near} segments vs {at_far} zoomed out");
    }

    /// Content positioned on a canvas must follow the canvas: drawn at the
    /// transformed place, hit there, clipped to the canvas, and lining up with
    /// anything else drawn in canvas coordinates.
    ///
    /// Regression: node panels were built with `layer_in`, which hangs off the
    /// root, so they drew at raw canvas coordinates over other panels while
    /// the wires (built with `add_leaf_at`, inside the canvas) moved correctly.
    #[test]
    fn positioned_content_follows_the_canvas() {
        use crate::render_contract::PrimitiveKind;
        let node = Rect::new(100.0, 50.0, 120.0, 60.0);
        let mut st = CanvasState { zoom: 2.0, pan: Vec2::new(30.0, 40.0), wheel_zooms: false, ..CanvasState::default() };
        let mut ui = ui();

        // Canvas fills the window, so the transform is exactly zoom + pan.
        let expect = Rect::new(100.0 * 2.0 + 30.0, 50.0 * 2.0 + 40.0, 120.0 * 2.0, 60.0 * 2.0);

        let run = |ui: &mut Ui, st: &mut CanvasState| -> (Vec<[f32; 4]>, Vec<[f32; 4]>, Response) {
            ui.begin_frame(FrameInfo::default());
            let mut inner = Response::default();
            ui.canvas("c", st, |ui, _| {
                // A wire in canvas coordinates, from the node's right edge.
                let wid = ui.make_id("wire");
                ui.add_leaf_at(wid, Rect::new(0.0, 0.0, 900.0, 600.0), LeafOptions::default(), move |p, _| {
                    p.line(Vec2::new(node.right(), node.y), Vec2::new(400.0, 300.0), 2.0, Color::WHITE);
                });
                let f = Frame { fill: Color::WHITE, border: Color::WHITE, border_width: 1.0, radius: 0.0, shadow: false, clip: true };
                let nid = ui.make_id("node");
                ui.container_at(nid, node, f, |ui| {
                    let bid = ui.make_id("hit");
                    ui.add_leaf(bid, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, |_, _| {});
                    inner = ui.interact(bid);
                });
            });
            let out = ui.end_frame();
            let mut shapes = Vec::new();
            let mut lines = Vec::new();
            for i in &out.draw.instances {
                match PrimitiveKind::from_code(i.params[3]) {
                    Some(PrimitiveKind::Line) => lines.push(i.uv),
                    _ => shapes.push(i.rect),
                }
            }
            (shapes, lines, inner)
        };

        run(&mut ui, &mut st);
        let (shapes, lines, _) = run(&mut ui, &mut st);

        // 1. Drawn where the canvas puts it, at the canvas's scale.
        let found = shapes.iter().any(|r| {
            (r[0] - expect.x).abs() < 0.5 && (r[1] - expect.y).abs() < 0.5 && (r[2] - expect.w).abs() < 0.5
        });
        assert!(found, "node drawn at {shapes:?}, expected {expect:?}");

        // 2. The wire leaving the node's edge starts at the node's drawn edge,
        //    so wires and nodes cannot disagree.
        let wire = lines[0];
        assert!(
            (wire[0] - expect.right()).abs() < 0.5 && (wire[1] - expect.y).abs() < 0.5,
            "wire starts at ({}, {}) but the node's corner is ({}, {})",
            wire[0],
            wire[1],
            expect.right(),
            expect.y
        );

        // 3. Hit where it is drawn, not at its canvas coordinates.
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(node.x + 10.0, node.y + 10.0) });
        assert!(!run(&mut ui, &mut st).2.hovered, "hit at raw canvas coordinates");
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(expect.x + 10.0, expect.y + 10.0) });
        assert!(run(&mut ui, &mut st).2.hovered, "not hit where it is drawn");

        // 4. Panned far away it is clipped out entirely, rather than escaping
        //    the canvas and drawing over neighbouring panels.
        st.pan = Vec2::new(-20_000.0, -20_000.0);
        run(&mut ui, &mut st);
        let (shapes, _, _) = run(&mut ui, &mut st);
        assert!(shapes.is_empty(), "content escaped the canvas: {shapes:?}");
    }

    /// Sideways scrolling: the timeline case. Wide rows inside a narrow area
    /// move horizontally, the vertical axis is independent, and `Grow` children
    /// span the *content* width rather than the viewport.
    #[test]
    fn a_scroll_area_scrolls_sideways_independently() {
        let mut ui = ui();
        // 10 rows, each holding one 900px-wide item, in a 300x100 viewport.
        let frame = |ui: &mut Ui| -> Vec<Rect> {
            let mut rects = Vec::new();
            ui.begin_frame(FrameInfo::default());
            let opts = ScrollOptions::both(Size::Fixed(300.0), Size::Fixed(100.0));
            ui.scroll_area_with("grid", opts, |ui| {
                for r in 0..10 {
                    let id = ui.make_id(("row", r));
                    ui.add_leaf(id, Layout::leaf(Size::Fixed(900.0), Size::Fixed(30.0)), Vec2::ZERO, true, |_, _| {});
                    if let Some(rr) = ui.rect_of(id) {
                        rects.push(rr);
                    }
                }
            });
            let _ = ui.end_frame();
            rects
        };
        frame(&mut ui);
        let start = frame(&mut ui);
        assert_eq!(start[0].x, 0.0, "starts at the left");
        assert_eq!(start[0].y, 0.0, "starts at the top");

        // A sideways wheel moves x and leaves y alone.
        for _ in 0..40 {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(100.0, 50.0) });
            ui.push(InputEvent::Wheel { delta: Vec2::new(-20.0, 0.0), unit: crate::WheelUnit::Pixel });
            frame(&mut ui);
        }
        let moved = frame(&mut ui);
        assert!(moved[0].x < -100.0, "did not scroll sideways: x = {}", moved[0].x);
        assert_eq!(moved[0].y, 0.0, "a sideways wheel moved the vertical axis");
        // Clamped at the end: content 900 - viewport 300 = 600.
        assert!(moved[0].x >= -600.5, "scrolled past the content: x = {}", moved[0].x);

        // Let the horizontal smoothing settle, or the next assertion reads an
        // offset that is still easing towards its target.
        for _ in 0..20 {
            frame(&mut ui);
        }
        let settled = frame(&mut ui);
        let x_before = settled[0].x;
        for _ in 0..10 {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(100.0, 50.0) });
            ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -20.0), unit: crate::WheelUnit::Pixel });
            frame(&mut ui);
        }
        let both = frame(&mut ui);
        assert!(both[0].y < -50.0, "did not scroll vertically: y = {}", both[0].y);
        assert!((both[0].x - x_before).abs() < 0.5, "a vertical wheel moved the horizontal axis");
    }

    /// A vertical-only area must not scroll sideways just because something
    /// inside it is wide — that would make every long label shift the column.
    #[test]
    fn a_vertical_area_ignores_wide_content() {
        let mut ui = ui();
        let frame = |ui: &mut Ui| -> Rect {
            ui.begin_frame(FrameInfo::default());
            let mut out = Rect::default();
            ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(100.0)), |ui| {
                let id = ui.make_id("wide");
                ui.add_leaf(id, Layout::leaf(Size::Fixed(900.0), Size::Fixed(30.0)), Vec2::ZERO, true, |_, _| {});
                out = ui.rect_of(id).unwrap_or_default();
            });
            let _ = ui.end_frame();
            out
        };
        frame(&mut ui);
        frame(&mut ui);
        for _ in 0..20 {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(50.0, 50.0) });
            ui.push(InputEvent::Wheel { delta: Vec2::new(-30.0, 0.0), unit: crate::WheelUnit::Pixel });
            frame(&mut ui);
        }
        assert_eq!(frame(&mut ui).x, 0.0, "a vertical-only area scrolled sideways");
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

    /// A canvas on a tablet: two fingers pinch-zoom around their midpoint and
    /// pan, and the canvas point under the midpoint stays under it.
    #[test]
    fn two_fingers_pinch_zoom_and_pan_a_canvas() {
        let mut ui = ui();
        let mut st = CanvasState::default();
        let frame = |ui: &mut Ui, st: &mut CanvasState, fingers: Vec<Vec2>| {
            push_fingers(ui, &fingers);
            ui.begin_frame(FrameInfo::default());
            ui.canvas("c", st, |_, _| {});
            let _ = ui.end_frame();
        };
        frame(&mut ui, &mut st, vec![]);
        frame(&mut ui, &mut st, vec![]);
        let mid = Vec2::new(300.0, 200.0);
        let under = |st: &CanvasState| Vec2::new((mid.x - st.pan.x) / st.zoom, (mid.y - st.pan.y) / st.zoom);
        let before = under(&st);
        frame(&mut ui, &mut st, touch(&[(280.0, 200.0)]));
        frame(&mut ui, &mut st, touch(&[(280.0, 200.0), (320.0, 200.0)]));
        frame(&mut ui, &mut st, touch(&[(260.0, 200.0), (340.0, 200.0)]));
        assert!((st.zoom - 2.0).abs() < 1e-3, "spreading 40 -> 80 px doubles the zoom: {}", st.zoom);
        let after = under(&st);
        assert!((after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3, "{before:?} vs {after:?}");

        // Moving both fingers pans by the same screen distance.
        let pan = st.pan;
        frame(&mut ui, &mut st, touch(&[(270.0, 230.0), (350.0, 230.0)]));
        assert_eq!(st.pan - pan, Vec2::new(10.0, 30.0));
        assert!((st.zoom - 2.0).abs() < 1e-3, "a parallel move does not zoom");
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
