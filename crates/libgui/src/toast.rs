//! Notifications that come and go: "Saved", "Export failed", "Deleted —
//! Undo".
//!
//! The app pushes them from anywhere with [`Ui::toast`] — mid-frame, from an
//! event handler, before the first frame — and draws the stack once a frame,
//! last, with [`Ui::show_toasts`]. libgui does the rest: they slide in at a
//! corner, stack, wait their turn when there are too many, pause while the
//! pointer is over them, and leave on their own.
//!
//! **A waiting notification costs nothing.** It does not keep the host drawing
//! to watch its clock: it asks to be woken when it is due
//! ([`Ui::request_repaint_in`]), and an otherwise idle window sleeps until
//! then. That relies on the host passing the real time since the last frame —
//! see [`FrameInfo::dt`](crate::FrameInfo::dt).
//!
//! What a notification says, how long it stays and whether it has an action
//! are the app's; the kind only chooses a colour from the theme.

use crate::{Align, Color, FocusKind, Frame, Id, Insets, Layer, Layout, Rect, Size, Ui, Vec2};

/// What sort of news it is. Chooses a colour from the theme's palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

/// A notification to show. Build one with [`Toast::new`] or a kind's
/// constructor, then adjust it.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    /// Seconds on screen, not counting time under the pointer. `None` stays
    /// until closed.
    pub duration: Option<f32>,
    /// A button beside the message — "Undo", "Show" — reported in
    /// [`ToastResponse::action`] when pressed.
    pub action: Option<String>,
}

impl Toast {
    /// An info notification for four seconds: long enough to read a short
    /// sentence, short enough not to pile up.
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), kind: ToastKind::Info, duration: Some(4.0), action: None }
    }
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message)
    }
    pub fn success(message: impl Into<String>) -> Self {
        Self { kind: ToastKind::Success, ..Self::new(message) }
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self { kind: ToastKind::Warning, ..Self::new(message) }
    }
    /// An error stays until it is closed: it is the one a person must not
    /// miss by looking away.
    pub fn error(message: impl Into<String>) -> Self {
        Self { kind: ToastKind::Error, duration: None, ..Self::new(message) }
    }
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = Some(seconds.max(0.0));
        self
    }
    /// Stays until closed.
    pub fn sticky(mut self) -> Self {
        self.duration = None;
        self
    }
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }
}

/// Names a notification, to dismiss it or to tell which one's action was
/// pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToastId(pub u64);

/// Which corner the stack grows from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastCorner {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

/// How [`Ui::show_toasts_with`] lays the stack out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToastOptions {
    pub corner: ToastCorner,
    /// At most this many at once; the rest wait their turn, and their clocks
    /// do not start until they are shown.
    pub max_visible: usize,
    pub width: f32,
    /// Distance from the window's edges.
    pub margin: f32,
}

impl Default for ToastOptions {
    fn default() -> Self {
        Self { corner: ToastCorner::BottomRight, max_visible: 4, width: 340.0, margin: 16.0 }
    }
}

/// What the stack did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToastResponse {
    /// This notification's action was pressed. It is dismissed too.
    pub action: Option<ToastId>,
    /// This notification was closed by hand.
    pub closed: Option<ToastId>,
}

/// One notification and its life so far.
#[derive(Clone, Debug)]
pub(crate) struct ToastEntry {
    id: ToastId,
    toast: Toast,
    /// Seconds shown, not counting time under the pointer.
    age: f32,
    /// On its way out; removed once it has slid away.
    leaving: bool,
    /// Never drawn yet: it starts from nothing and slides in.
    fresh: bool,
    /// How far in it was last frame, 0..1.
    shown: f32,
}

impl Ui {
    /// Queue a notification. Callable at any time — between frames, from an
    /// event handler — and shown by the next [`Ui::show_toasts`].
    pub fn toast(&mut self, toast: Toast) -> ToastId {
        self.next_toast += 1;
        let id = ToastId(self.next_toast);
        self.toasts.push(ToastEntry { id, toast, age: 0.0, leaving: false, fresh: true, shown: 0.0 });
        // Woken to show it, however idle the window was — also when this
        // runs between frames, where only `last_repaint` reaches the host.
        self.request_repaint();
        self.last_repaint = Some(0.0);
        id
    }

