//! Modal dialogs: a dialog over a dimmed window that cannot be used until the
//! dialog is gone.
//!
//! libgui does the mechanics every dialog needs and every app gets subtly
//! wrong by hand:
//!
//! - **Nothing behind it can be reached.** No hover, click, wheel or drop
//!   lands on the window behind; widgets there keep their focus but take no
//!   keys; app shortcuts built outside the dialog do not fire (unless
//!   [`ModalOptions::shortcuts_behind`] says they should).
//! - **Tab stays inside it,** and on the frame it appears the keyboard moves
//!   to its first control.
//! - **Focus comes back** to where it was when the dialog goes away.
//! - **Escape, Enter and a click outside are reported,** not acted on:
//!   [`ModalResponse::cancelled`], [`submitted`](ModalResponse::submitted)
//!   and [`clicked_outside`](ModalResponse::clicked_outside).
//!
//! What it does *not* decide is what closing means. The dialog is shown on
//! every frame the app builds it and gone on the first frame it does not, so
//! "open" is the app's own `bool`, and whether Escape discards an edit, or a
//! click outside counts as cancel, is the app's to say.

use crate::input::UiAction;
use crate::{Frame, Id, Layer, Layout, Rect, Size, Ui};

/// How a [`Ui::modal`] looks and behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalOptions {
    /// The dialog's width, logical px. Its height fits its content.
    pub width: f32,
    /// Dim the window behind. Off, it is still blocked — just not shaded.
    pub dim: bool,
    /// Let app shortcuts built outside the dialog fire while it is up — a
    /// tool palette that stays usable, an app-wide Save. Off by default:
    /// a shortcut acting on a document behind an open dialog is a surprise.
    pub shortcuts_behind: bool,
}

impl Default for ModalOptions {
    fn default() -> Self {
        Self { width: 420.0, dim: true, shortcuts_behind: false }
    }
}

/// What happened to a [`Ui::modal`] this frame. Nothing here closes it: stop
/// building it, and it is gone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModalResponse<R = ()> {
    /// What the body returned.
    pub inner: R,
    /// Escape, while this is the dialog on top.
    pub cancelled: bool,
    /// Enter (or the host's Submit) that no control inside used: the
    /// dialog's default action.
    pub submitted: bool,
    /// A click on the dimmed window outside the dialog.
    pub clicked_outside: bool,
    /// This is the first frame the dialog is shown.
    pub opened: bool,
}

/// A modal being built: kept on `Ui` between `open_modal` and `close_modal`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModalBuild {
    pub id: Id,
    /// Its entry in `Ui::modals_built`.
    slot: usize,
    focus_from: usize,
    top: bool,
    cancelled: bool,
    clicked_outside: bool,
    opened: bool,
}

impl Ui {
    /// A modal dialog titled `title` (empty for none), with `body` inside it.
    ///
    /// ```ignore
    /// if self.confirm_delete {
    ///     let r = ui.modal("delete", "Delete 3 parts?", &ModalOptions::default(), |ui| {
    ///         ui.label("This cannot be undone.");
    ///         ui.row(|ui| {
    ///             ui.flex();
    ///             (ui.button("Cancel").clicked, ui.button_primary("Delete").clicked)
    ///         })
    ///     });
    ///     let (cancel, delete) = r.inner;
    ///     if delete || r.submitted { self.delete_selection(); self.confirm_delete = false; }
    ///     if cancel || r.cancelled { self.confirm_delete = false; }
    /// }
    /// ```
    ///
    /// Shown on every frame you build it; gone on the first you do not. A
    /// modal built inside another one's body (a confirmation over a settings
    /// dialog) goes on top of it and takes the keys.
    pub fn modal<R>(&mut self, key: &str, title: &str, opts: &ModalOptions, body: impl FnOnce(&mut Self) -> R) -> ModalResponse<R> {
        self.open_modal(key, title, opts);
        let inner = body(self);
        let r = self.close_modal();
        ModalResponse { inner, cancelled: r.cancelled, submitted: r.submitted, clicked_outside: r.clicked_outside, opened: r.opened }
    }

