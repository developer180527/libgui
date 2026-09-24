//! A text field whose value only changes when the app accepts it.
//!
//! The UI half of every field that is not free text: a dimension, an email
//! address, a colour in hex, a file name. What counts as valid is the app's —
//! its grammar, its names, its units — so libgui asks rather than decides. What
//! it owns is the behaviour around the question, which is the same everywhere:
//!
//! - The app's text is not touched while the user types. The field edits its
//!   own copy, so half an entry never reaches the model.
//! - Enter, Tab or a click elsewhere **commits**: the validator is asked, and
//!   the text is written only if it says yes.
//! - Escape puts back what was there.
//! - Refused text stays in the field, marked, with the reason beneath it. When
//!   Enter was the way out, focus stays too, with the caret where the
//!   validator said the problem is.
//! - Focus selects everything, so typing replaces the entry.
//!
//! The text the app keeps is the **source**, and an optional `display` is what
//! the field shows while nobody is editing it. A parametric CAD keeps
//! `width * 2` as the source and shows `40 mm`; editing starts from the
//! source, so the link survives.

use crate::{Response, Ui};
use std::fmt;

/// Why a validator refused the text, and optionally where.
///
/// `at` is a byte offset into the text. With one, the caret goes there when
/// Enter was refused; without one, the caret is left where it was. Not every
/// parser can say where, and none is required to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub message: String,
    pub at: Option<usize>,
}

impl FieldError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), at: None }
    }

    /// The same error, pointing at byte `at`.
    pub fn at(mut self, at: usize) -> Self {
        self.at = Some(at);
        self
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FieldError {}

/// How a [`Ui::validated_input`] field looks and behaves.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedOptions<'a> {
    /// Shown while the field is not being edited, in place of the source
    /// text: the evaluated `40 mm` beside a source of `width * 2`. `None`
    /// shows the source itself.
    pub display: Option<&'a str>,
    /// Shown while the field is empty.
    pub placeholder: &'a str,
    /// Select everything when focus arrives, so typing replaces the entry.
    /// On by default; off suits a field people append to.
    pub select_on_focus: bool,
}

impl Default for ValidatedOptions<'_> {
    fn default() -> Self {
        Self { display: None, placeholder: "", select_on_focus: true }
    }
}

/// What a [`Ui::validated_input`] did this frame.
#[derive(Clone, Debug, Default)]
pub struct ValidatedResponse {
    pub response: Response,
    /// The validator accepted new text and it was written to the caller's
    /// string this frame. The one to act on: push an undo step, re-solve.
    /// Not set when the accepted text is what was already there.
    pub committed: bool,
    /// The text in the field was edited this frame. The caller's string was
    /// not: it changes on commit.
    pub changed: bool,
    /// Escape was pressed and the edit thrown away.
    pub cancelled: bool,
    pub focused: bool,
    /// The field holds text the validator refused, and its reason. It stays
    /// until the user edits it, commits something acceptable, or presses
    /// Escape; the caller's string is untouched meanwhile.
    pub error: Option<FieldError>,
}

/// Retained per field. Not `Copy`, so it lives beside `text_states` rather
/// than in it.
#[derive(Clone, Debug, Default)]
pub(crate) struct ValidatedEdit {
    /// What the field is showing: the display or source text while inactive,
    /// the user's edit while active.
    text: String,
    /// The text is the user's — being edited, or refused — rather than the
    /// caller's.
    active: bool,
    focused: bool,
    error: Option<FieldError>,
}

impl Ui {
    /// A text field whose value changes only when `validate` accepts it.
    ///
    /// `text` is the app's source text, written only on an accepted commit.
    /// `validate` is called at most once a frame, only on commit, with the
    /// whole edit; its grammar, names and units are the app's own.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, email: &mut String) {
    /// let r = ui.validated_input("email", email, |t| {
    ///     match t.find('@') {
    ///         Some(i) if i > 0 && i + 1 < t.len() => Ok(()),
    ///         _ => Err(FieldError::new("an address needs a name and a domain")),
    ///     }
    /// });
    /// if r.committed { /* save */ }
    /// # }
    /// ```
    pub fn validated_input(
        &mut self,
        key: &str,
        text: &mut String,
        validate: impl FnOnce(&str) -> Result<(), FieldError>,
    ) -> ValidatedResponse {
        self.validated_input_with(key, text, &ValidatedOptions::default(), validate)
    }