    /// Take a notification away: it slides out on the next frame. Nothing
    /// happens if it has already gone.
    pub fn dismiss_toast(&mut self, id: ToastId) {
        if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id) {
            t.leaving = true;
            self.animating = true;
            self.last_repaint = Some(0.0);
        }
    }

    /// How many notifications are showing or waiting.
    pub fn toast_count(&self) -> usize {
        self.toasts.iter().filter(|t| !t.leaving).count()
    }

    /// Draw the notifications: once a frame, **last**, so they sit above the
    /// rest of the window.
    pub fn show_toasts(&mut self) -> ToastResponse {
        self.show_toasts_with(ToastOptions::default())
    }

    /// [`Ui::show_toasts`] at a chosen corner, width and count.
    pub fn show_toasts_with(&mut self, opts: ToastOptions) -> ToastResponse {
        let mut out = ToastResponse::default();
        if self.toasts.is_empty() {
            return out;
        }
        let dt = self.elapsed;
        let speed = 14.0;
        let p = self.theme.palette;
        let size = self.theme.metrics.font_size;
        let radius = self.theme.metrics.radius_large;
        let pad = self.theme.metrics.space * 1.5;
        let gap = self.theme.metrics.space;

        // Gone: slid all the way out.
        self.toasts.retain(|t| !(t.leaving && !t.fresh && t.shown <= 0.001));

        let shown = self.toasts.len().min(opts.max_visible.max(1));
        let top = matches!(opts.corner, ToastCorner::TopRight | ToastCorner::TopLeft);
        let left = matches!(opts.corner, ToastCorner::BottomLeft | ToastCorner::TopLeft);
        let screen = self.input.screen_size;
        let margin = opts.margin;
        let place = move |sz: Vec2| {
            let x = if left { margin } else { screen.x - margin - sz.x };
            let y = if top { margin } else { screen.y - margin - sz.y };
            Rect::new(x, y, sz.x, sz.y)
        };
        let width = opts.width.min(screen.x - 2.0 * margin).max(120.0);

        // The oldest are shown first and the newest wait; of those shown, the
        // newest sits nearest the corner, where the eye already is.
        let order: Vec<usize> = if top { (0..shown).rev().collect() } else { (0..shown).collect() };
        let mut soonest: Option<f32> = None;
        let stack = Id::new("libgui_toasts");
        self.layer_fit_in(stack, Layer::Window, place, Frame::none(), |ui| {
            let col = Layout::column().width(Size::Fixed(width)).height(Size::Fit).gap(0.0);
            ui.container(col, Frame::none(), |ui| {
                for i in order {
                    let (id, kind, message, action, leaving, fresh) = {
                        let t = &ui.toasts[i];
                        (t.id, t.toast.kind, t.toast.message.clone(), t.toast.action.clone(), t.leaving, t.fresh)
                    };
                    let base = toast_id(id);
                    // Its slide lives in retained state under this id, which is
                    // swept unless something marks it seen.
                    ui.keep_id(base);
                    if fresh {
                        ui.set_anim(base, 0, 0.0);
                        ui.toasts[i].fresh = false;
                    }
                    let shown = ui.animate_with_speed(base, 0, if leaving { 0.0 } else { 1.0 }, speed);
                    ui.toasts[i].shown = shown;
                    let a = shown;
                    // Height grows and collapses with it, so the rest of the
                    // stack moves rather than jumps.
                    let full = ui.rect_of(base.with("card")).map_or(0.0, |r| r.h) + gap;
                    let wrap = Layout::column().width(Size::Grow(1.0)).height(if shown >= 1.0 { Size::Fit } else { Size::Fixed(full * ease(shown)) });
                    // Under the pointer, by where the card was last frame:
                    // the card is a container, and its buttons take the hit.
                    let hovered = ui.input.mouse_inside && ui.rect_of(base.with("card")).is_some_and(|r| r.contains(ui.input.mouse_pos));
                    ui.container_id(base.with("wrap"), wrap, Frame { clip: true, ..Frame::none() }, |ui| {
                        let accent = kind_color(&p, kind);
                        let frame = Frame {
                            fill: p.surface.with_alpha(p.surface.a * a),
                            border: p.border_strong.with_alpha(p.border_strong.a * a),
                            border_width: 1.0,
                            radius,
                            shadow: a > 0.5,
                            clip: true,
                        };
                        let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fit).padding(Insets::xy(pad, pad * 0.7)).gap(gap).align(Align::Start, Align::Center);
                        ui.container_id(base.with("card"), row, frame, |ui| {
                            // The kind, as a bar on the leading edge.
                            let bar = ui.make_id((base, "bar"));
                            ui.add_leaf(bar, Layout::leaf(Size::Fixed(3.0), Size::Fixed(size * 1.6)), Vec2::ZERO, false, move |pt, r| {
                                pt.rect(r, accent.with_alpha(accent.a * a), 1.5);
                            });
                            ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Fit), Frame::none(), |ui| {
                                ui.paragraph_with(&message, size, p.text.with_alpha(p.text.a * a), Align::Start);
                            });
                            if let Some(label) = &action {
                                if link(ui, base.with("action"), label, p.accent.with_alpha(p.accent.a * a), size) {
                                    out.action = Some(id);
                                    ui.toasts[i].leaving = true;
                                }
                            }
                            if close(ui, base.with("close"), p.text_muted.with_alpha(p.text_muted.a * a), size) {
                                out.closed = Some(id);
                                ui.toasts[i].leaving = true;
                            }
                        });
                        // The gap belongs to the card, so it collapses with it.
                        ui.space(gap);
                    });

                    // The clock: only while fully in, and not under the pointer.
                    let t = &mut ui.toasts[i];
                    if !t.leaving && shown >= 1.0 && !hovered {
                        t.age += dt;
                    }
                    if let (false, Some(d)) = (t.leaving, t.toast.duration) {
                        // A whisker of slack: a host that slept exactly the time asked for
                        // must find it due, whatever the float rounding.
                        if t.age >= d - 1e-3 {
                            t.leaving = true;
                            ui.animating = true;
                        } else if !hovered {
                            let left = d - t.age;
                            soonest = Some(soonest.map_or(left, |s: f32| s.min(left)));
                        }
                    }
                }
            });
        });
        // Asleep until the next one is due.
        if let Some(s) = soonest {
            self.request_repaint_in(s);
        }
        out
    }
}