    /// [`Ui::modal`] without a closure. Build the body, then call
    /// [`Ui::close_modal`], which says what happened.
    pub fn open_modal(&mut self, key: &str, title: &str, opts: &ModalOptions) {
        let id = self.make_id(("modal", key));
        let opened = !self.modals_last.contains(&id);
        if opened {
            self.modal_return.insert(id, self.focused);
        }
        // On top: the last modal built last frame. A new one goes on top of
        // whatever was there, from its first frame.
        let top = opened || self.modals_last.last() == Some(&id);
        if top {
            self.modal_lets_shortcuts = opts.shortcuts_behind;
        }
        // Escape is the top dialog's, taken before anything inside it reads
        // it — a focused field would otherwise use it to drop its focus and
        // the dialog would never hear it. A popup of the dialog's own (a combo
        // inside it) backs out first.
        let cancelled = top && !opened && self.open_chain_is_empty() && self.take_action(UiAction::Cancel);

        let s = self.theme.modal;
        let scrim_id = id.with("scrim");
        if opened {
            self.set_anim(scrim_id, 0, 0.0);
        }
        let shade = self.animate_with_speed(scrim_id, 0, 1.0, 16.0);
        let color = if opts.dim { s.scrim.with_alpha(s.scrim.a * shade) } else { crate::Color::TRANSPARENT };
        let outside = self.modal_scrim(scrim_id, color);

        // Centred, at the size it measured last frame (the one-frame rule
        // every fitted layer has: it is invisible on its first frame and in
        // place from its second).
        let screen = self.input.screen_size;
        let fitted = match self.layer_min.get(&id).copied() {
            Some(v) => v,
            None => {
                self.request_repaint();
                crate::Vec2::ZERO
            }
        };
        let w = opts.width.min(screen.x - 32.0).max(120.0);
        let h = fitted.y.min(screen.y - 32.0);
        let rect = Rect::new(((screen.x - w) * 0.5).round(), ((screen.y - h) * 0.5).round(), w, h);
        let layout = Layout::column().width(Size::Fixed(w)).height(Size::Fit).padding(crate::Insets::all(s.padding)).gap(s.gap);
        let frame = Frame { fill: s.fill, border: s.border, border_width: 1.0, radius: s.radius, shadow: true, clip: true };
        self.open_layer_with(id, Layer::Modal, rect, layout, frame);
        if !title.is_empty() {
            let size = self.theme.metrics.font_size_heading;
            self.text_with(title, size, s.title);
        }
        let slot = self.modals_built.len();
        self.modals_built.push((id, self.focus_order.len()..self.focus_order.len()));
        self.modal_build.push(ModalBuild {
            id,
            slot,
            focus_from: self.focus_order.len(),
            top,
            cancelled,
            clicked_outside: outside.clicked,
            opened,
        });
    }

    /// Close the modal opened by [`Ui::open_modal`].
    ///
    /// # Panics
    ///
    /// When no modal is open, or a container opened inside it has not been
    /// closed.
    pub fn close_modal(&mut self) -> ModalResponse {
        let Some(m) = self.modal_build.pop() else {
            panic!("libgui: close_modal without an open modal");
        };
        // Enter that nothing inside used is the dialog's default action.
        let submitted = m.top && !m.opened && self.take_action(UiAction::Submit);
        self.close_layer_checked(m.id, "close_modal");
        self.modals_built[m.slot].1 = m.focus_from..self.focus_order.len();
        ModalResponse { inner: (), cancelled: m.cancelled, submitted, clicked_outside: m.clicked_outside, opened: m.opened }
    }

    /// True while a modal is up (as of last frame): what an app checks before
    /// acting on input it reads itself, rather than through a widget.
    pub fn any_modal_open(&self) -> bool {
        !self.modals_last.is_empty()
    }

    /// Focus at the end of a frame: give it back from dialogs that went away,
    /// and move it into one that has just appeared.
    pub(crate) fn end_modals(&mut self) {
        // Gone: built last frame, not this one.
        let gone: Vec<Id> = self.modals_last.iter().copied().filter(|id| !self.modals_built.iter().any(|(b, _)| b == id)).collect();
        for id in gone {
            if let Some(back) = self.modal_return.remove(&id) {
                // Only if focus is nowhere a widget still is: a dialog that
                // closed because something else took focus keeps that.
                let lost = self.focused.is_none_or(|f| !self.focus_order.contains(&f));
                if lost {
                    self.focused = back;
                }
            }
        }
        // Appeared: the keyboard goes to the top one's first control, unless
        // something inside it already has it.
        if let Some((id, range)) = self.modals_built.last().cloned() {
            if !self.modals_last.contains(&id) {
                let inside = self.focused.is_some_and(|f| self.focus_order[range.clone()].contains(&f));
                if !inside {
                    self.focused = self.focus_order.get(range.start).filter(|_| !range.is_empty()).copied();
                }
            }
        }
    }
}