    /// [`Ui::validated_input`] with a display string, a placeholder, and the
    /// choice of whether focus selects everything.
    pub fn validated_input_with(
        &mut self,
        key: &str,
        text: &mut String,
        opts: &ValidatedOptions,
        validate: impl FnOnce(&str) -> Result<(), FieldError>,
    ) -> ValidatedResponse {
        let id = self.make_id(("validated_input", key));
        self.mark_seen(id);
        let mut ed = self.validated_edits.remove(&id).unwrap_or_default();

        // While the text is not the user's, show the app's: its display if it
        // has one, the source otherwise. Copied only when it differs, so a
        // steady frame allocates nothing.
        if !ed.active {
            let shown = opts.display.unwrap_or(text.as_str());
            if ed.text != shown {
                ed.text.clear();
                ed.text.push_str(shown);
            }
        }

        let r = if ed.error.is_some() {
            let danger = self.theme.palette.danger;
            self.with_style(
                |t| {
                    t.text_input.border = danger;
                    t.text_input.border_hover = danger;
                    t.text_input.border_focus = danger;
                    t.text_input.focus_ring = danger.with_alpha(0.35);
                },
                |ui| ui.text_input(key, &mut ed.text, opts.placeholder),
            )
        } else {
            self.text_input(key, &mut ed.text, opts.placeholder)
        };
        let field = r.response.id;

        let mut out = ValidatedResponse {
            response: r.response,
            changed: r.changed,
            cancelled: r.cancelled,
            focused: r.focused,
            ..Default::default()
        };

        if r.focused && !ed.focused {
            // Focus arrived. Editing starts from the source, never the display:
            // `width * 2`, not the `40 mm` it evaluated to. A refused entry is
            // already the user's and is kept.
            if !ed.active {
                ed.text.clear();
                ed.text.push_str(text);
                ed.active = true;
            }
            if let Some(st) = self.text_states.get_mut(&field) {
                if opts.select_on_focus {
                    st.select_all(ed.text.len());
                } else {
                    st.place_caret(&ed.text, ed.text.len());
                }
            }
        }
        if r.changed {
            // Editing is how a user acknowledges an error.
            ed.error = None;
        }

        let mut kept_focus = false;
        if ed.focused && !r.focused {
            if r.cancelled {
                ed.active = false;
                ed.error = None;
            } else {
                // Enter, Tab, a click elsewhere: all commit.
                match validate(&ed.text) {
                    Ok(()) => {
                        if ed.text != *text {
                            text.clear();
                            text.push_str(&ed.text);
                            out.committed = true;
                        }
                        ed.active = false;
                        ed.error = None;
                    }
                    Err(e) => {
                        // Refused on Enter: the user meant to stay and finish,
                        // so they do, at the problem. Refused on a click
                        // elsewhere: they meant to leave, and focus is not
                        // pulled back from wherever they went.
                        if r.submitted {
                            self.focused = Some(field);
                            kept_focus = true;
                            if let (Some(at), Some(st)) = (e.at, self.text_states.get_mut(&field)) {
                                st.place_caret(&ed.text, at);
                            }
                        }
                        ed.error = Some(e);
                    }
                }
            }
        }
        ed.focused = r.focused || kept_focus;
        out.focused = ed.focused;

        if let Some(e) = &ed.error {
            let size = self.theme.metrics.font_size * 0.9;
            let danger = self.theme.palette.danger;
            self.text_with(&e.message, size, danger);
        }
        out.error = ed.error.clone();
        self.validated_edits.insert(id, ed);
        out
    }
}