fn toast_id(id: ToastId) -> Id {
    Id::new(("libgui_toast", id.0))
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn kind_color(p: &crate::Palette, kind: ToastKind) -> Color {
    match kind {
        ToastKind::Info => p.accent,
        ToastKind::Success => p.success,
        ToastKind::Warning => p.warning,
        ToastKind::Error => p.danger,
    }
}

/// A text button, focusable, for the toast's action.
fn link(ui: &mut Ui, id: Id, label: &str, color: Color, size: f32) -> bool {
    let r = ui.interact_focusable(id, FocusKind::Control);
    let hot = ui.animate_bool(id, 0, r.hovered || r.focused);
    let m = ui.fonts.measure(ui.font, size, label);
    let text = ui.frame_text(label);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(m.x + 8.0), Size::Fixed(m.y + 6.0)), Vec2::ZERO, true, move |p, rect| {
        p.rect(rect, color.with_alpha(color.a * 0.12 * hot), 4.0);
        p.text_centered(rect, size, color, text);
    });
    if r.hovered {
        ui.cursor = crate::Cursor::Pointer;
    }
    r.clicked
}

/// The close cross.
fn close(ui: &mut Ui, id: Id, color: Color, size: f32) -> bool {
    let r = ui.interact_focusable(id, FocusKind::Control);
    let hot = ui.animate_bool(id, 0, r.hovered || r.focused);
    let s = (size * 1.4).round();
    ui.add_leaf(id, Layout::leaf(Size::Fixed(s), Size::Fixed(s)), Vec2::ZERO, true, move |p, rect| {
        p.rect(rect, color.with_alpha(color.a * 0.15 * hot), 4.0);
        let c = rect.center();
        let k = s * 0.2;
        let w = 1.5;
        p.line(Vec2::new(c.x - k, c.y - k), Vec2::new(c.x + k, c.y + k), w, color);
        p.line(Vec2::new(c.x + k, c.y - k), Vec2::new(c.x - k, c.y + k), w, color);
    });
    if r.hovered {
        ui.cursor = crate::Cursor::Pointer;
    }
    r.clicked
}
