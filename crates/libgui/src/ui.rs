use crate::layout::{self, Kids, Node, Scroll};
use crate::text_edit::TextState;
use crate::hash::{FxMap, FxSet};
use crate::input::UiEvent;
use crate::input_state::InputState;
use crate::scroll::{ScrollConfig, Smoothing};
use crate::{Align, Axis, Atlas, Color, Cursor, DrawList, FontId, Fonts, FrameInfo, FrameInput, Gesture, Id, InputEvent, Insets, Key, KeyBindings, Layout, Painter, PlatformOutput, PointerButton, PointerKind, Rect, Shortcut, Size, Theme, Transform, UiAction, Vec2};
use std::hash::Hash;

/// `v` if it is a real number, else `fallback`.
///
/// Guards the values that reach libgui's own retained state — animation
/// values, scroll offsets — where a NaN would not wash out on the next frame
/// but stay for the lifetime of the widget. Geometry an app passes straight
/// through to a draw call is left alone: that is the app's number, and a
/// NaN there draws nothing rather than corrupting anything.
pub(crate) fn sane(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

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
    /// Has keyboard focus. A widget that shows a focused state draws it from
    /// here; the focus *ring* is drawn for it.
    pub focused: bool,
    /// Mouse went down on this widget and has not been released yet.
    pub active: bool,
    pub pressed: bool,
    pub clicked: bool,
    /// Clicked twice in the same place, inside
    /// [`Ui::double_click_time`](Ui::double_click_time). Both `clicked` and
    /// this are true on that frame: a double click is a click that happens to
    /// be the second one, and a widget that only cares about single clicks
    /// needs no change.
    pub double_clicked: bool,
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
    /// Modifiers held this frame. On the frame something was clicked, these
    /// are the modifiers of that click — which is what a list row needs to
    /// tell a plain click from a Ctrl-click or a Shift-click.
    pub modifiers: crate::Modifiers,
    /// Two-finger pinch over this widget: zoom ratio minus 1 (0 = none).
    pub pinch: f32,
    /// Two-finger pan over this widget.
    pub pan2: Vec2,
}


impl Response {
    /// True on the frame a menu-like widget should open: the press itself, or
    /// a keyboard activation.
    ///
    /// Menus open on press, not on click. Waiting for the release makes the
    /// menu arrive a beat after the hand, which reads as the whole UI being
    /// slow even when the frame behind it costs a fraction of a millisecond.
    /// A keyboard activation sets `clicked` without `active` — there was no
    /// press to start it — so it is picked up here too.
    pub fn opened(&self) -> bool {
        self.pressed || (self.clicked && !self.active)
    }
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
    /// Override [`Ui::scroll`] for this area: a timeline and an inspector do
    /// not have to feel the same.
    pub config: Option<ScrollConfig>,
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
            config: None,
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
    /// Override [`Ui::scroll`] for this list. See [`ScrollConfig`].
    pub config: Option<ScrollConfig>,
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
            config: None,
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
    /// How the offset is currently closing on the target. Set when input
    /// arrives and kept until the next input, so an ease a wheel notch started
    /// is not cut short by the input-free frames that follow it.
    smoothing: Smoothing,
    /// The offset handed to layout last frame. A scroll is "moving" when this
    /// frame's differs, which is the only reliable test: a trackpad scroll
    /// settles within the frame, so `offset == target` even mid-gesture.
    applied: f32,
}

impl ScrollAxis {
    fn max(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// Move the destination by this frame's input, and pick how the offset
    /// should follow it.
    ///
    /// `continuous` and `stepped` are this frame's delta in logical px, split
    /// by what the host said the signal *is* — not by what produced it. A
    /// finger on the glass and a fling in progress are continuous by
    /// construction and always track exactly; easing them would make the
    /// content trail the thing moving it.
    fn update(&mut self, continuous: f32, stepped: f32, touch: Option<f32>, cfg: &ScrollConfig, dt: f32) {
        let max = self.max();
        let delta = continuous + stepped;
        if delta != 0.0 {
            self.target -= delta;
            self.velocity = 0.0;
            // A frame carrying a step is smoothed as a step: the notch is the
            // big number, and it is the part that needs turning into motion.
            self.smoothing = if stepped != 0.0 { cfg.stepped } else { cfg.continuous };
        }
        match touch {
            Some(d) => {
                self.target = (self.target - d).clamp(0.0, max);
                self.velocity += (-d / dt - self.velocity) * 0.4;
                self.smoothing = Smoothing::Instant;
            }
            None if self.velocity.abs() > cfg.fling_cutoff => {
                self.target += self.velocity * dt;
                self.velocity *= (-cfg.friction * dt).exp();
                if self.target <= 0.0 || self.target >= max {
                    self.velocity = 0.0;
                }
                self.smoothing = Smoothing::Instant;
            }
            None => self.velocity = 0.0,
        }
    }

    /// `scale` is physical px per logical px: an ease ends when it has less
    /// than half a physical pixel left to travel, because nothing it does
    /// after that can reach the screen.
    fn settle(&mut self, dt: f32, scale: f32) {
        // Content and viewport come from layout, which an app can feed a NaN
        // (a `Size::Fixed(f32::NAN)`, a column width read from a broken file).
        // Offsets are retained, so one would stay for good.
        self.content = sane(self.content, 0.0);
        self.viewport = sane(self.viewport, 0.0);
        self.offset = sane(self.offset, 0.0);
        self.target = sane(self.target, 0.0);
        self.velocity = sane(self.velocity, 0.0);
        let max = self.max();
        self.target = self.target.clamp(0.0, max);
        match self.smoothing {
            Smoothing::Eased { rate } if rate > 0.0 => {
                let k = 1.0 - (-rate * dt).exp();
                self.offset += (self.target - self.offset) * k;
                if (self.target - self.offset).abs() * scale < 0.5 {
                    self.offset = self.target;
                }
            }
            _ => self.offset = self.target,
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
    /// Where this frame's time went. All zeroes without the `profile` feature.
    pub profile: crate::Profile,
    pub draw: &'a DrawList,
    pub atlas: &'a Atlas,
    pub screen_size: Vec2,
    pub scale: f32,
    pub clear_color: Color,
    /// Requests for the host: cursor, clipboard, keyboard/IME, pointer lock, repaint.
    pub platform: PlatformOutput,
}

/// One UI: its retained state, its fonts and its frame in progress.
///
/// # Threads
///
/// `Ui` is **not `Send`**, and that is deliberate rather than an oversight.
/// It holds the app's own values in boxed closures and trait objects — paint
/// closures, drag-and-drop payloads (`Box<dyn Any>`), the font rasteriser —
/// and none of them is required to be `Send`. Requiring it would forbid an
/// app from capturing an `Rc` or a `RefCell` in a paint closure, which is the
/// ordinary thing to do on a UI thread.
///
/// So a `Ui` lives on the thread that created it. An engine that simulates or
/// renders elsewhere sends *data* to the UI thread — the state to show, a
/// finished texture to display with `TextureId::User` — and the UI thread
/// builds the frame and hands its `FrameOutput` to the renderer. Nothing in
/// libgui needs the frame to be built where it is drawn.
///
/// ```compile_fail
/// // Pinned: if `Ui` ever becomes `Send`, this stops failing and the section
/// // above must be rewritten.
/// fn is_send<T: Send>() {}
/// is_send::<libgui::Ui>();
/// ```
/// What a click should do to a selection. Which modifier produces which of
/// these is a platform convention and therefore not libgui's to decide —
/// `libgui_keymap::Keymap::select_kind` maps them, or an app can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectKind {
    /// A plain click: this one and nothing else.
    Replace,
    /// Ctrl/Cmd-click: add or remove this one, leave the rest.
    Toggle,
    /// Shift-click: everything from the anchor to here.
    Range,
}

/// What to do to the app's selection, from [`Ui::select`].
///
/// libgui does not hold the selection. A CAD model browser selects *bodies*, a
/// file list selects *paths*, a timeline selects *clips*: the set is the app's,
/// in the app's own terms. What libgui keeps is the **anchor** — where the
/// last plain click landed — because that is the piece every application would
/// otherwise reimplement, and the piece that is easy to get wrong when the
/// list changes under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Clear the selection and select this index alone.
    Only(usize),
    /// Flip this index; leave everything else.
    Toggle(usize),
    /// Clear the selection and select this inclusive range, low end first.
    Range(std::ops::RangeInclusive<usize>),
}

/// How a collection's keyboard cursor behaves. See [`Ui::open_collection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavOptions {
    /// How many items there are. The cursor is clamped into it, so a list that
    /// shrinks under the cursor does not leave it pointing past the end.
    pub len: usize,
    /// How far [`Nav::PageNext`](crate::Nav::PageNext) jumps. The collection
    /// knows this and the keymap does not: a screenful is however many rows
    /// are actually on screen.
    pub page: usize,
    /// The cursor runs off the end back to the start. Off by default: in a
    /// long list it is disorienting, and it is the wrong behaviour for a tree.
    /// A combo or a short menu usually wants it on.
    pub wrap: bool,
}

impl Default for NavOptions {
    fn default() -> Self {
        Self { len: 0, page: 10, wrap: false }
    }
}

