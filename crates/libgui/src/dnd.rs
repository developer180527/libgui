//! General drag and drop: any widget can be a source, any container a zone.
//!
//! The core knows nothing about the OS. An in-app drag is driven entirely by
//! the pointer events the host already feeds in; a drag that comes *from*
//! outside (an OS file drop, another application) is the host's to detect, and
//! it hands it over with [`Ui::begin_external_drag`]. Either way the routing,
//! hit-testing and payload handover below are identical.
//!
//! ```ignore
//! for (i, item) in items.iter().enumerate() {
//!     ui.with_key(item.id, |ui| {
//!         let r = ui.selectable(&item.name, selected == i);
//!         ui.drag_source_from(&r, || Payload::new("item", item.id).with_label(&item.name));
//!     });
//! }
//! // elsewhere, inside the container that should receive them:
//! if let Some(p) = ui.drop_zone(&["item"]).dropped {
//!     if let Ok(id) = p.take::<ItemId>() { /* move it */ }
//! }
//! ui.drag_ghost();
//! ```

use std::any::Any;

use crate::layout::{Insets, Layout, Size};
use crate::math::{Rect, Vec2};
use crate::ui::{Frame, Layer, Response, Ui};
use crate::{Id, Key};

/// What a drag carries: a `kind` the zones filter on, a `label` for the ghost,
/// and a value only the app understands.
pub struct Payload {
    kind: &'static str,
    label: String,
    value: Box<dyn Any>,
}

impl Payload {
    /// `kind` names the sort of thing being dragged (`"track"`, `"file"`); drop
    /// zones list the kinds they accept, and a drag a zone does not accept
    /// passes straight through it to whatever is underneath.
    pub fn new(kind: &'static str, value: impl Any) -> Self {
        Self { kind, label: String::new(), value: Box::new(value) }
    }

    /// Text for [`Ui::drag_ghost`]. Purely cosmetic.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Borrow the value, for a zone that previews a drop before taking it.
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    /// Take the value out. Returns the payload unchanged if `T` is not what it
    /// holds, so a zone accepting several kinds can try each in turn.
    pub fn take<T: Any>(self) -> Result<T, Self> {
        let Self { kind, label, value } = self;
        match value.downcast::<T>() {
            Ok(v) => Ok(*v),
            Err(value) => Err(Self { kind, label, value }),
        }
    }
}

/// An in-flight drag.
pub(crate) struct Active {
    pub payload: Payload,
    /// The widget it came from, or `None` for an external drag.
    pub source: Option<Id>,
    /// Where in the source's rect the pointer grabbed it, so the ghost keeps
    /// the same relation to the pointer as the thing it replaces.
    pub grab: Vec2,
    /// The pointer has been released (or the host dropped an external drag):
    /// zones may take the payload this frame, and the drag ends at `end_frame`.
    pub releasing: bool,
}

pub(crate) enum Dnd {
    Idle,
    /// Pressed on a source but still inside the threshold, so this is a click
    /// until proven otherwise.
    Press {
        source: Id,
        start: Vec2,
    },
    Active(Active),
}

impl Active {
    pub(crate) fn kind(&self) -> &'static str {
        self.payload.kind
    }
}

impl Dnd {
    pub(crate) fn active(&self) -> Option<&Active> {
        match self {
            Dnd::Active(a) => Some(a),
            _ => None,
        }
    }
}

/// What a [`Ui::drag_source`] did this frame.
#[derive(Clone, Copy, Debug)]
pub struct DragSource {
    /// The drag began this frame: the payload has just been built.
    pub started: bool,
    /// A drag from this widget is in flight. Usually drawn dimmed or as a gap.
    pub dragging: bool,
}

/// What a [`Ui::drop_zone`] saw this frame.
pub struct DropZone {
    /// A drag this zone accepts is over it — draw the insertion line now.
    pub hovered: bool,
    /// The zone's rect, from last frame's layout, for drawing the highlight.
    pub rect: Rect,
    /// Pointer position in the space the zone was built in (canvas
    /// coordinates inside a canvas), for working out an insertion index.
    pub pointer: Vec2,
    /// Released over this zone: the payload is yours.
    pub dropped: Option<Payload>,
}