/// Where a collection's keyboard cursor is, and what the keyboard just did to
/// it. See [`Ui::open_collection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavResponse {
    /// The collection's own id, for [`Ui::cursor_at`].
    pub id: Id,
    /// Which item the keyboard is on. Always a valid index while the
    /// collection is non-empty; 0 when it is empty.
    pub cursor: usize,
    /// The cursor changed this frame. A list that follows its cursor assigns
    /// on this rather than every frame, so the pointer can select a different
    /// row than the keyboard is resting on.
    pub moved: bool,
    /// The collection has keyboard focus.
    pub focused: bool,
    /// Activated from the keyboard (Enter) while focused: open the cursor's
    /// item, the way a double click would.
    pub activated: bool,
    /// [`Nav::Expand`](crate::Nav::Expand) on the cursor — a tree's Right.
    /// Nothing in libgui knows your tree's shape, so this is reported and not
    /// acted on.
    pub expand: bool,
    /// [`Nav::Collapse`](crate::Nav::Collapse) — a tree's Left.
    pub collapse: bool,
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
    pub(crate) animating: bool,
    mouse_prev: Vec2,
    pub(crate) mouse_delta: Vec2,
    prev_down: bool,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
    pub(crate) nodes: Vec<Node>,
    /// Child indices, contiguous per node; `Node.children` is a range into it.
    /// Built through `open_kids`, which is a stack: a container's children sit
    /// at the top of it until the container closes, because any container
    /// opened *inside* it has already taken its own children away again.
    kids: Vec<u32>,
    open_kids: Vec<u32>,
    /// Open containers, innermost last: (node, where its children start in
    /// `open_kids`).
    pub(crate) stack: Vec<(usize, u32)>,
    /// Layers hang off the root while other containers are open, so they
    /// cannot go through `open_kids`. Appended to the root's children when it
    /// closes, which is also where they belong in paint order.
    root_kids: Vec<u32>,
    /// Working memory for `solve` and `paint`, kept across frames.
    scratch: crate::layout::Scratch,
    /// This frame's paint closures, stored inline rather than boxed one by one.
    paints: crate::paint_arena::PaintArena,
    /// This frame's widget text, so a paint closure carries a handle rather
    /// than an owned `String`.
    strs: crate::text_arena::TextArena,
    // Retained state
    rects: FxMap<Id, Rect>,
    hits: Vec<(Id, Rect)>,
    hovered: Option<Id>,
    active: Option<Id>,
    anims: FxMap<(Id, u8), f32>,
    pub(crate) seen: FxSet<Id>,
    /// Next free suffix per colliding base id, so N widgets sharing a key cost
    /// O(N) to disambiguate rather than O(N^2).
    dup_next: FxMap<Id, u32>,
    /// Active `with_key` scopes as (salt, container depth at push).
    key_salt: Vec<(Id, usize)>,
    /// Shortcut scopes: a shortcut only fires if every enclosing scope is active.
    shortcut_scopes: Vec<bool>,
    /// Keys already claimed this frame, so one press drives one command.
    consumed_keys: Vec<Key>,
    /// Actions the focused field was offered this frame and could not use, so
    /// their chords fall through to the app. See [`Ui::release_action`].
    released_actions: Vec<UiAction>,
    /// The last click: which widget, when, and where. One entry, not one per
    /// widget — a double click is two clicks on the *same* widget, so a click
    /// anywhere else ends any sequence in progress.
    last_click: Option<(Id, f64, Vec2)>,
    /// This frame's double-click decision, once something has asked for it.
    ///
    /// Resolving it *consumes* `last_click`, so asking twice used to answer
    /// differently: the second call saw a click on the same widget zero
    /// seconds ago and said yes. A widget may legitimately be interacted with
    /// more than once in a frame — an app asking for the response again — so
    /// the answer is memoised rather than recomputed.
    double_click: Option<(Id, bool)>,
    /// Bytes of document the text widgets read this frame; see
    /// [`crate::testing::FrameCost::text_scanned`].
    pub(crate) text_scanned: usize,
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
    pub(crate) layer_min: FxMap<Id, Vec2>,
    draw: DrawList,
    pub(crate) time: f64,
    // Keyboard focus
    pub(crate) focused: Option<Id>,
    pub(crate) focus_order: Vec<Id>,
    pending_tab: Option<bool>,
    /// Which kinds of widget the keyboard visits, and whether a click moves
    /// focus. Platform convention, so libgui has no opinion: see
    /// [`crate::FocusPolicy`] and `libgui_keymap`.
    pub focus_policy: crate::FocusPolicy,
    /// How long after a click a second one still counts as a double click, in
    /// seconds.
    ///
    /// A **platform convention**, not a library constant: macOS and Windows
    /// both expose it as a system setting, and an app that reads it should
    /// write it here. The default is the value both platforms ship.
    pub double_click_time: f32,
    /// How long a pause in typing closes the current undo step, in seconds.
    ///
    /// A stretch of uninterrupted typing should be one thing to take back, not
    /// one step per keystroke; where the stretch *ends* is taste, and taste is
    /// the app's. Two seconds is the usual figure and the default. Set it to
    /// `0.0` for a step per edit, or `f64::INFINITY` to coalesce until
    /// something else breaks the run (a newline, a command, a click).
    pub undo_run_pause: f64,
    /// False inside a disabled scope. Read it with [`Ui::is_enabled`].
    enabled: bool,
    /// Focus arrived by keyboard, so it is worth drawing a ring. A click moves
    /// focus without one, the way every desktop behaves — a ring that appears
    /// under the mouse is noise.
    focus_visible: bool,
    pub(crate) text_states: FxMap<Id, TextState>,
    /// Per-field undo. Separate from `text_states` because it is the one piece
    /// of text state that is not `Copy`, and because it is pruned on the same
    /// rule: a field that stops being built forgets what was typed into it.
    pub(crate) text_history: FxMap<Id, crate::text_history::History>,
    pub(crate) copied: Option<String>,
    pub(crate) ime_rect: Option<Rect>,
    // Scrolling
    scroll_states: FxMap<Id, ScrollState>,
    /// Scroll areas currently open, innermost last.
    scroll_stack: Vec<Id>,
    /// Focusable widget -> the innermost scroll area it was built in. Kept
    /// across frames, because focus moves at the *end* of a frame, when every
    /// scroll area has already closed and the stack says nothing.
    in_scroll: FxMap<Id, Id>,
    /// "Bring this widget into view": (scroll area, widget). Served at the top
    /// of the area's next frame, where its state and both rects are to hand.
    scroll_requests: Vec<(Id, Id)>,
    // Keyboard navigation inside a collection
    /// Where the keyboard cursor sits in each collection, by the collection's
    /// own id. Swept with the rest when the collection stops being built.
    nav_states: FxMap<Id, usize>,
    /// Collections currently open, innermost last. A row built inside one is
    /// not its own focus stop: the collection is the stop, and the arrows move
    /// within it.
    nav_open: Vec<Id>,
    /// Collection id -> where a range-select extends from. See `Ui::select`.
    select_anchors: FxMap<Id, usize>,
    /// Collection id -> the container node that shows its focus ring.
    ///
    /// A collection is a focus stop without being a node: it has no rect of
    /// its own, so the ring — which `paint` draws around the focused *node* —
    /// had nowhere to go, and tabbing onto a list lit nothing up. It borrows
    /// the ring of the container it was opened in, which is the rect a person
    /// would call "the list" anyway.
    nav_ring: FxMap<Id, Id>,
    // Drag and drop
    pub(crate) dnd: crate::dnd::Dnd,
    /// Zones that accepted the current drag, in paint order; the innermost one
    /// under the pointer is resolved at the next frame's start.
    pub(crate) drop_hits: Vec<(Id, Rect)>,
    pub(crate) drop_hot: Option<Id>,
    /// Pointer travel (logical px) before a press on a drag source becomes a drag.
    pub drag_threshold: f32,
    /// What the last frame asked for, so [`Ui::needs_frame`] can answer before
    /// the next one starts. `Some(0.0)` until a frame has run, because a `Ui`
    /// that has never drawn has everything to do.
    last_repaint: Option<f32>,
    /// Collect the extra counters in [`crate::testing::FrameCost`] that are
    /// not free (today: unkeyed duplicates). Off in a shipping app.
    pub audit: bool,
    /// Ids that only became unique through the build-order fallback, while
    /// `audit` is on.
    dup_ids: FxSet<Id>,
    /// Nodes left out of the tree for nesting past [`crate::layout::MAX_DEPTH`].
    too_deep: u32,
    /// Paragraphs built this frame: (node, text, size, alignment). A wrapping
    /// paragraph's height depends on the width it is *given*, which layout
    /// only knows after it has run, so these are re-measured between the two
    /// passes. See `Ui::rewrap`.
    pub(crate) wrapping: Vec<(u32, crate::FrameText, f32)>,
    cost: crate::testing::FrameCost,
    profile: crate::Profile,
    /// Subtree recordings, for [`Ui::cached`].
    cache: crate::subtree_cache::Cache,
    rects_order: Vec<(Id, Rect)>,
    /// Ids handed to `cached` this frame, so recordings for subtrees the app
    /// stopped building can be swept.
    cached_seen: FxSet<Id>,
    /// The theme the cache was recorded against: a theme change invalidates
    /// every pixel in it.
    /// The theme recordings were made under, compared rather than hashed: the
    /// app owns `ui.theme` and can edit a colour in place.
    cache_theme: Theme,
    /// Counts animations that have not reached their target. Compared across
    /// a `cached` body: the global `animating` flag cannot be, because
    /// anything outside the subtree — an easing scroll, a spinner elsewhere —
    /// sets it first and hides the subtree's own.
    unsettled: u64,
    /// Subtrees that were animating when they last built, so a replay cannot
    /// freeze a fade half way.
    cache_busy: FxSet<Id>,
    scroll_hits: Vec<(Id, Rect)>,
    pub(crate) scroll_target: Option<Id>,
    /// Hit rects that win over normal widgets (splitters).
    top_hits: Vec<(Id, Rect)>,
    overlays: Vec<OverlayFn>,
    // Touch
    /// Finger travel (logical px) before a tap turns into a scroll.
    pub touch_slop: f32,
    /// How input becomes scroll motion, for every area that does not override
    /// it through [`ScrollOptions::config`]. See [`ScrollConfig`].
    pub scroll: ScrollConfig,
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
    /// Grow the hit area by this many px on every side — thin things like
    /// splitters and column grips are easier to grab than they are to see.
    ///
    /// The widget grows, its clip does not: the hit area still stops at
    /// whatever the ancestors clipped to.
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

    /// Build a `Ui` whose default font is a [`FontStack`](crate::FontStack):
    /// the first face, then the rest as fallbacks for what it cannot draw.
    ///
    /// This is the one-line form of the common case. A UI that shows names,
    /// filenames or user text in any script wants it, because the alternative
    /// is blank glyphs rather than a graceful loss of style. Which fonts go in
    /// the list is yours to decide — libgui does not go looking for system
    /// fonts.
    #[cfg(feature = "fontdue")]
    pub fn with_fallbacks(theme: Theme, fonts: &[&[u8]]) -> Result<Self, crate::FontError> {
        let stack = crate::FontStack::from_fonts(fonts)?;
        Ok(Self::with_rasterizer(theme, Box::new(stack)))
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
            kids: Vec::new(),
            open_kids: Vec::new(),
            stack: Vec::new(),
            root_kids: Vec::new(),
            scratch: crate::layout::Scratch::default(),
            paints: crate::paint_arena::PaintArena::default(),
            strs: crate::text_arena::TextArena::default(),
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
            released_actions: Vec::new(),
            last_click: None,
            double_click: None,
            text_scanned: 0,
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
            focus_policy: crate::FocusPolicy::default(),
            double_click_time: 0.5,
            undo_run_pause: crate::text_history::DEFAULT_RUN_PAUSE,
            enabled: true,
            focus_visible: false,
            text_states: FxMap::default(),
            text_history: FxMap::default(),
            copied: None,
            ime_rect: None,
            scroll_states: FxMap::default(),
            scroll_stack: Vec::new(),
            in_scroll: FxMap::default(),
            scroll_requests: Vec::new(),
            nav_states: FxMap::default(),
            nav_open: Vec::new(),
            nav_ring: FxMap::default(),
            select_anchors: FxMap::default(),
            scroll_hits: Vec::new(),
            scroll_target: None,
            dnd: crate::dnd::Dnd::Idle,
            drop_hits: Vec::new(),
            drop_hot: None,
            drag_threshold: 4.0,
            last_repaint: Some(0.0),
            audit: false,
            dup_ids: FxSet::default(),
            too_deep: 0,
            wrapping: Vec::new(),
            cost: crate::testing::FrameCost::default(),
            profile: crate::Profile::default(),
            cache: crate::subtree_cache::Cache::default(),
            rects_order: Vec::new(),
            cached_seen: FxSet::default(),
            cache_theme: Theme::dark(),
            unsettled: 0,
            cache_busy: FxSet::default(),
            top_hits: Vec::new(),
            overlays: Vec::new(),
            touch_slop: 8.0,
            scroll: ScrollConfig::default(),
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

    /// Install the chords that perform libgui's widget actions (caret
    /// movement, deletion, clipboard, Submit/Cancel, focus traversal).
    ///
    /// libgui has no defaults and no idea what platform it is on: until this
    /// is called, keys are only keys, and a text field takes typed text but
    /// does nothing on Backspace. `libgui_keymap` builds per-platform tables;
    /// a host with its own keymap can instead send [`InputEvent::Action`].
    pub fn set_key_bindings(&mut self, bindings: KeyBindings) {
        self.input_state.bindings = bindings;
    }

    pub fn key_bindings(&self) -> &KeyBindings {
        &self.input_state.bindings
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
    /// cannot drive two commands. Refuses inside a
    /// [`shortcut_scope`](Ui::shortcut_scope) that is not active, and while a
    /// text field has focus refuses the keys that belong to the field: any
    /// key the [`KeyBindings`] turned into an action this frame (Backspace,
    /// arrows, Cmd+A…) and any chord that types a character (`Space`, `F`).
    /// A chord with Ctrl or Cmd that the field does not use (`Cmd+S`) still
    /// gets through.
    ///
    /// The chord is concrete. Choosing Cmd+S on a Mac and Ctrl+S elsewhere is
    /// the app's keymap (`libgui_keymap::Keymap::triggered` does it):
    ///
    /// ```ignore
    /// if ui.consume_shortcut(Shortcut::plain(Key::S).ctrl()) { save(); }
    /// ```
    /// Called by a widget that was handed a [`UiAction`] this frame and could
    /// not use it, so the chord behind it goes back to the app. Cleared every
    /// frame; see [`Ui::consume_shortcut`].
    pub fn release_action(&mut self, action: UiAction) {
        if !self.released_actions.contains(&action) {
            self.released_actions.push(action);
        }
    }

    pub fn consume_shortcut(&mut self, sc: Shortcut) -> bool {
        if !self.shortcut_scopes.iter().all(|&a| a) {
            return false;
        }
        if self.typing && (sc.types_text() || self.input.keys_bound.contains(&sc.key)) {
            // …unless the focused field met that action this frame and had
            // nothing to do with it. An undo chord over a field with an empty
            // history is the app's, the way an NSTextView shares its window's
            // undo manager — otherwise a caret resting in a search box makes
            // the app's own undo unreachable.
            let released = self
                .key_bindings()
                .resolve(sc.key, &self.input.modifiers)
                .is_some_and(|a| self.released_actions.contains(&a));
            if !released {
                return false;
            }
        }
        if !self.input.keys_pressed.contains(&sc.key) || self.consumed_keys.contains(&sc.key) {
            return false;
        }
        if !sc.matches(sc.key, &self.input.modifiers) {
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
        self.mark_seen(id);
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
        n.paint = Some(self.paints.push(|_: &mut Painter, _: Rect| {}));
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.root_kids.push(idx as u32);
    }

    /// Show `body` in a popup panel if `id` is open, positioned near its anchor
    /// and kept on screen. Returns whether it was shown.
    ///
    /// Open it with [`Ui::open_popup`]; it closes on a click outside, on Escape,
    /// or when you call [`Ui::close_popups`] (what a menu item does).
    pub fn popup<R>(&mut self, id: Id, min_width: f32, body: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if !self.open_popup_body(id, min_width) {
            return None;
        }
        let r = body(self);
        self.close_popup_body();
        Some(r)
    }

    /// Open a popup's panel without a closure, for a binding that cannot hold
    /// one. Returns false when the popup is not open, in which case its body
    /// must not be built and [`Ui::close_popup_body`] must NOT be called.
    ///
    /// This is the third place the same split was needed — containers, scroll
    /// areas, popups — which is what a closure-based API costs a language
    /// without closures.
    pub fn open_popup_body(&mut self, id: Id, min_width: f32) -> bool {
        if !self.popup_open(id) {
            return false;
        }
        self.popup_sheet();
        if self.input.events.contains(&UiEvent::Action(UiAction::Cancel)) {
            // Innermost first: Escape backs out one level.
            if self.open_chain.last() == Some(&id) {
                self.open_chain.pop();
                return false;
            }
        }
        let anchor = self.open_anchors.get(&id).copied().unwrap_or_default();
        // The panel sizes itself to its content, but its rect is also what
        // positions it, so the content size comes from last frame's measure.
        // On the first frame it opens at `min_width` and settles on the next,
        // the same one-frame rule as `Response::rect`.
        let fitted = match self.layer_min.get(&id).copied() {
            Some(v) => v,
            None => {
                // First frame open: the content has not been measured yet, so
                // this frame is provisional. Ask for another one — otherwise a
                // host that draws only when `needs_frame` says to would leave
                // the popup pinned at zero height until unrelated input
                // happened to wake it.
                self.animating = true;
                Vec2::new(min_width, 0.0)
            }
        };
        let rect = self.place_popup(anchor, Vec2::new(fitted.x.max(min_width), fitted.y));
        let s = self.theme.menu;
        let (pad, gap) = (s.padding, s.gap);

        self.mark_seen(id);
        let layout = Layout::column().width(Size::Fit).height(Size::Fit).padding(pad).gap(gap);
        let mut n = Node::new(id, layout);
        n.absolute = Some(rect);
        n.z = Layer::Popup;
        n.clip = true;
        n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
            p.shadow(r.translate(0.0, 6.0), s.radius, 24.0, p.theme.palette.shadow);
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
        }));
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.root_kids.push(idx as u32);

        self.popup_stack.push(id);
        self.open(idx);
        true
    }

    /// Close the panel opened by [`Ui::open_popup_body`].
    pub fn close_popup_body(&mut self) {
        self.close();
        self.popup_stack.pop();
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
        self.mark_seen(id);
        let text = text.to_string();
        let mut n = Node::new(id, Layout::leaf(Size::Fixed(w), Size::Fixed(h)));
        n.absolute = Some(rect);
        n.z = Layer::Tooltip;
        n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
            p.shadow(r.translate(0.0, 3.0), s.radius, 12.0, p.theme.palette.shadow);
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
            p.text_left(r.shrink(pad.left, 0.0, pad.right, 0.0), size, s.text, &text);
        }));
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.root_kids.push(idx as u32);
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

    /// Whether the next frame would differ from the last one, answered
    /// **before** building it.
    ///
    /// [`PlatformOutput::repaint_after`] already lets a host that only draws
    /// for the UI go to sleep. This is for the host that does not: an engine
    /// running its viewport at 120 Hz, a DAW drawing meters, a video editor
    /// playing back. Those redraw every frame for their own reasons, and
    /// without asking they would rebuild, re-lay-out and re-paint a UI that
    /// has not moved — measured at three fifths of the frame in paint alone.
    ///
    /// `elapsed` is seconds since the last frame, the same clock that feeds
    /// [`FrameInfo::dt`]. When this returns false, skip `begin_frame` and
    /// `end_frame` entirely and redraw the geometry you already uploaded:
    /// nothing about the UI has changed, so the last frame's draw list is
    /// still correct.
    ///
    /// ```ignore
    /// let elapsed = now - last_ui_frame;
    /// if ui.needs_frame(elapsed) {
    ///     ui.begin_frame(FrameInfo { dt: elapsed, ..info });
    ///     build(&mut ui);
    ///     let out = ui.end_frame();
    ///     renderer.upload(&out);            // only now
    ///     last_ui_frame = now;
    /// }
    /// renderer.draw(&mut pass);             // every frame, from what it has
    /// ```
    ///
    /// Queued input always wins: pushing an event makes this true whatever
    /// the last frame asked for.
    /// What an input method is composing, shown inline at the focused field's
    /// caret and not part of its value until the host commits it.
    pub(crate) fn preedit(&self) -> Option<(&str, usize)> {
        self.input_state.preedit()
    }

    pub fn needs_frame(&self, elapsed: f32) -> bool {
        if self.input_state.has_pending() {
            return true;
        }
        match self.last_repaint {
            Some(after) => elapsed >= after,
            None => false,
        }
    }

    /// [`Ui::needs_frame`] for the frame the host is about to build, which is
    /// what a host that can be resized should call.
    ///
    /// A resize is not input: no `InputEvent` describes it, and the new size
    /// arrives only in [`FrameInfo`]. So `needs_frame` alone cannot see one,
    /// and a host that gates on it reuses the batches it built at the *old*
    /// size until some unrelated event happens to wake it — the window edge
    /// moves and the UI inside it does not follow. Passing the info the frame
    /// would be built with closes that hole.
    pub fn needs_frame_for(&self, info: &FrameInfo, elapsed: f32) -> bool {
        if info.screen_size != self.input.screen_size || info.scale != self.input.scale {
            return true;
        }
        self.needs_frame(elapsed)
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
        // A host's clock and window metrics arrive from outside, and one bad
        // value would not merely draw a bad frame: `dt` drives animation, and
        // a NaN stored in retained state stays there. (A real one: a host that
        // times frames across a suspend, or divides by a zero refresh rate.)
        let info = FrameInfo {
            screen_size: Vec2::new(sane(info.screen_size.x, 0.0).max(0.0), sane(info.screen_size.y, 0.0).max(0.0)),
            scale: {
                let s = sane(info.scale, 1.0);
                if s > 0.0 { s } else { 1.0 }
            },
            // Clamped, not just made finite: a paused-then-resumed app hands
            // over the whole pause as one step, which would fling every
            // animation straight to its target.
            dt: sane(info.dt, 0.0).clamp(0.0, 0.25),
        };
        let mut input = self.input_state.frame(info);
        // Scroll units reach `FrameInput` unconverted, because what a notch is
        // worth is the app's to set. Everything that just wants a number in px
        // — a canvas's zoom, `Response::scroll` — reads the total; a scroll
        // area reads the three apart, because how a delta is smoothed depends
        // on which one it came in through.
        let cfg = self.scroll;
        let steps = |lines: f32, pages: f32| cfg.steps_to_px(lines, pages);
        input.scroll = input.scroll_px
            + Vec2::new(steps(input.scroll_lines.x, input.scroll_pages.x), steps(input.scroll_lines.y, input.scroll_pages.y));
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
            UiEvent::Action(UiAction::FocusNext) => Some(false),
            UiEvent::Action(UiAction::FocusPrevious) => Some(true),
            _ => None,
        });
        self.time += input.dt as f64;
        self.focus_order.clear();
        self.overlays.clear();
        self.ime_rect = None;
        self.cursor = Cursor::Default;
        self.input = input;
        self.nodes.clear();
        self.paints.clear();
        self.strs.clear();
        self.kids.clear();
        self.open_kids.clear();
        self.root_kids.clear();
        self.stack.clear();
        self.seen.clear();
        self.dup_next.clear();
        self.key_salt.clear();
        self.shortcut_scopes.clear();
        self.consumed_keys.clear();
        self.released_actions.clear();
        // A press somewhere else ends any double-click in progress — on
        // another widget or on nothing at all. Without this, clicking a
        // button, then the background, then the button again reports a double
        // click, which is not what the two clicks meant.
        if self.pressed && self.hovered != self.last_click.map(|(id, _, _)| id) {
            self.last_click = None;
        }
        self.double_click = None;
        self.text_scanned = 0;
        // Focus is resolved during a frame, so this is last frame's answer —
        // the same one-frame-late rule the rest of the input model uses.
        self.typing = self.focused.is_some();
        self.sheet_done = false;
        self.rects_order.clear();
        self.too_deep = 0;
        self.wrapping.clear();
        self.cached_seen.clear();
        self.dup_ids.clear();
        let _ = self.fonts.take_rasterized();
        // Between frames is the only safe moment: a repack moves every glyph,
        // and last frame's instances are gone while this frame's are not
        // emitted yet. Recordings made under the old packing are dropped by
        // the `atlas_repacks` check in `Ui::cached`.
        self.fonts.repack();
        let _ = self.fonts.take_shaped_runs();
        self.dnd_begin_frame();
        self.popup_stack.clear();
        self.xform_stack.clear();
        let s = self.input.screen_size;
        let root = Node::new(Id::new("root"), Layout::column().width(Size::Fixed(s.x)).height(Size::Fixed(s.y)));
        self.nodes.push(root);
        self.open(0);
    }

    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        debug_assert_eq!(self.stack.len(), 1, "unbalanced containers");
        // Layers were collected aside while other containers were open; they
        // join the root's children now, after its flow content, which is the
        // order `paint` and `place` already expect of absolute nodes.
        self.open_kids.extend_from_slice(&self.root_kids);
        self.close();
        let s = self.input.screen_size;
        let screen = Rect::new(0.0, 0.0, s.x, s.y);
        let clock = crate::profile::Clock::start();
        let t = crate::profile::Clock::start();
        layout::fit(&mut self.nodes, &self.kids, 0);
        let measure_ms = t.ms();
        let t = crate::profile::Clock::start();
        layout::arrange(&mut self.nodes, &self.kids, 0, screen, &mut self.scratch);
        // A paragraph's height follows from the width it was given, which is
        // only known now. Where that changed the answer, the solve is worth
        // running again — once. In the steady state widths do not move, so
        // this costs a walk of the paragraphs and nothing else.
        if self.rewrap() {
            layout::fit(&mut self.nodes, &self.kids, 0);
            layout::arrange(&mut self.nodes, &self.kids, 0, screen, &mut self.scratch);
        }
        let place_ms = t.ms();
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
        self.drop_hits.clear();
        let scale = self.input.scale.max(0.01);
        let mut painter =
            Painter { draw: &mut self.draw, fonts: &mut self.fonts, theme: &self.theme, font: self.font, strs: self.strs.bytes(), scale };
        let mut sink = HitSink {
            hits: &mut self.hits,
            rects: &mut self.rects,
            scroll_hits: &mut self.scroll_hits,
            top_hits: &mut self.top_hits,
            drop_hits: &mut self.drop_hits,
            rects_order: &mut self.rects_order,
            recording: 0,
            offscreen: 0,
        };
        let t = crate::profile::Clock::start();
        // A focused collection shows its ring on the container it lives in.
        let focus_ring = self
            .focus_visible
            .then_some(self.focused)
            .flatten()
            .map(|id| self.nav_ring.get(&id).copied().unwrap_or(id));
        paint(&mut self.nodes, &self.kids, 0, &mut painter, &mut sink, &mut self.scratch, &mut self.paints, &mut self.cache, focus_ring);
        for overlay in self.overlays.drain(..) {
            overlay(&mut painter);
        }
        let paint_ms = t.ms();
        let offscreen = sink.offscreen;
        let dup = &self.dup_ids;
        self.cost = crate::testing::FrameCost {
            nodes: self.nodes.len(),
            instances: self.draw.instances.len(),
            batches: self.draw.batches.len(),
            glyphs_rasterized: self.fonts.take_rasterized(),
            text_shaped: self.fonts.take_shaped_runs(),
            offscreen_nodes: offscreen,
            // Only the interactive ones matter: `space` and `separator` share
            // a key by design and have no state to lose. The scan is O(nodes),
            // so it only runs when someone asked for it.
            too_deep: self.too_deep,
            unkeyed_duplicates: match self.audit {
                true => self.nodes.iter().filter(|n| n.interactive && dup.contains(&n.id)).count() as u32,
                false => 0,
            },
            text_scanned: self.text_scanned,
        };

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
            self.focus_visible = true;
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
                let to = order[next];
                self.focused = Some(to);
                // Focus that cannot be seen is not focus. Tabbing into a form
                // taller than its viewport used to leave the ring drawn
                // somewhere off screen, which makes any such form unusable
                // from the keyboard.
                self.scroll_to(to);
            }
        }

        if self.released {
            self.active = None;
            self.active_drag = false;
        }
        let seen = &self.seen;
        self.anims.retain(|(id, _), _| seen.contains(id));
        self.text_states.retain(|id, _| seen.contains(id));
        self.text_history.retain(|id, _| seen.contains(id));
        self.scroll_states.retain(|id, _| seen.contains(id));
        self.in_scroll.retain(|id, _| seen.contains(id));
        self.nav_states.retain(|id, _| seen.contains(id));
        self.nav_ring.retain(|id, _| seen.contains(id));
        self.select_anchors.retain(|id, _| seen.contains(id));
        if self.focused.is_some_and(|f| !seen.contains(&f)) {
            self.focused = None;
        }

        self.dnd_end_frame();
        let used = std::mem::take(&mut self.cached_seen);
        self.cache.sweep(&used);
        self.cached_seen = used;

        // A glyph that did not fit is drawn on the frame after the repack, so
        // an otherwise idle UI has to be asked for that one more frame.
        let busy =
            self.active.is_some() || self.touch_scroll.is_some() || self.animating || self.fonts.repack_pending();
        let repaint_after = if busy {
            Some(0.0)
        } else if self.focused.is_some() {
            Some(0.5) // caret blink
        } else {
            None
        };
        self.last_repaint = repaint_after;
        self.profile = crate::Profile {
            measure_ms,
            place_ms,
            paint_ms,
            end_frame_ms: 0.0,
            nodes: self.nodes.len(),
            instances: self.draw.instances.len(),
            text_draws: self.fonts.take_text_draws(),
            cached_hits: std::mem::take(&mut self.cache.hits_this_frame),
            cached_misses: std::mem::take(&mut self.cache.misses_this_frame),
        };
        // Last, so it covers everything above it — including itself being
        // assigned after the phases it is the sum of.
        self.profile.end_frame_ms = clock.ms();
        let platform = PlatformOutput {
            cursor: self.cursor,
            copied_text: self.copied.take(),
            paste_requested: self.input_state.paste_requested && self.focused.is_some(),
            text_input: self.focused.and(self.ime_rect),
            wants_pointer: self.wants_pointer(),
            wants_keyboard: self.wants_keyboard(),
            pointer_lock: self.lock_request,
            repaint_after,
        };
        FrameOutput {
            profile: self.profile,
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
    /// Note that `id` exists this frame, so its retained state is not pruned.
    /// While a [`Ui::cached`] subtree is recording, the id is kept so a later
    /// replay can say the same thing on its behalf.
    pub(crate) fn mark_seen(&mut self, id: Id) -> bool {
        let fresh = self.seen.insert(id);
        if self.cache.recording > 0 {
            self.cache.saw(id);
        }
        fresh
    }

    pub fn make_id(&mut self, src: impl Hash) -> Id {
        let parent = self.nodes[self.stack.last().expect("libgui: widget built outside begin_frame/end_frame").0].id;
        // A `with_key` scope salts the widgets built directly inside it. Nested
        // containers inherit it through their own (already salted) id, so the
        // salt is mixed in exactly once.
        let base = match self.key_salt.last() {
            Some(&(salt, depth)) if depth == self.stack.len() => parent.with(salt.0).with(&src),
            _ => parent.with(&src),
        };
        if self.mark_seen(base) {
            return base;
        }
        let mut n = *self.dup_next.get(&base).unwrap_or(&1);
        let mut id = base.with(n);
        // Loops only on a genuine hash collision with an unrelated id.
        while !self.mark_seen(id) {
            n += 1;
            id = base.with(n);
        }
        self.dup_next.insert(base, n + 1);
        if self.audit {
            self.dup_ids.insert(id);
        }
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

    /// Register `id` as a place the keyboard can go, and report what it did.
    ///
    /// Whether this kind of widget is visited at all is [`Ui::focus_policy`],
    /// which is the app's to set because it is a platform convention rather
    /// than a fact — see [`crate::focus`]. A widget that is not visited still
    /// works with the mouse; it simply never has focus.
    pub fn focusable(&mut self, id: Id, kind: crate::FocusKind) -> crate::KeyResponse {
        // Not a focus stop, and not somewhere focus may stay: a scope that
        // switches off while a widget inside it has focus must hand it back.
        if !self.enabled {
            if self.focused == Some(id) {
                self.focused = None;
            }
            return crate::KeyResponse::default();
        }
        // A row built inside an open collection is not a focus stop of its
        // own: the collection is the stop and the arrows move within it, which
        // is what `FocusKind::Collection` has always promised. Without this,
        // Tab walks every row of a thousand-row list one at a time.
        if kind == crate::FocusKind::Collection && !self.nav_open.is_empty() {
            if self.focused == Some(id) {
                self.focused = self.nav_open.last().copied();
            }
            return crate::KeyResponse::default();
        }
        if !self.focus_policy.accepts(kind) {
            // Focus must not be left on something the policy no longer visits.
            if self.focused == Some(id) {
                self.focused = None;
            }
            return crate::KeyResponse::default();
        }
        self.focus_order.push(id);
        // Which area would have to move to show this widget. Recorded here
        // rather than looked up later: by the time focus moves, at the end of
        // the frame, every scroll area has closed.
        if let Some(&area) = self.scroll_stack.last() {
            self.in_scroll.insert(id, area);
        }
        let focused = self.focused == Some(id);
        if !focused {
            return crate::KeyResponse::default();
        }
        // What chord produces Submit is the keymap's, and a host with no
        // keyboard can send the action straight in.
        let activated = self.take_action(UiAction::Submit);
        if self.take_action(UiAction::Cancel) {
            self.focused = None;
        }
        crate::KeyResponse { focused, activated }
    }

    /// [`Ui::interact`] and [`Ui::focusable`] together, which is what a
    /// keyboard-reachable widget wants: `Response::clicked` is then true
    /// whether it was pressed or activated from the keyboard.
    pub fn interact_focusable(&mut self, id: Id, kind: crate::FocusKind) -> Response {
        let mut r = self.interact(id);
        // A press moves focus where the platform says it should. Text fields
        // take it regardless: there is nowhere else for a caret to be.
        if r.pressed && (self.focus_policy.click_focuses || kind == crate::FocusKind::Text) {
            self.focused = Some(id);
            self.focus_visible = false;
        }
        let k = self.focusable(id, kind);
        r.focused = k.focused;
        r.clicked |= k.activated;
        r
    }

    /// [`Ui::interact_focusable`] for a widget that owns its drag (a slider, a
    /// drag value): on touch it keeps the finger rather than handing it to the
    /// scroll area around it.
    pub fn interact_focusable_drag(&mut self, id: Id, kind: crate::FocusKind) -> Response {
        let mut r = self.interact_drag(id);
        if r.pressed && (self.focus_policy.click_focuses || kind == crate::FocusKind::Text) {
            self.focused = Some(id);
            self.focus_visible = false;
        }
        let k = self.focusable(id, kind);
        r.focused = k.focused;
        r.clicked |= k.activated;
        r
    }

    /// Take a pending [`UiAction`] if one arrived this frame, so two widgets
    /// cannot both act on it.
    fn take_action(&mut self, want: UiAction) -> bool {
        let at = self.input.events.iter().position(|e| *e == UiEvent::Action(want));
        match at {
            Some(i) => {
                self.input.events.remove(i);
                true
            }
            None => false,
        }
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
        // A disabled widget reports nothing at all: not hovered, not clicked,
        // no drag. Resolved here rather than in each widget, so a widget added
        // later cannot forget to check.
        if !self.enabled {
            return Response { id, rect, ..Response::default() };
        }
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
            focused: self.focused == Some(id),
            active,
            pressed: hovered && self.pressed,
            clicked: active && hovered && self.released,
            double_clicked: active && hovered && self.released && self.is_double_click(id),
            // Zero on the frame the drag starts: the pointer movement that
            // brought it onto the widget happened *before* the press, and with
            // a teleporting pointer (a pen, synthetic input) that jump is large.
            drag_delta: if active && !started { delta * (1.0 / t.zoom) } else { Vec2::ZERO },
            raw_delta: if active { self.input.raw_delta } else { None },
            secondary_pressed: over_now && self.input.buttons_pressed[PointerButton::Secondary.index()],
            middle_pressed: over_now && self.input.buttons_pressed[PointerButton::Middle.index()],
            scroll: if hovered { self.input.scroll } else { Vec2::ZERO },
            mouse_pos: t.inv_point(self.input.mouse_pos),
            modifiers: self.input.modifiers,
            pinch: if over { self.gesture.zoom - 1.0 } else { 0.0 },
            pan2: if over { self.gesture.pan } else { Vec2::ZERO },
        }
    }

    /// Was this release the second click of a double click?
    ///
    /// Two clicks on the same widget, within [`Ui::double_click_time`] and
    /// without the pointer wandering more than a few pixels between them —
    /// the third click starts a new pair rather than reporting again, so a
    /// rapid run of clicks alternates single, double, single, double instead
    /// of firing a double every frame.
    fn is_double_click(&mut self, id: Id) -> bool {
        if let Some((who, answer)) = self.double_click {
            if who == id {
                return answer;
            }
        }
        let at = self.input.mouse_pos;
        let now = self.time;
        let double = match self.last_click {
            Some((prev, when, pos)) => {
                prev == id
                    && now - when <= self.double_click_time as f64
                    && (pos.x - at.x).abs() <= 4.0
                    && (pos.y - at.y).abs() <= 4.0
            }
            None => false,
        };
        self.last_click = if double { None } else { Some((id, now, at)) };
        self.double_click = Some((id, double));
        double
    }

    /// Retained animation value: eases towards `target` each frame.
    pub fn animate(&mut self, id: Id, slot: u8, target: f32) -> f32 {
        let k = 1.0 - (-self.theme.metrics.anim_speed * self.input.dt).exp();
        let target = sane(target, 0.0);
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.001 || !v.is_finite() {
            *v = target;
        }
        let v = *v;
        self.animating |= v != target;
        self.unsettled += (v != target) as u64;
        v
    }

    /// Like [`Ui::animate`] with an explicit rate (1/s).
    pub fn animate_with_speed(&mut self, id: Id, slot: u8, target: f32, speed: f32) -> f32 {
        let k = 1.0 - (-speed * self.input.dt).exp();
        let target = sane(target, 0.0);
        let v = self.anims.entry((id, slot)).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.01 || !v.is_finite() {
            *v = target;
        }
        let v = *v;
        self.animating |= v != target;
        self.unsettled += (v != target) as u64;
        v
    }

    /// Jump an animation to `value` (it then eases towards its next target).
    pub fn set_anim(&mut self, id: Id, slot: u8, value: f32) {
        self.anims.insert((id, slot), sane(value, 0.0));
    }

    /// Mark an explicitly-constructed id as alive this frame so its retained
    /// state (animations, text/scroll state) is kept.
    pub fn keep_id(&mut self, id: Id) {
        self.mark_seen(id);
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

    /// Copy `text` into the frame's text arena and return a handle to it.
    ///
    /// A widget is declared before its rect is known, so its paint closure has
    /// to own the text it will draw. Owning a `String` means an allocation per
    /// text widget per frame; a [`FrameText`](crate::FrameText) is eight bytes naming a range in
    /// a buffer that is reused, so the copy lands in space that already exists.
    ///
    /// The handle is valid for **this frame only**, which is exactly as long as
    /// the paint closure carrying it. Do not keep one: a stale handle resolves
    /// to `""` rather than to somebody else's text.
    pub fn frame_text(&mut self, text: &str) -> crate::FrameText {
        self.strs.push(text)
    }

    /// Size the per-frame buffers for about `widgets` widgets, up front.
    ///
    /// A `Ui` allocates only while its arenas grow into the shape of your
    /// frame: after that a frame costs nothing but the `String` each text
    /// widget's paint closure owns. That growth is a handful of allocations
    /// over the first frames, which is invisible in an app and very visible in
    /// a real-time thread that must not touch the allocator at all. Call this
    /// once, with a generous guess: a container counts as a widget, buffers
    /// only ever grow, and an overestimate costs memory while an underestimate
    /// costs a few reallocations later.
    ///
    /// Nothing else is needed to use your own allocator: libgui keeps no
    /// process-global state, so it allocates through whatever
    /// `#[global_allocator]` your binary installs.
    pub fn reserve(&mut self, widgets: usize) {
        // The root, and the slack that keeps an off-by-a-few guess free.
        let n = widgets + 16;
        self.nodes.reserve(n.saturating_sub(self.nodes.capacity()));
        self.kids.reserve(n.saturating_sub(self.kids.capacity()));
        // Worst case every widget is a child of one still-open container.
        self.open_kids.reserve(n.saturating_sub(self.open_kids.capacity()));
        self.hits.reserve(widgets.saturating_sub(self.hits.capacity()));
        self.rects.reserve(widgets.saturating_sub(self.rects.len()));
        self.seen.reserve(widgets.saturating_sub(self.seen.len()));
        self.focus_order.reserve(64);
        self.paints.reserve(widgets);
        // A label averages a couple of dozen characters.
        self.strs.reserve(widgets * 24);
        self.scratch.reserve(widgets);
        self.anims.reserve(widgets.saturating_sub(self.anims.len()));
        self.dup_next.reserve(widgets.saturating_sub(self.dup_next.len()));
        self.scroll_hits.reserve(64);
        self.top_hits.reserve(64);
        self.drop_hits.reserve(64);
        self.consumed_keys.reserve(16);
        self.key_salt.reserve(64);
        self.shortcut_scopes.reserve(64);
        self.xform_stack.reserve(16);
        self.popup_stack.reserve(16);
        self.stack.reserve(64);
        self.overlays.reserve(16);
        self.layer_min.reserve(64);
        // Widgets average a few instances each: a box, a border, some glyphs.
        self.draw.reserve(widgets * 4);
    }

    /// Where the last frame's time went. All zeroes unless the `profile`
    /// feature is on; see [`crate::Profile`].
    pub fn profile(&self) -> crate::Profile {
        self.profile
    }

    /// What the last completed frame cost. See [`crate::testing`].
    /// This frame's draw list, after [`Ui::end_frame`].
    ///
    /// The same data [`FrameOutput::draw`] carries, reachable without holding
    /// the output — which is what a renderer expanding the frame into
    /// triangles needs, since [`crate::mesh::Mesh::build`] takes the list and
    /// the output borrows the `Ui`.
    pub fn draw_list(&self) -> &crate::DrawList {
        &self.draw
    }

    pub fn frame_cost(&self) -> crate::testing::FrameCost {
        self.cost
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

    fn attach(&mut self, mut node: Node) -> usize {
        debug_assert!(!self.stack.is_empty(), "libgui: widget built outside begin_frame/end_frame");
        // Stamped on every node built in a disabled scope. `paint` *sets* this
        // rather than multiplying, so a whole subtree carrying the same value
        // fades once, however deeply it nests.
        if !self.enabled {
            node.alpha = self.theme.metrics.disabled_alpha;
        }
        let idx = self.nodes.len();
        self.nodes.push(node);
        if self.stack.len() <= crate::layout::MAX_DEPTH {
            self.open_kids.push(idx as u32);
        } else {
            // Left out of the tree, so the recursive passes never reach it.
            // One place, rather than a depth check in each of the three.
            self.too_deep += 1;
        }
        idx
    }

    /// Start collecting children for `i`.
    fn open(&mut self, i: usize) {
        self.stack.push((i, self.open_kids.len() as u32));
    }

    /// Finish the innermost container: its children are whatever was added
    /// since `open`, and they are contiguous, because every container opened
    /// inside it took its own children off the top before this point.
    fn close(&mut self) {
        let (i, mark) = self.stack.pop().expect("libgui: unbalanced containers");
        // A scroll area leaves the stack whichever way it was closed: the
        // closure form and the open/close pair both land here.
        if self.nodes[i].scroll.is_some() {
            self.scroll_stack.pop();
        }
        let start = self.kids.len() as u32;
        self.kids.extend_from_slice(&self.open_kids[mark as usize..]);
        self.open_kids.truncate(mark as usize);
        self.nodes[i].children = Kids { start, len: self.kids.len() as u32 - start };
    }

    /// How many children the open container has so far: the position a bare
    /// `container` derives its id from.
    fn open_child_count(&self) -> usize {
        let mark = self.stack.last().map_or(0, |&(_, m)| m as usize);
        self.open_kids.len() - mark
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
        self.mark_seen(id);
        let mut n = Node::new(id, layout);
        n.intrinsic = content;
        n.interactive = opts.interactive;
        n.hit_pad = opts.hit_pad;
        n.hit_top = opts.hit_top;
        n.paint = Some(self.paints.push(paint));
        self.attach(n);
    }

    /// Generic container. Children added inside `body` are laid out by `layout`.
    pub fn container<R>(&mut self, layout: Layout, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        let idx = self.open_child_count();
        let id = self.make_id(("container", idx));
        self.container_id(id, layout, frame, body)
    }

    /// Container with an explicit id: its rect is queryable via `rect_of`, and
    /// children's ids derive from it, so their state follows it around.
    pub fn container_id<R>(&mut self, id: Id, layout: Layout, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.open_container(id, layout, frame);
        let r = body(self);
        self.close_container();
        r
    }

    /// Open a container without a closure, for callers that cannot hold one:
    /// a C or scripting binding, or generated code. Rust code should prefer
    /// [`Ui::container`] and [`Ui::container_id`], which cannot forget to
    /// close.
    ///
    /// Every `open_container` must be matched by exactly one
    /// [`Ui::close_container`] before [`Ui::end_frame`]. Forgetting one panics
    /// there rather than silently reparenting the rest of the window, and a
    /// binding should check [`Ui::open_depth`] to report the caller's mistake
    /// as an error of its own instead.
    pub fn open_container(&mut self, id: Id, layout: Layout, frame: Frame) {
        self.mark_seen(id);
        let mut n = Node::new(id, layout);
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 || frame.shadow {
            n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
                if frame.shadow {
                    p.shadow(r.translate(0.0, 4.0), frame.radius, 16.0, p.theme.palette.shadow);
                }
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        let i = self.attach(n);
        self.open(i);
    }

    /// Close the innermost [`Ui::open_container`]. Panics if none is open.
    pub fn close_container(&mut self) {
        assert!(self.open_depth() > 0, "libgui: close_container with no container open");
        self.close();
    }

    /// How many containers are open above the frame's root — zero when the
    /// tree is balanced. A binding checks this before `end_frame` to blame the
    /// caller for a missing close.
    pub fn open_depth(&self) -> usize {
        self.stack.len().saturating_sub(1)
    }

    /// Replay a subtree's pixels instead of producing them again, while
    /// nothing that could change them has changed.
    ///
    /// This is for the case [`Ui::needs_frame`] cannot help with: *part* of
    /// the window is live — a meter, a clock, a playhead — so a frame has to
    /// run, and the rest of the UI repaints for nothing. Profiling says paint
    /// is ninety per cent of a frame's `end_frame` time, so the rest of the UI
    /// is most of the bill.
    ///
    /// `deps` is everything the subtree draws from. Get it wrong and you will
    /// see stale pixels, so it is the one thing the library cannot check for
    /// you:
    ///
    /// ```ignore
    /// ui.cached("outliner", (objects.len(), revision, selected), |ui| {
    ///     for (i, o) in objects.iter().enumerate() {
    ///         ui.with_key(o.id, |ui| { let _ = ui.selectable(&o.name, i == selected); });
    ///     }
    /// });
    /// ```
    ///
    /// Everything else is checked. A replay happens only when the pointer is
    /// doing the same thing over the subtree as when it was recorded, keyboard
    /// focus is outside it, nothing inside is still animating, and the theme
    /// has not changed — each of those being a way the pixels could differ
    /// that `deps` would not mention. A miss simply builds, so a subtree that
    /// never qualifies is correct and costs one hash.
    ///
    /// A pointer *resting* inside is not a reason to rebuild: the widget under
    /// it is the same widget, so the pixels are the same pixels. That matters,
    /// because a pointer resting in a panel is what someone reading one looks
    /// like. Moving it, or pressing a button, does rebuild.
    ///
    /// A subtree that merely **moved** keeps its recording: the instances are
    /// translated and clipped afresh against whatever encloses them now, since
    /// the ancestors did not move just because it did. The exception is a
    /// pointer inside it, which is then over a different widget than the one
    /// recorded.
    ///
    /// Widgets inside a replayed subtree do not run, so they cannot report
    /// anything: interaction still *works* — the hit rects are replayed too,
    /// and the pointer arriving invalidates the cache — but a `Response` from
    /// inside is only produced on a frame that built. Cache the parts of your
    /// UI that are display, not the parts you read answers from.
    pub fn cached(&mut self, key: impl Hash, deps: impl Hash, body: impl FnOnce(&mut Self)) {
        let id = self.make_id(("cached", &key));
        self.cached_seen.insert(id);
        let deps = Id::new(&deps).0;

        // A theme change repaints everything, so nothing recorded under the
        // old one is worth keeping. Compared field by field, once a frame:
        // `ui.theme` is the app's, and a hot-reloaded file or a colour picker
        // changes a palette without changing the theme's name.
        if self.theme != self.cache_theme {
            self.cache.clear();
            self.cache_theme = self.theme.clone();
        }
        let env = self.cache_env();

        let rect = self.rect_of(id).unwrap_or_default();
        let pointer = self.pointer_over(rect);
        let focus_in = self.focused.is_some_and(|f| self.rects.get(&f).is_some_and(|r| rect.contains(Vec2::new(r.x, r.y))));
        let quiet = !self.cache_busy.contains(&id);

        let replay = if quiet && !focus_in { self.cache.can_replay(id, deps, env, rect, pointer) } else { None };
        if replay.is_some() {
            let min = self.cache.entry_min(id).unwrap_or_default();
            self.cache.mark_seen(id, &mut self.seen);
            self.cache.hits_this_frame += 1;
            let mut n = Node::new(id, Layout::leaf(Size::Fixed(min.x), Size::Fixed(min.y)));
            n.cached = true;
            self.attach(n);
            return;
        }

        // Miss: build it, and record what it produces.
        self.cache.misses_this_frame += 1;
        let settled_before = self.unsettled;
        let start = self.cache.open_ids();
        let mut n = Node::new(id, Layout::column().width(Size::Fit).height(Size::Fit));
        n.recording = true;
        let i = self.attach(n);
        self.open(i);
        body(self);
        self.close();
        self.cache.close_ids(id, start);
        self.cache.set_deps(id, deps, pointer, env);
        // A subtree that is still animating cannot be replayed next frame: its
        // pixels are going to move on their own.
        if self.unsettled != settled_before {
            self.cache_busy.insert(id);
        } else {
            self.cache_busy.remove(&id);
        }
    }

    /// Re-measure every paragraph against the width layout just gave it.
    /// True when any of them changed height, so the solve has to run again.
    fn rewrap(&mut self) -> bool {
        let mut changed = false;
        for &(node, text, size) in &self.wrapping {
            let n = &self.nodes[node as usize];
            let w = n.rect.w;
            if w <= 0.0 {
                continue;
            }
            let s = crate::PaintText::get(&text, self.strs.bytes());
            let h = self.fonts.measure_wrapped(self.font, size, s, w).y;
            if (self.nodes[node as usize].intrinsic.y - h).abs() > 0.01 {
                self.nodes[node as usize].intrinsic.y = h;
                changed = true;
            }
        }
        changed
    }

    /// The pointer as far as `rect`'s appearance is concerned: where it is if
    /// it is inside, and nothing at all if it is not.
    /// What a recording is only valid under: see `subtree_cache::Env`.
    fn cache_env(&self) -> crate::subtree_cache::Env {
        crate::subtree_cache::Env {
            scale: self.input.scale,
            xform: self.xform(),
            atlas_repacks: self.fonts.atlas().repacks,
        }
    }

    fn pointer_over(&self, rect: Rect) -> crate::subtree_cache::Pointer {
        let inside = self.input.mouse_inside && rect.contains(self.input.mouse_pos);
        crate::subtree_cache::Pointer(inside.then_some((self.input.mouse_pos, self.input.buttons_down)))
    }

    /// A container whose children are laid out shifted by `offset`, and
    /// clipped to it — a scroll area's displacement without a scroll area's
    /// state, input or scrollbars.
    ///
    /// The point is *shared* displacement. A table's header, its frozen
    /// columns and every one of its rows have to agree on one horizontal
    /// offset to the exact pixel, and giving each of them a scroll area of its
    /// own would mean keeping N of them in sync. Instead the caller owns the
    /// number and hands it to each pane.
    ///
    /// `snap_text` follows the same rule as [`Ui::scroll_area`]: false while
    /// the offset is moving, so the text inside tracks it sub-pixel instead of
    /// shearing against the boxes it sits in.
    pub(crate) fn offset_container<R>(
        &mut self,
        id: Id,
        offset: Vec2,
        snap_text: bool,
        layout: Layout,
        frame: Frame,
        body: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.mark_seen(id);
        let mut n = Node::new(id, layout);
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 {
            n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        // Neither axis scrolls, so `place` applies the offset without growing
        // the content box and `paint` draws no bars.
        n.scroll = Some(Scroll {
            offset,
            scroll_x: false,
            scroll_y: false,
            bar_x: id.with("nobar_x"),
            bar_y: id.with("nobar_y"),
            vis_x: 0.0,
            hot_x: 0.0,
            vis_y: 0.0,
            hot_y: 0.0,
            style: self.theme.scrollbar,
            snap: snap_text,
        });
        let i = self.attach(n);
        self.open(i);
        let r = body(self);
        self.close();
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

    /// [`Ui::layer`] that sizes itself to its content: `place` is handed the
    /// fitted size and returns where to put it. Returns the rect it used.
    ///
    /// The layer's rect is also what positions it, so the size can only come
    /// from the previous frame's measure: on the frame it first appears it is
    /// zero-sized and invisible, and it settles on the next — the same
    /// one-frame rule as [`Response::rect`]. Used by [`Ui::drag_ghost`].
    pub fn layer_fit_in<R>(
        &mut self,
        id: Id,
        z: Layer,
        place: impl FnOnce(Vec2) -> Rect,
        frame: Frame,
        body: impl FnOnce(&mut Self) -> R,
    ) -> (Rect, R) {
        let fitted = match self.layer_min.get(&id).copied() {
            Some(v) => v,
            None => {
                // Provisional, as in `popup`: measured next frame, so ask for one.
                self.animating = true;
                Vec2::ZERO
            }
        };
        let rect = place(fitted);
        let layout = Layout::column().width(Size::Fit).height(Size::Fit);
        (rect, self.layer_with(id, z, rect, layout, frame, body))
    }

    /// [`Ui::layer`] in an explicit stacking [`Layer`].
    pub fn layer_in<R>(&mut self, id: Id, z: Layer, rect: Rect, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.layer_with(id, z, rect, Layout::column().shrink(), frame, body)
    }

    /// [`Ui::layer_in`] without a closure, for a binding that cannot hold one.
    /// Close it with [`Ui::close_layer`].
    ///
    /// This is the building block for a modal: a layer over the window at
    /// [`Layer::Popup`] or above, with a scrim drawn under it. libgui has no
    /// modal of its own — what a modal *blocks* is an app's question, not a
    /// layout one.
    pub fn open_layer(&mut self, id: Id, z: Layer, rect: Rect, frame: Frame) {
        self.open_layer_with(id, z, rect, Layout::column().shrink(), frame);
    }

    pub fn close_layer(&mut self) {
        self.close();
    }

    fn layer_with<R>(&mut self, id: Id, z: Layer, rect: Rect, layout: Layout, frame: Frame, body: impl FnOnce(&mut Self) -> R) -> R {
        self.open_layer_with(id, z, rect, layout, frame);
        let r = body(self);
        self.close();
        r
    }

    /// The opening half of [`Ui::layer_with`], so the closure form and the
    /// open/close pair cannot drift apart.
    fn open_layer_with(&mut self, id: Id, z: Layer, rect: Rect, layout: Layout, frame: Frame) {
        self.mark_seen(id);
        let mut n = Node::new(id, layout);
        n.absolute = Some(rect);
        n.z = z;
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 || frame.shadow {
            n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
                if frame.shadow {
                    p.shadow(r.translate(0.0, 8.0), frame.radius, 28.0, p.theme.palette.shadow);
                }
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        // Layers hang off the root so they sit above all flow content.
        let idx = self.nodes.len();
        self.nodes.push(n);
        self.root_kids.push(idx as u32);
        self.open(idx);
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
        self.mark_seen(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.absolute = Some(rect);
        n.z = z;
        n.clip = frame.clip;
        if frame.fill.a > 0.0 || frame.border_width > 0.0 || frame.shadow {
            n.paint = Some(self.paints.push(move |p: &mut Painter, r: Rect| {
                if frame.shadow {
                    p.shadow(r.translate(0.0, 4.0), frame.radius, 16.0, p.theme.palette.shadow);
                }
                p.rect_bordered(r, frame.fill, frame.radius, frame.border_width, frame.border);
            }));
        }
        let i = self.attach(n);
        self.open(i);
        let r = body(self);
        self.close();
        r
    }

    /// Leaf at an absolute rect inside the current container (resize grips, badges).
    pub fn add_leaf_at(&mut self, id: Id, rect: Rect, opts: LeafOptions, paint: impl FnOnce(&mut Painter, Rect) + 'static) {
        self.mark_seen(id);
        let mut n = Node::new(id, Layout::leaf(Size::Fixed(rect.w), Size::Fixed(rect.h)));
        n.absolute = Some(rect);
        n.interactive = opts.interactive;
        n.hit_pad = opts.hit_pad;
        n.hit_top = opts.hit_top;
        n.paint = Some(self.paints.push(paint));
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
        self.open_scroll_area_id(id, opts);
        let r = body(self);
        self.close_container();
        r
    }

    /// Open a scroll area without a closure, for a binding that cannot hold
    /// one. Close it with [`Ui::close_scroll_area`]; the same rules as
    /// [`Ui::open_container`] apply.
    /// Give a list, tree or table's rows a **keyboard cursor**, and make the
    /// whole thing one focus stop instead of one per row.
    ///
    /// Tab moves focus *between* widgets. This is the other half, which libgui
    /// did not have: moving *within* one. A hierarchy panel you cannot arrow
    /// through is the first thing a keyboard user — and any user of a
    /// professional tool — reaches for and does not find.
    ///
    /// The cursor is an index, kept per collection id and swept when the
    /// collection stops being built. The rows are built between this call and
    /// [`Ui::close_collection`]; each row compares its own index against
    /// `cursor` to draw itself as the current one.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, names: &[String], selected: &mut usize) {
    /// let nav = ui.open_collection("hierarchy", names.len());
    /// if nav.moved {
    ///     *selected = nav.cursor; // this list follows the cursor; a
    /// }                           // multi-select one would not
    /// for (i, name) in names.iter().enumerate() {
    ///     if ui.selectable_keyed(i, name, *selected == i).clicked {
    ///         *selected = i;
    ///         ui.set_cursor(nav.id, i); // clicking moves the cursor too
    ///     }
    /// }
    /// ui.close_collection();
    /// # }
    /// ```
    ///
    /// Which key moves the cursor is not libgui's business — the arrows are
    /// bound to [`UiAction::Navigate`] by the keymap, and a host with a
    /// gamepad or a jog wheel sends the action directly.
    pub fn open_collection(&mut self, key: &str, len: usize) -> NavResponse {
        self.open_collection_with(key, NavOptions { len, ..Default::default() })
    }

    /// [`Ui::open_collection`] with a page size and wrapping. See
    /// [`NavOptions`].
    pub fn open_collection_with(&mut self, key: &str, opts: NavOptions) -> NavResponse {
        let id = self.make_id(("collection", key));
        self.keep_id(id);
        // Lend the ring to the enclosing container, which is the rect a person
        // would point at and call "the list".
        let container = self.nodes[self.stack.last().expect("libgui: collection built outside a frame").0].id;
        self.nav_ring.insert(id, container);
        // The focus stop is resolved *before* the collection opens, or it
        // would swallow its own focusability along with its rows'.
        let k = self.focusable(id, crate::FocusKind::Collection);
        self.nav_open.push(id);

        let len = opts.len;
        // An empty collection still has a cursor, so adding the first row does
        // not move it from somewhere surprising.
        let mut cursor = self.nav_states.get(&id).copied().unwrap_or(0);
        cursor = cursor.min(len.saturating_sub(1));
        let before = cursor;
        let mut expand = false;
        let mut collapse = false;

        if k.focused && len > 0 {
            let page = opts.page.max(1);
            // Every pending Navigate is taken, not just the first: a key held
            // down delivers several in a frame, and dropping them makes a long
            // list feel like it is fighting the hand holding the key.
            while let Some(nav) = self.take_nav() {
                let step = |c: usize, d: isize| -> usize {
                    let n = len as isize;
                    let t = c as isize + d;
                    if opts.wrap {
                        t.rem_euclid(n) as usize
                    } else {
                        t.clamp(0, n - 1) as usize
                    }
                };
                match nav {
                    crate::Nav::Next => cursor = step(cursor, 1),
                    crate::Nav::Previous => cursor = step(cursor, -1),
                    crate::Nav::First => cursor = 0,
                    crate::Nav::Last => cursor = len - 1,
                    // A page never wraps, whatever `wrap` says: landing at the
                    // far end of a list because a page overshot by two is not
                    // what the key means.
                    crate::Nav::PageNext => cursor = (cursor + page).min(len - 1),
                    crate::Nav::PagePrevious => cursor = cursor.saturating_sub(page),
                    crate::Nav::Expand => expand = true,
                    crate::Nav::Collapse => collapse = true,
                }
            }
        }
        self.nav_states.insert(id, cursor);
        NavResponse {
            id,
            cursor,
            moved: cursor != before,
            focused: k.focused,
            activated: k.activated,
            expand,
            collapse,
        }
    }

    pub fn close_collection(&mut self) {
        self.nav_open.pop();
    }

    /// [`Ui::open_collection`] with a closure, for Rust callers. The
    /// open/close pair exists because C has no closures.
    pub fn collection<R>(&mut self, key: &str, len: usize, body: impl FnOnce(&mut Self, NavResponse) -> R) -> R {
        let nav = self.open_collection(key, len);
        let r = body(self, nav);
        self.close_collection();
        r
    }

    /// Turn a click on `index` into a change to the app's selection, keeping
    /// the anchor a range-select needs.
    ///
    /// Multi-select is three rules everyone writes the same way and nobody
    /// enjoys writing: a plain click replaces, Ctrl/Cmd toggles, Shift takes
    /// everything from the last plain click to here. The third needs an
    /// anchor that survives across frames, moves when a plain click or a
    /// toggle lands, and does *not* move while a range is being dragged out —
    /// which is the part that is fiddly, so it lives here.
    ///
    /// The selection itself stays yours: libgui never learns what a row *is*.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # use std::collections::BTreeSet;
    /// # fn f(ui: &mut Ui, bodies: &[String], picked: &mut BTreeSet<usize>, kind: SelectKind) {
    /// let nav = ui.open_collection("model", bodies.len());
    /// for (i, body) in bodies.iter().enumerate() {
    ///     if ui.selectable_keyed(i, body, picked.contains(&i)).clicked {
    ///         match ui.select(nav.id, i, kind) {
    ///             Selection::Only(i) => { picked.clear(); picked.insert(i); }
    ///             Selection::Toggle(i) => { if !picked.remove(&i) { picked.insert(i); } }
    ///             Selection::Range(r) => { picked.clear(); picked.extend(r); }
    ///         }
    ///     }
    /// }
    /// ui.close_collection();
    /// # }
    /// ```
    ///
    /// `collection` is the id from [`NavResponse`], so the anchor is swept
    /// with the rest of that collection's state. A list without a collection
    /// can pass any stable [`Id`] of its own.
    pub fn select(&mut self, collection: Id, index: usize, kind: SelectKind) -> Selection {
        match kind {
            SelectKind::Replace => {
                self.select_anchors.insert(collection, index);
                Selection::Only(index)
            }
            // A toggle moves the anchor too: the next Shift-click extends from
            // what was last touched, which is what every file manager does.
            SelectKind::Toggle => {
                self.select_anchors.insert(collection, index);
                Selection::Toggle(index)
            }
            // The anchor deliberately does *not* move: dragging a Shift-click
            // up and down a list has to grow and shrink one range rather than
            // ratchet a new one from wherever it last was.
            SelectKind::Range => {
                let from = self.select_anchors.get(&collection).copied().unwrap_or(index);
                let (lo, hi) = if from <= index { (from, index) } else { (index, from) };
                Selection::Range(lo..=hi)
            }
        }
    }

    /// Where a range-select would extend from, if anything has been clicked.
    pub fn select_anchor(&self, collection: Id) -> Option<usize> {
        self.select_anchors.get(&collection).copied()
    }

    /// Put the cursor on `index`, so clicking a row leaves the keyboard where
    /// the pointer left off rather than back where it was.
    ///
    /// Takes the id from [`NavResponse::id`] rather than the collection's key:
    /// [`Ui::make_id`] disambiguates repeated keys by build order, so deriving
    /// the id a second time would address a different collection.
    pub fn set_cursor(&mut self, collection: Id, index: usize) {
        self.nav_states.insert(collection, index);
    }

    /// Where the keyboard cursor is, by the id [`NavResponse`] carries. For
    /// reading it back outside the frame that built the collection.
    pub fn cursor_at(&self, id: Id) -> Option<usize> {
        self.nav_states.get(&id).copied()
    }

    /// Take one pending [`Nav`](crate::Nav), whichever it is. `take_action`
    /// wants the exact action, and a collection accepts any of eight.
    fn take_nav(&mut self) -> Option<crate::Nav> {
        let at = self.input.events.iter().position(|e| matches!(e, UiEvent::Action(UiAction::Navigate(_))));
        match at {
            Some(i) => match self.input.events.remove(i) {
                UiEvent::Action(UiAction::Navigate(n)) => Some(n),
                _ => None,
            },
            None => None,
        }
    }

    /// Scroll whatever area contains `id` until that widget is visible.
    ///
    /// Served at the top of that area's next frame, so it composes with this
    /// frame's wheel and drag rather than fighting them, and it eases like any
    /// other scroll. Nothing happens if the widget is already visible, if it
    /// is not inside a scroll area, or if it has not been laid out yet.
    ///
    /// Focus already does this for itself — see [`Ui::focusable`]. Call it by
    /// hand for a *cursor* the library does not own: the current row of an
    /// [`Ui::open_collection`], a search hit, a node the app selected in code.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, rows: &[String]) {
    /// let nav = ui.open_collection("hierarchy", rows.len());
    /// for (i, row) in rows.iter().enumerate() {
    ///     let r = ui.selectable_keyed(i, row, nav.cursor == i);
    ///     if nav.moved && nav.cursor == i {
    ///         ui.scroll_to(r.id); // keep the keyboard cursor on screen
    ///     }
    /// }
    /// ui.close_collection();
    /// # }
    /// ```
    /// Build widgets that cannot be used and say so by looking it.
    ///
    /// A command-enablement model — a ribbon that greys out what does not
    /// apply to the selection, a panel whose fields are dead until something
    /// is picked — wants to wrap a group rather than pass a flag to every
    /// widget in it, so this is a scope:
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, has_selection: bool) {
    /// ui.enabled(has_selection, |ui| {
    ///     if ui.button("Join").clicked { /* … */ }
    ///     if ui.button("Subtract").clicked { /* … */ }
    /// });
    /// # }
    /// ```
    ///
    /// Inside, every widget is inert — no hover, no click, no keyboard focus —
    /// and everything painted is multiplied by
    /// [`Metrics::disabled_alpha`](crate::Metrics::disabled_alpha). That last
    /// part happens in the draw list rather than in each widget, so an app's
    /// own [`Ui::add_leaf`] drawing greys out with the rest without knowing
    /// that it can.
    ///
    /// **Disabling nests one way only.** `ui.enabled(true, …)` inside a
    /// disabled scope does not re-enable: a group switched off has switched
    /// off everything in it, and a child claiming otherwise is a bug rather
    /// than an intent.
    pub fn enabled<R>(&mut self, enabled: bool, body: impl FnOnce(&mut Self) -> R) -> R {
        let was = self.open_enabled(enabled);
        let r = body(self);
        self.close_enabled(was);
        r
    }

    /// [`Ui::enabled`] without a closure, for a binding that cannot hold one.
    /// Returns what to hand back to [`Ui::close_enabled`].
    pub fn open_enabled(&mut self, enabled: bool) -> bool {
        let was = self.enabled;
        self.enabled &= enabled;
        was
    }

    pub fn close_enabled(&mut self, was: bool) {
        self.enabled = was;
    }

    /// Whether widgets built now can be used. False inside
    /// [`Ui::enabled`]`(false, …)`.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn scroll_to(&mut self, id: Id) {
        // The area this call sits inside, if any, and otherwise the one the
        // widget was last built in — which is what a caller outside the area
        // means, and what a focus change has to rely on.
        let area = self.scroll_stack.last().copied().or_else(|| self.in_scroll.get(&id).copied());
        if let Some(area) = area {
            self.scroll_requests.retain(|&(a, _)| a != area);
            self.scroll_requests.push((area, id));
        }
    }

    pub fn open_scroll_area(&mut self, key: &str) {
        let gap = self.theme.metrics.space;
        let opts = ScrollOptions { gap, ..ScrollOptions::new(Size::Grow(1.0)) };
        let id = self.make_id(("scroll", key));
        self.open_scroll_area_id(id, opts);
    }

    /// Close the innermost [`Ui::open_scroll_area`].
    pub fn close_scroll_area(&mut self) {
        self.close_container();
    }

    fn open_scroll_area_id(&mut self, id: Id, opts: ScrollOptions) {
        let bar_y = id.with("bar");
        let bar_x = id.with("bar_x");
        self.mark_seen(bar_y);
        self.mark_seen(bar_x);
        let mut st = self.scroll_states.get(&id).copied().unwrap_or_default();
        // A pending "bring this into view", served before this frame's input
        // so a wheel notch in the same frame still wins.
        if let Some(i) = self.scroll_requests.iter().position(|&(a, _)| a == id) {
            let (_, want) = self.scroll_requests.remove(i);
            if let (Some(w), Some(v)) = (self.rects.get(&want).copied(), self.rects.get(&id).copied()) {
                bring_into_view(&mut st, w, v, opts);
            }
        }
        let at_end = st.y.target >= st.y.max() - 1.0;

        let inside = self.scroll_target == Some(id);
        let dt = self.input.dt.max(1e-4);
        let touching = self.touch_scroll == Some(id);
        let cfg = opts.config.unwrap_or(self.scroll);

        // The two kinds of signal stay apart all the way down: the host said
        // which is which, `cfg` says what each one does, and nothing here
        // knows or cares what hardware is on the other end.
        let zero = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
        let (px, lines, pages) =
            if inside { (self.input.scroll_px, self.input.scroll_lines, self.input.scroll_pages) } else { zero };
        let stepped = Vec2::new(cfg.steps_to_px(lines.x, pages.x), cfg.steps_to_px(lines.y, pages.y));
        // A scroll with no sideways component still scrolls an area that only
        // goes sideways: what you expect when you scroll over a timeline.
        let sideways = opts.scroll_x && !opts.scroll_y && px.x == 0.0 && stepped.x == 0.0;
        let (px_x, stepped_x) = if sideways { (px.y, stepped.y) } else { (px.x, stepped.x) };
        let touch = touching.then_some(self.mouse_delta);

        if opts.scroll_x {
            st.x.update(px_x, stepped_x, touch.map(|d| d.x), &cfg, dt);
        }
        if opts.scroll_y {
            st.y.update(px.y, stepped.y, touch.map(|d| d.y), &cfg, dt);
        }
        if opts.stick_to_end && at_end && st.y.max() > 0.0 {
            st.y.target = f32::INFINITY;
        }

        // Dragging a thumb is a direct manipulation: it tracks the pointer,
        // whatever the config says about the wheel.
        let by = self.interact_drag(bar_y);
        let bx = self.interact_drag(bar_x);
        drag_bar(&mut st.y, &by, Axis::Y);
        drag_bar(&mut st.x, &bx, Axis::X);

        let scale = self.input.scale.max(0.01);
        st.x.settle(self.input.dt, scale);
        st.y.settle(self.input.dt, scale);

        // A scroll standing still sits on the physical pixel grid: crisp text,
        // hard box edges. While it moves, the offset keeps its sub-pixel part
        // and the text inside it stops snapping too, so the two never shear.
        //
        // Rounding a *moving* scroll is what made the start of a trackpad
        // flick stutter. The first frames of a gesture are 0.2-0.8 px each,
        // and round-to-nearest turned that smooth ramp into "nothing, nothing,
        // nothing, a whole pixel, nothing, a whole pixel". `offset == target`
        // is no use as a test — a trackpad scroll settles within the frame —
        // so the test is against the offset handed to layout last frame.
        let snap = |v: f32| (v * scale).round() / scale;
        let moving = st.x.offset != st.x.applied || st.y.offset != st.y.applied;
        if !moving {
            st.x.offset = snap(st.x.offset);
            st.y.offset = snap(st.y.offset);
            st.x.target = st.x.offset;
            st.y.target = st.y.offset;
        }
        st.x.applied = st.x.offset;
        st.y.applied = st.y.offset;

        self.animating |= st.x.offset != st.x.target || st.x.velocity != 0.0;
        self.animating |= st.y.offset != st.y.target || st.y.velocity != 0.0;
        // A moving scroll is a gesture in progress: keep the frames coming so
        // the host does not go to sleep between the pointer's own events.
        self.animating |= moving;
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
        self.scroll_stack.push(id);
        let mut n = Node::new(id, layout);
        n.clip = true;
        n.scroll = Some(Scroll {
            offset: Vec2::new(st.x.offset, st.y.offset),
            scroll_x: opts.scroll_x,
            scroll_y: opts.scroll_y,
            bar_x,
            bar_y,
            vis_x,
            hot_x,
            vis_y,
            hot_y,
            style: self.theme.scrollbar,
            snap: !moving,
        });
        let i = self.attach(n);
        self.open(i);
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
            config: opts.config,
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

        self.mark_seen(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.clip = true;
        n.xform = Some(t);
        let idx = self.attach(n);
        self.open(idx);
        self.xform_stack.push(t);
        // Background first, so everything built after it wins the pointer.
        let opts = LeafOptions { interactive: true, ..Default::default() };
        self.add_leaf_at(bg_id, visible, opts, |_, _| {});
        let r = body(self, view);
        self.xform_stack.pop();
        self.close();
        (bg, r)
    }

    /// Draw and interact with `body` under an explicit [`Transform`]. The raw
    /// primitive behind [`Ui::canvas`], for a viewport you drive yourself.
    pub fn with_transform<R>(&mut self, id: Id, t: Transform, body: impl FnOnce(&mut Self) -> R) -> R {
        self.mark_seen(id);
        let mut n = Node::new(id, Layout::column().shrink());
        n.clip = true;
        n.xform = Some(t);
        let idx = self.attach(n);
        self.open(idx);
        self.xform_stack.push(t);
        let r = body(self);
        self.xform_stack.pop();
        self.close();
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
    drop_hits: &'a mut Vec<(Id, Rect)>,
    /// Rects in paint order, kept only while a `cached` subtree is recording:
    /// `rects` is a map, and a replay has to put back exactly what it took.
    rects_order: &'a mut Vec<(Id, Rect)>,
    recording: u32,
    /// Nodes laid out and then clipped away entirely.
    offscreen: u32,
}

impl HitSink<'_> {
    fn mark(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.hits.len() as u32,
            self.top_hits.len() as u32,
            self.scroll_hits.len() as u32,
            self.drop_hits.len() as u32,
            self.rects_order.len() as u32,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    nodes: &mut [Node],
    kids: &[u32],
    i: usize,
    p: &mut Painter,
    sink: &mut HitSink,
    s: &mut crate::layout::Scratch,
    paints: &mut crate::paint_arena::PaintArena,
    cache: &mut crate::subtree_cache::Cache,
    focus_ring: Option<Id>,
) {
    let rect = nodes[i].rect;
    let id = nodes[i].id;
    if nodes[i].cached {
        // Only now is the rect this frame's, so only now can the offset from
        // the recording be known.
        let was = cache.entry_rect(id).unwrap_or(rect);
        let d = Vec2::new(rect.x - was.x, rect.y - was.y);
        // Whatever clips this subtree from outside is re-read now rather than
        // replayed: the ancestors did not move just because the subtree did.
        let outer = p.draw.clip();
        for &(tex, inst, inner) in cache.instances_of(id) {
            let mut inst = inst;
            inst.rect[0] += d.x;
            inst.rect[1] += d.y;
            let Some(c) = inner.translate(d.x, d.y).intersect(&outer) else { continue };
            // Culled again here rather than trusted from the recording, so a
            // replay's draw list is the same size as the build's would be.
            // The margin covers a shadow's blur and a border's width, the way
            // `DrawList::push` bounds a shape.
            let margin = inst.params[1] + inst.params[2] + 1.0;
            let bounds = Rect::new(inst.rect[0], inst.rect[1], inst.rect[2], inst.rect[3]).expand(margin);
            if c.intersect(&bounds).is_none() {
                continue;
            }
            inst.clip = [c.x, c.y, c.right(), c.bottom()];
            p.draw.replay(tex, inst);
        }
        for &(list, hid, r) in cache.hits_of(id) {
            use crate::subtree_cache::HitList::*;
            let Some(r) = r.translate(d.x, d.y).intersect(&outer) else { continue };
            match list {
                Normal => sink.hits.push((hid, r)),
                Top => sink.top_hits.push((hid, r)),
                Scroll => sink.scroll_hits.push((hid, r)),
                Drop => sink.drop_hits.push((hid, r)),
            }
        }
        for &(rid, r) in cache.rects_of(id) {
            sink.rects.insert(rid, r.translate(d.x, d.y));
        }
        return;
    }
    let rec = nodes[i].recording.then(|| {
        sink.recording += 1;
        p.draw.open_barrier();
        (p.draw.instance_count(), sink.mark())
    });
    sink.rects.insert(id, rect);
    if sink.recording > 0 {
        sink.rects_order.push((id, rect));
    }
    // Hit rects are compared against the pointer, so they are stored in window
    // space; `sink.rects` keeps the canvas-space rect a widget reports.
    let t = p.draw.xform();
    let win = t.rect(rect);
    let visible = p.draw.clip().intersect(&win);
    if visible.is_none() {
        sink.offscreen += 1;
    }
    if nodes[i].interactive {
        let pad = nodes[i].hit_pad * t.zoom;
        // The pad grows the widget, not the clip: `clip.expand(pad)` would let
        // a padded widget at a clip's edge be hit outside it — a table's last
        // column grip grabbed from the panel next door.
        if let Some(r) = p.draw.clip().intersect(&win.expand(pad)) {
            if nodes[i].hit_top {
                sink.top_hits.push((id, r));
            } else {
                sink.hits.push((id, r));
            }
        }
    }
    // Only a node that actually scrolls takes the wheel. An offset container
    // carries a `Scroll` to displace its children and nothing else, so it must
    // not swallow the gesture from the scroll area it sits inside.
    if let (Some(sc), Some(r)) = (nodes[i].scroll, visible) {
        if sc.scroll_x || sc.scroll_y {
            sink.scroll_hits.push((id, r));
        }
    }
    if let (true, Some(r)) = (nodes[i].drop_zone, visible) {
        sink.drop_hits.push((id, r));
    }
    let snap_text = nodes[i].scroll.map(|s| s.snap);
    if let Some(snap) = snap_text {
        p.draw.push_snap_text(snap);
    }
    // Set rather than multiply: every node in a disabled subtree carries the
    // same alpha, so nesting a disabled group inside another does not fade it
    // twice. Restored below, after this node's children.
    let saved_alpha = p.draw.alpha;
    p.draw.alpha = nodes[i].alpha;
    if let Some(f) = nodes[i].paint.take() {
        paints.run(f, p, rect);
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
    let children = nodes[i].children;
    // Flow children first, then absolute ones on top of them.
    for k in children.range() {
        let c = kids[k] as usize;
        if nodes[c].absolute.is_none() {
            paint(nodes, kids, c, p, sink, s, paints, cache, focus_ring);
        }
    }
    // Absolute children stack by layer, and by build order within a layer, so a
    // menu is above a floating panel however early the panel was built. Packed
    // as `(layer << 32) | index`, so one integer sort does both and the scratch
    // needs no access to `nodes`.
    let base = s.floating.len();
    for k in children.range() {
        let c = kids[k] as usize;
        if nodes[c].absolute.is_some() {
            s.floating.push(((nodes[c].z as u64) << 32) | c as u64);
        }
    }
    let n = s.floating.len() - base;
    if n > 1 {
        s.floating[base..].sort_unstable();
    }
    for k in 0..n {
        let c = (s.floating[base + k] & 0xffff_ffff) as usize;
        paint(nodes, kids, c, p, sink, s, paints, cache, focus_ring);
    }
    s.floating.truncate(base);
    if xform.is_some() {
        p.draw.pop_xform();
        p.fonts.set_zoom(p.draw.xform().zoom);
    }
    if focus_ring == Some(id) {
        // Here rather than in each widget: one place, correctly clipped by
        // whatever clips the widget, and it works for custom widgets too.
        let w = p.theme.metrics.focus_ring_width;
        let r = rect.expand(w * 0.5);
        p.rect_bordered(r, Color::TRANSPARENT, p.theme.metrics.radius + w, w, p.theme.palette.focus_ring);
    }
    if let Some(sc) = nodes[i].scroll {
        scrollbars(p, sink, rect, nodes[i].content, sc);
    }
    if snap_text.is_some() {
        p.draw.pop_snap_text();
    }
    if clip {
        p.draw.pop_clip();
    }
    p.draw.alpha = saved_alpha;
    if let Some((first, marks)) = rec {
        // Where this recording's copies start in the arenas — taken *here*,
        // not when the subtree opened. A `cached` subtree nested inside this
        // one closes first and appends its own copies in between, and a mark
        // from the open would swallow them: the outer would replay the inner
        // twice.
        let draw_marks = cache.open_draw();
        // Everything the subtree produced is now a contiguous run in each of
        // the sinks, because paint is depth-first and it owned all of it.
        // Both walk instance indices in order, so one cursor pairs them —
        // matched on the index rather than assumed to line up, because an
        // instance pushed outside a barrier has no entry at all.
        // Two streams in draw order — the instances that made it into this
        // frame's list, and the ones culled aside — merged back by the index
        // each has or would have had.
        let inner = p.draw.inner_log_since(first).to_vec();
        let culled = p.draw.culled_since(first).to_vec();
        let live: Vec<(u32, crate::TextureId, crate::Instance)> = p.draw.since(first).collect();
        let (mut li, mut ci, mut ii) = (0usize, 0usize, 0usize);
        while li < live.len() || ci < culled.len() {
            let take_culled = match (live.get(li), culled.get(ci)) {
                (Some(&(l, ..)), Some(&(c, ..))) => c <= l,
                (None, Some(_)) => true,
                _ => false,
            };
            if take_culled {
                let (_, tex, inst, ic) = culled[ci];
                ci += 1;
                cache.record_instance(tex, inst, ic);
            } else {
                let (idx, tex, inst) = live[li];
                li += 1;
                while ii < inner.len() && inner[ii].0 < idx {
                    ii += 1;
                }
                let ic = match inner.get(ii) {
                    Some(&(i, c)) if i == idx => c,
                    _ => Rect::UNBOUNDED,
                };
                cache.record_instance(tex, inst, ic);
            }
        }
        use crate::subtree_cache::HitList::*;
        for &(hid, r) in &sink.hits[marks.0 as usize..] {
            cache.record_hit(Normal, hid, r);
        }
        for &(hid, r) in &sink.top_hits[marks.1 as usize..] {
            cache.record_hit(Top, hid, r);
        }
        for &(hid, r) in &sink.scroll_hits[marks.2 as usize..] {
            cache.record_hit(Scroll, hid, r);
        }
        for &(hid, r) in &sink.drop_hits[marks.3 as usize..] {
            cache.record_hit(Drop, hid, r);
        }
        for &(rid, r) in &sink.rects_order[marks.4 as usize..] {
            cache.record_rect(rid, r);
        }
        sink.recording -= 1;
        p.draw.close_barrier();
        let pending = cache.pending_of(id);
        cache.close_draw(id, pending, rect, nodes[i].min, draw_marks);
    }
}

/// Drag or click a scrollbar track. Direct manipulation: the thumb tracks the
/// pointer exactly, whatever smoothing the config asks of the wheel.
/// Move `st` so that `want` (a widget's rect from the last layout) falls
/// inside `view` (the scroll area's own rect).
///
/// Both rects are in window coordinates, so the widget's position already has
/// the current offset in it: the distance it overshoots an edge by *is* the
/// amount to scroll, which is why this needs no content coordinates.
///
/// Nothing happens when the widget is already visible. A widget taller than
/// the viewport is aligned to its leading edge rather than centred, so a tall
/// row scrolled to shows its beginning.
fn bring_into_view(st: &mut ScrollState, want: Rect, view: Rect, opts: ScrollOptions) {
    /// A little air, so a row scrolled to does not sit flush against the edge
    /// and look clipped.
    const MARGIN: f32 = 4.0;

    fn axis(a: &mut ScrollAxis, lo: f32, size: f32, view_lo: f32, view_size: f32) {
        // Measured from where the target already is, not from where the offset
        // has eased to, so two requests in quick succession compose instead of
        // fighting each other.
        let pending = a.target - a.offset;
        let (lo, hi) = (lo - view_lo - MARGIN, lo + size - view_lo + MARGIN);
        let delta = if hi - lo >= view_size || lo < 0.0 {
            // Before the leading edge, or too big to fit: align its start.
            lo
        } else if hi > view_size {
            hi - view_size
        } else {
            return; // already visible
        };
        a.target = (a.offset + pending + delta).clamp(0.0, a.max());
    }
    if opts.scroll_y {
        axis(&mut st.y, want.y, want.h, view.y, view.h);
    }
    if opts.scroll_x {
        axis(&mut st.x, want.x, want.w, view.x, view.w);
    }
}

fn drag_bar(st: &mut ScrollAxis, bar: &Response, axis: Axis) {
    let max = st.max();
    let (track_len, track_start, pointer) = match axis {
        Axis::Y => (bar.rect.h, bar.rect.y, bar.mouse_pos.y),
        Axis::X => (bar.rect.w, bar.rect.x, bar.mouse_pos.x),
    };
    if max <= 0.0 || track_len <= 0.0 {
        return;
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
            st.smoothing = Smoothing::Instant;
        }
    }
    if bar.active {
        let d = match axis {
            Axis::Y => bar.drag_delta.y,
            Axis::X => bar.drag_delta.x,
        };
        st.target += d * max / travel;
        st.smoothing = Smoothing::Instant;
    }
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

    /// `hit_pad` makes a thin widget easier to grab, but it must not make it
    /// grabbable outside the clip its ancestors imposed: a table's last column
    /// grip would be draggable from the panel next door.
    #[test]
    fn a_padded_hit_area_stops_at_the_clip() {
        let mut ui = ui();
        let id = Id::new("grip");
        let frame = |ui: &mut Ui, at: Vec2| -> Response {
            ui.push(InputEvent::PointerMoved { pos: at });
            ui.begin_frame(FrameInfo { dt: 1.0, ..FrameInfo::default() });
            let mut resp = None;
            // A clipped panel 100 wide, with a padded widget at its right edge.
            let panel = Layout::row().width(Size::Fixed(100.0)).height(Size::Fixed(40.0));
            ui.container(panel, Frame { clip: true, ..Frame::none() }, |ui| {
                ui.space(94.0);
                resp = Some(ui.interact(id));
                let opts = LeafOptions { interactive: true, hit_pad: 8.0, hit_top: false };
                ui.add_leaf_at(id, Rect::new(94.0, 0.0, 6.0, 40.0), opts, |_, _| {});
            });
            let _ = ui.end_frame();
            resp.unwrap()
        };
        frame(&mut ui, Vec2::new(97.0, 20.0));

        // Just inside the panel, the pad does its job: the 6px grip is hit
        // from 4px to its left.
        assert!(frame(&mut ui, Vec2::new(90.0, 20.0)).hovered, "the pad did not widen the grip inside the clip");
        // Just outside the panel, it must not.
        assert!(!frame(&mut ui, Vec2::new(104.0, 20.0)).hovered, "the grip was hit outside its clip");
        assert!(!frame(&mut ui, Vec2::new(106.0, 20.0)).hovered, "the grip was hit outside its clip");
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

    /// The scroll area under test, built for one frame. Returns the first
    /// row's y and the topmost glyph's y, both from *this* frame's layout.
    fn scroll_frame(ui: &mut Ui, delta: f32, unit: crate::WheelUnit, scale: f32) -> (f32, f32) {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, 50.0) });
        if delta != 0.0 {
            ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, delta), unit });
        }
        ui.begin_frame(FrameInfo { dt: 1.0 / 60.0, scale, ..FrameInfo::default() });
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(100.0)), |ui| {
            for i in 0..40 {
                if i == 0 {
                    ui.add_leaf(Id::new("row0"), Layout::leaf(Size::Grow(1.0), Size::Fixed(24.0)), Vec2::ZERO, false, |_, _| {});
                } else {
                    let _ = ui.button(&format!("item {i}"));
                }
            }
        });
        let out = ui.end_frame();
        let glyph = crate::render_contract::PrimitiveKind::Glyph.code();
        let text_y = out
            .draw
            .instances
            .iter()
            .filter(|i| i.params[3] == glyph)
            .map(|i| i.rect[1])
            .fold(f32::INFINITY, f32::min);
        drop(out);
        (ui.rect_of(Id::new("row0")).unwrap_or_default().y, text_y)
    }

    /// Trackpad (pixel) deltas are already smooth, so the content follows them
    /// exactly; easing them again only made the list trail the fingers. Wheel
    /// notches (lines) still ease.
    ///
    /// A scroll that is *standing still* sits on the physical pixel grid, so
    /// text is crisp and boxes have hard edges. A scroll that is *moving* does
    /// not: the first frames of a trackpad flick are fractions of a pixel
    /// each, and rounding them away made the start of every scroll stutter.
    /// Its text stops snapping at the same time, so the two never shear.
    #[test]
    fn trackpad_scroll_is_exact_and_offsets_land_on_pixels() {
        use crate::WheelUnit::{Line, Pixel};
        let frame = |ui: &mut Ui, d: f32, u: crate::WheelUnit, s: f32| scroll_frame(ui, d, u, s).0;

        // Trackpad: each frame's delta is applied in full, the same frame.
        let mut ui = ui();
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Pixel, 1.0)).last().unwrap();
        assert_eq!(frame(&mut ui, -30.0, Pixel, 1.0), y0 - 30.0, "trackpad scroll lagged");
        assert_eq!(frame(&mut ui, -30.0, Pixel, 1.0), y0 - 60.0, "trackpad scroll lagged");
        assert_eq!(frame(&mut ui, 0.0, Pixel, 1.0), y0 - 60.0, "trackpad scroll kept moving after the fingers stopped");
        assert_eq!(frame(&mut ui, 0.0, Pixel, 1.0), y0 - 60.0);

        // The start of a flick: deltas smaller than a pixel must survive. Each
        // one used to round to nothing, then to a whole pixel at once.
        for scale in [1.0, 1.5, 2.0] {
            let mut ui = self::ui();
            let mut y = (0..3).map(|_| frame(&mut ui, 0.0, Pixel, scale)).last().unwrap();
            for d in [-0.2f32, -0.35, -0.5, -0.8, -1.1] {
                let next = frame(&mut ui, d, Pixel, scale);
                let step = y - next;
                assert!((step + d).abs() < 1e-3, "at scale {scale}, a {d} px step moved {step}");
                y = next;
            }
        }

        // Wheel notch: eases in over several frames rather than jumping.
        let mut ui = self::ui();
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Line, 1.0)).last().unwrap();
        frame(&mut ui, -1.0, Line, 1.0);
        let first_step = y0 - frame(&mut ui, 0.0, Line, 1.0);
        assert!(first_step > 0.0 && first_step < 24.0, "a wheel notch should ease, moved {first_step}");

        // At a fractional DPI scale, the offset lands back on a physical pixel
        // once the ease finishes.
        let mut ui = self::ui();
        let scale = 1.5;
        for _ in 0..3 {
            frame(&mut ui, 0.0, Line, scale);
        }
        frame(&mut ui, -1.0, Line, scale);
        for _ in 0..40 {
            frame(&mut ui, 0.0, Line, scale);
        }
        let y = frame(&mut ui, 0.0, Line, scale) * scale;
        assert!((y - y.round()).abs() < 1e-3, "settled row at {y} physical px, between pixels");

        // Text moves with its row, sub-pixel and all: a snapped baseline over
        // an unsnapped box is the shear this pairing exists to prevent. The
        // first moving frame also gives up the snap it was resting on, so the
        // comparison is between frames that are both already moving.
        let mut ui = self::ui();
        let (_, t0) = (0..3).map(|_| scroll_frame(&mut ui, 0.0, Pixel, 2.0)).last().unwrap();
        let (r1, t1) = scroll_frame(&mut ui, -0.25, Pixel, 2.0);
        let (r2, t2) = scroll_frame(&mut ui, -0.25, Pixel, 2.0);
        let (r3, t3) = scroll_frame(&mut ui, -0.25, Pixel, 2.0);
        assert!(t1 < t0 && t0 - t1 < 1.0, "a quarter-pixel scroll did not move the text: {t0} -> {t1}");
        assert!(((t1 - t2) - (r1 - r2)).abs() < 1e-3, "text and its row box moved by different amounts");
        assert!(((t2 - t3) - (r2 - r3)).abs() < 1e-3, "text and its row box moved by different amounts");
        assert!((r2 - r3 - 0.25).abs() < 1e-3, "a quarter-pixel scroll moved the row by {}", r2 - r3);
    }

    /// Everything that reads a scroll as a plain number in px — a canvas's
    /// zoom, `Response::scroll` — sees every unit, converted through the
    /// config. Splitting the units apart for the scroll areas once left this
    /// permanently zero, and nothing noticed.
    #[test]
    fn frame_scroll_totals_every_unit_in_px() {
        use crate::WheelUnit::{Line, Page, Pixel};
        let read = |ui: &mut Ui| -> (Vec2, f32) {
            ui.begin_frame(FrameInfo::default());
            let id = ui.make_id("probe");
            let r = ui.interact(id);
            ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, |_, _| {});
            let total = ui.input.scroll;
            let _ = ui.end_frame();
            (total, r.scroll.y)
        };
        let mut ui = ui();
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(100.0, 100.0) });
        read(&mut ui);

        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -7.5), unit: Pixel });
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -2.0), unit: Line });
        ui.push(InputEvent::Wheel { delta: Vec2::new(-1.0, 0.0), unit: Page });
        let (total, resp) = read(&mut ui);
        assert_eq!(total.y, -7.5 - 2.0 * 24.0, "lines missing from the px total");
        assert_eq!(total.x, -480.0, "pages missing from the px total");
        assert_eq!(resp, total.y, "Response::scroll disagrees with the frame total");

        // And it follows the config, not a constant.
        ui.scroll.line = 10.0;
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -2.0), unit: Line });
        assert_eq!(read(&mut ui).0.y, -20.0);
    }

    /// Smoothing is policy, not a guess about the hardware. The host says
    /// whether a delta is continuous or stepped; the app says what happens to
    /// each; and every combination of the two behaves the way it was asked to.
    #[test]
    fn smoothing_is_configurable_per_signal_and_per_area() {
        use crate::WheelUnit::{Line, Pixel};
        let frame = |ui: &mut Ui, d: f32, u: crate::WheelUnit, s: f32| scroll_frame(ui, d, u, s).0;

        // Continuous input, eased on request: it must *not* arrive in full.
        let mut ui = ui();
        ui.scroll.continuous = Smoothing::Eased { rate: 20.0 };
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Pixel, 1.0)).last().unwrap();
        let step = y0 - frame(&mut ui, -30.0, Pixel, 1.0);
        assert!(step > 0.0 && step < 30.0, "an eased continuous scroll jumped {step} of 30");

        // Stepped input, instant on request: a notch arrives whole.
        let mut ui = self::ui();
        ui.scroll.stepped = Smoothing::Instant;
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Line, 1.0)).last().unwrap();
        assert_eq!(y0 - frame(&mut ui, -1.0, Line, 1.0), ui.scroll.line, "an instant notch was eased");

        // A line is worth whatever the app says it is.
        let mut ui = self::ui();
        ui.scroll.line = 100.0;
        ui.scroll.stepped = Smoothing::Instant;
        let y0 = (0..3).map(|_| frame(&mut ui, 0.0, Line, 1.0)).last().unwrap();
        assert_eq!(y0 - frame(&mut ui, -1.0, Line, 1.0), 100.0);

        // An ease that a notch started keeps going on the frames after it,
        // when no input arrives at all and `continuous` alone would snap.
        let mut ui = self::ui();
        for _ in 0..3 {
            frame(&mut ui, 0.0, Line, 1.0);
        }
        let a = frame(&mut ui, -1.0, Line, 1.0);
        let b = frame(&mut ui, 0.0, Line, 1.0);
        let c = frame(&mut ui, 0.0, Line, 1.0);
        assert!(a > b && b > c, "the ease stopped as soon as the input did: {a}, {b}, {c}");
        assert!(c > -ui.scroll.line, "the ease jumped straight to the destination");

        // Per area: two lists side by side, the same notch over each, and
        // they answer differently because only one overrides the config.
        let mut ui = self::ui();
        let instant = ScrollConfig { stepped: Smoothing::Instant, ..ui.scroll };
        let build = |ui: &mut Ui, notch: f32, over: f32| -> (f32, f32) {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(20.0, over) });
            if notch != 0.0 {
                ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, notch), unit: Line });
            }
            ui.begin_frame(FrameInfo { dt: 1.0 / 60.0, ..FrameInfo::default() });
            for (n, cfg) in [("eased", None), ("instant", Some(instant))] {
                let opts = ScrollOptions { config: cfg, ..ScrollOptions::new(Size::Fixed(100.0)) };
                ui.scroll_area_with(n, opts, |ui| {
                    for i in 0..40 {
                        if i == 0 {
                            let l = Layout::leaf(Size::Grow(1.0), Size::Fixed(24.0));
                            ui.add_leaf(Id::new((n, "row")), l, Vec2::ZERO, false, |_, _| {});
                        } else {
                            let _ = ui.button(&format!("item {i}"));
                        }
                    }
                });
            }
            let _ = ui.end_frame();
            let y = |n| ui.rect_of(Id::new((n, "row"))).unwrap_or_default().y;
            (y("eased"), y("instant"))
        };
        for _ in 0..3 {
            build(&mut ui, 0.0, 50.0);
        }
        // One notch over the first area (y = 50), then one over the second
        // (y = 150). Same event, same frame budget, different config.
        let (e0, _) = build(&mut ui, 0.0, 50.0);
        let (e1, _) = build(&mut ui, -1.0, 50.0);
        let (_, i0) = build(&mut ui, 0.0, 150.0);
        let (_, i1) = build(&mut ui, -1.0, 150.0);
        let eased = e0 - e1;
        assert!(eased > 0.0 && eased < 24.0, "the default area should ease its notch, moved {eased}");
        assert_eq!(i0 - i1, 24.0, "the area overriding the config still eased its notch");
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
        let save = Shortcut::plain(Key::S).ctrl();

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

    /// Chords are physical and exact: libgui has no idea which of Ctrl and
    /// Cmd is "the" shortcut key. Cmd+S is not Ctrl+S; the keymap picks.
    #[test]
    fn chords_are_physical_and_exact() {
        let (ctrl_s, cmd_s) = (Shortcut::plain(Key::S).ctrl(), Shortcut::plain(Key::S).logo());
        let mut ui = ui();
        press(&mut ui, Key::S, &[Key::SuperLeft]);
        ui.begin_frame(FrameInfo::default());
        assert!(!ui.consume_shortcut(ctrl_s), "Cmd+S fired a Ctrl+S shortcut");
        assert!(ui.consume_shortcut(cmd_s), "Cmd+S did not fire");
        let _ = ui.end_frame();
        release(&mut ui, Key::S, &[Key::SuperLeft]);
    }

    /// While the user is typing, the keys the field handles belong to the
    /// field — but a real command shortcut must still get through.
    #[test]
    fn typing_keeps_its_keys_but_not_all_of_them() {
        let mut ui = ui();
        ui.set_key_bindings(crate::input::test_bindings());
        let mut text = String::from("hello");
        let frame = |ui: &mut Ui, text: &mut String| -> (bool, bool, bool) {
            ui.begin_frame(FrameInfo::default());
            let del = ui.consume_shortcut(Shortcut::plain(Key::Delete));
            let save = ui.consume_shortcut(Shortcut::plain(Key::S).ctrl());
            let play = ui.consume_shortcut(Shortcut::plain(Key::Space));
            ui.text_input("field", text, "");
            let _ = ui.end_frame();
            (del, save, play)
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

        // A bare key that types a character is the field's too: typing a
        // space must not also toggle playback.
        press(&mut ui, Key::Space, &[]);
        assert!(!frame(&mut ui, &mut text).2, "Space fired an app shortcut while typing");
        release(&mut ui, Key::Space, &[]);

        press(&mut ui, Key::S, &[Key::ControlLeft]);
        assert!(frame(&mut ui, &mut text).1, "Ctrl+S must still save while typing");
        release(&mut ui, Key::S, &[Key::ControlLeft]);
    }

    /// An inactive scope blocks the shortcuts inside it, and scopes nest.
    #[test]
    fn scopes_route_shortcuts_by_focus() {
        let mut ui = ui();
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
        // A long diagonal is split into strips to keep each quad tight (see
        // DrawList::line). Every strip carries the whole segment, and the
        // strips tile the longer axis: each edge bit-identical to the next
        // strip's, so no pixel is drawn twice (a translucent line would show
        // it) and none is skipped.
        let segs = seg(&out);
        assert!(segs.len() > 1, "a long diagonal should be split, got {} instance(s)", segs.len());
        for s in &segs {
            assert_eq!(*s, [10.0, 20.0, 110.0, 220.0], "a strip lost the segment's endpoints");
        }
        let rects: Vec<[f32; 4]> = out
            .draw
            .instances
            .iter()
            .filter(|i| PrimitiveKind::from_code(i.params[3]) == Some(PrimitiveKind::Line))
            .map(|i| i.rect)
            .collect();
        // dy > dx here, so the strips stack vertically. Rebuild each edge the
        // way the shader does, as centre ± half.
        for w in rects.windows(2) {
            let bottom = (w[0][1] + w[0][3] * 0.5) + w[0][3] * 0.5;
            let top = (w[1][1] + w[1][3] * 0.5) - w[1][3] * 0.5;
            assert_eq!(bottom.to_bits(), top.to_bits(), "strips {w:?} do not meet exactly");
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