impl Ui {
    /// Interact with `id` and make it a drag source in one call.
    ///
    /// `payload` is only called when the drag actually starts, so building it
    /// may be as expensive as cloning the dragged thing.
    pub fn drag_source(&mut self, id: Id, payload: impl FnOnce() -> Payload) -> DragSource {
        let r = self.interact_drag(id);
        self.drag_source_from(&r, payload)
    }

    /// Make a widget you have already interacted with a drag source, so a row
    /// that is both selectable and draggable resolves its input once.
    ///
    /// The response must come from [`Ui::interact_drag`] (or a widget built on
    /// it): on touch, a plain `interact` hands the finger to the surrounding
    /// scroll area as soon as it moves, which is exactly the movement a drag
    /// needs.
    pub fn drag_source_from(&mut self, r: &Response, payload: impl FnOnce() -> Payload) -> DragSource {
        if r.pressed {
            self.dnd = Dnd::Press { source: r.id, start: r.mouse_pos };
        }
        let mut started = false;
        if let Dnd::Press { source, start } = self.dnd {
            if source == r.id && r.active {
                let d = r.mouse_pos - start;
                if d.x.hypot(d.y) > self.drag_threshold {
                    let grab = start - Vec2::new(r.rect.x, r.rect.y);
                    self.dnd = Dnd::Active(Active { payload: payload(), source: Some(r.id), grab, releasing: false });
                    started = true;
                }
            }
        }
        DragSource { started, dragging: self.dnd.active().is_some_and(|a| a.source == Some(r.id)) }
    }

    /// Make the innermost open container a drop zone for these payload kinds.
    ///
    /// Zones nest: the innermost one under the pointer that accepts the drag
    /// wins, and one that does not accept it is not in the running at all.
    ///
    /// One container is one zone, so two calls in the same container are the
    /// same zone and the last one's `accepts` is the one that counts. Wrap each
    /// in its own container to have several.
    pub fn drop_zone(&mut self, accepts: &[&str]) -> DropZone {
        let i = self.stack.last().expect("libgui: drop_zone outside a container").0;
        let id = self.nodes[i].id;
        let accepted = self.dnd.active().is_some_and(|a| accepts.contains(&a.kind()));
        // Only accepting zones register, so resolution needs no kind filter and
        // a zone that rejects the drag does not shadow one beneath it.
        self.nodes[i].drop_zone = accepted;
        let hot = accepted && self.drop_hot == Some(id);
        let dropped = match &mut self.dnd {
            Dnd::Active(a) if hot && a.releasing => match std::mem::replace(&mut self.dnd, Dnd::Idle) {
                Dnd::Active(a) => Some(a.payload),
                _ => unreachable!(),
            },
            _ => None,
        };
        let rect = self.rect_of(id).unwrap_or_default();
        DropZone {
            hovered: hot && dropped.is_none(),
            rect,
            pointer: self.xform().inv_point(self.input.mouse_pos),
            dropped,
        }
    }

    /// The kind of drag in flight, if any.
    pub fn dragging(&self) -> Option<&'static str> {
        self.dnd.active().map(|a| a.payload.kind())
    }

    /// Borrow the in-flight payload: for a zone that wants to preview the drop.
    pub fn drag_payload(&self) -> Option<&Payload> {
        self.dnd.active().map(|a| &a.payload)
    }

    /// Abort any drag in flight. Nothing is dropped.
    pub fn cancel_drag(&mut self) {
        self.dnd = Dnd::Idle;
    }

    /// Hand the UI a drag that started outside it: an OS file drop crossing the
    /// window, a drag from another application. The host detects it and owns
    /// the OS side; from here on it routes like any other drag, against the
    /// pointer position the host is already sending.
    pub fn begin_external_drag(&mut self, payload: Payload) {
        self.dnd = Dnd::Active(Active { payload, source: None, grab: Vec2::ZERO, releasing: false });
    }

    /// Finish an external drag. `dropped` true offers the payload to whatever
    /// zone is under the pointer for one frame; false cancels outright (the
    /// drag left the window).
    pub fn end_external_drag(&mut self, dropped: bool) {
        match &mut self.dnd {
            Dnd::Active(a) if a.source.is_none() => {
                if dropped {
                    a.releasing = true;
                } else {
                    self.dnd = Dnd::Idle;
                }
            }
            _ => {}
        }
    }

    /// Outline a rect in the theme's drop-preview style: the "this will land
    /// here" highlight for a whole zone, as the dock draws for a panel.
    pub fn drop_highlight(&mut self, rect: Rect) {
        let s = self.theme.drop_preview;
        let id = self.make_id(("drop_highlight", rect.x as i32, rect.y as i32));
        self.add_leaf_at(id, rect, crate::LeafOptions::default(), move |p, r| {
            p.rect_bordered(r, s.fill, s.radius, s.border_width, s.border);
        });
    }

    /// Draw the default ghost — a card with the payload's label — following the
    /// pointer above everything else. Call it once per frame, at the end of the
    /// frame; for a custom ghost use [`Ui::layer_in`] with [`Layer::Drag`] and
    /// [`Ui::drag_ghost_rect`] instead.
    pub fn drag_ghost(&mut self) {
        let Some(label) = self.drag_payload().map(|p| p.label().to_string()) else { return };
        if label.is_empty() {
            return;
        }
        let id = Id::new("libgui_drag_ghost");
        let pad = self.theme.metrics.space;
        let mut frame = Frame::card(&self.theme);
        frame.fill.a *= 0.92;
        let (grab, mouse, screen) = (self.drag_grab(), self.input.mouse_pos, self.input.screen_size);
        let place = |size| ghost_rect(mouse - grab, size, screen);
        self.layer_fit_in(id, Layer::Drag, place, frame, |ui| {
            ui.container(Layout::row().padding(Insets::xy(pad, pad * 0.5)).height(Size::Fit), Frame::none(), |ui| {
                ui.label(&label);
            });
        });
    }

    /// Where in the source's rect the pointer grabbed the drag. Subtract it
    /// from the pointer to place a custom ghost the way `drag_ghost` does.
    pub fn drag_grab(&self) -> Vec2 {
        self.dnd.active().map_or(Vec2::ZERO, |a| a.grab)
    }

    /// Where a ghost of `size` should sit: at the pointer, offset so it keeps
    /// the grip the drag began with, and clamped to stay on screen.
    pub fn drag_ghost_rect(&self, size: Vec2) -> Rect {
        ghost_rect(self.input.mouse_pos - self.drag_grab(), size, self.input.screen_size)
    }

    /// Frame-start half of the drag state machine: resolve which zone the
    /// pointer is over, and turn a release into one frame of "up for grabs".
    pub(crate) fn dnd_begin_frame(&mut self) {
        self.drop_hot = if self.input.mouse_inside {
            self.drop_hits.iter().rev().find(|(_, r)| r.contains(self.input.mouse_pos)).map(|(id, _)| *id)
        } else {
            None
        };
        match &mut self.dnd {
            // The press never became a drag: it was a click.
            Dnd::Press { .. } if !self.input.mouse_down => self.dnd = Dnd::Idle,
            Dnd::Active(a) if a.source.is_some() && self.released => a.releasing = true,
            _ => {}
        }
        if self.key_pressed(Key::Escape) && self.dnd.active().is_some() {
            self.dnd = Dnd::Idle;
        }
    }

    /// Frame-end half: a payload nobody took is gone.
    pub(crate) fn dnd_end_frame(&mut self) {
        if self.dnd.active().is_some_and(|a| a.releasing) {
            self.dnd = Dnd::Idle;
        }
        // A source that is no longer built cannot finish its own drag.
        if let Dnd::Press { source, .. } = self.dnd {
            if !self.seen.contains(&source) {
                self.dnd = Dnd::Idle;
            }
        }
    }
}

/// Top-left `p`, size `size`, nudged to stay inside `screen`.
fn ghost_rect(p: Vec2, size: Vec2, screen: Vec2) -> Rect {
    let x = p.x.clamp(0.0, (screen.x - size.x).max(0.0));
    let y = p.y.clamp(0.0, (screen.y - size.y).max(0.0));
    Rect::new(x, y, size.x, size.y)
}
