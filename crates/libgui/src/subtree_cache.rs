//! Replaying a subtree's pixels instead of producing them again.
//!
//! Profiling a frame (`--features profile`) says where the time is:
//!
//! ```text
//! inspector, 500 widgets:  end_frame 0.188ms
//!   measure 0.005  place 0.007  paint 0.171  other 0.004     paint = 91%
//! ```
//!
//! Layout is four per cent of a frame. **Paint is ninety.** Within paint the
//! cost tracks the instance count, and instances are mostly glyphs — 500
//! labels emit 3,870 of them. So the thing worth skipping is not layout, it is
//! the instances, and the way to skip them is to keep the ones from last time.
//!
//! [`Ui::cached`](crate::Ui::cached) does that for a subtree whose pixels
//! cannot have changed. It exists for the case frame skipping cannot help:
//! *part* of the UI is live — a meter, a clock, a playhead — so a frame has to
//! run, and the other nine tenths of the window repaint for nothing.
//!
//! # Why it is conservative
//!
//! A replay is only correct if the result would have been identical, so a hit
//! needs all of:
//!
//! - the app's `deps` hash unchanged;
//! - the same environment: DPI scale, canvas zoom and the glyph atlas's
//!   packing, none of which the app passes in `deps` and each of which
//!   rewrites the very numbers a recording holds — an instance's coordinates
//!   are window pixels and a glyph's are atlas texels;
//! - the subtree landing at exactly the rect it was recorded at — no
//!   translation, because an instance carries its clip in window coordinates
//!   and a moved subtree's clip is not the one it recorded;
//! - the pointer outside it, since hover is a visual state the app never
//!   passes in `deps`;
//! - keyboard focus outside it, for the caret;
//! - nothing inside it still animating;
//! - the same theme.
//!
//! Each of those is a way the pixels could differ, and every one of them is
//! cheaper to test than the paint it saves.

use crate::draw::Instance;
use crate::hash::{FxMap, FxSet};
use crate::math::{Rect, Vec2};
use crate::{Id, TextureId, Transform};

/// Which hit list a recorded rect belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitList {
    Normal,
    Top,
    Scroll,
    Drop,
}

/// What the build half of a recording saw, kept until paint closes it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pending {
    pub deps: u64,
    pub pointer: Pointer,
    pub env: Env,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: u32,
    end: u32,
}

impl Span {
    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// The pointer, as far as a subtree's appearance is concerned: where it is if
/// it is inside, and which buttons are down. `None` when it is outside, where
/// its exact position cannot change anything the subtree draws.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Pointer(pub Option<(Vec2, [bool; 5])>);

/// Everything outside the app's `deps` that a recording's numbers depend on.
///
/// A recording holds finished instances: positions in window pixels, glyph
/// uvs in atlas texels, colours resolved from the theme. Change any of the
/// below and those numbers mean something different, so the recording is not
/// a recording of this frame any more. (The theme belongs on this list too,
/// but it is compared whole and clears every recording, in `Ui::cached`.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Env {
    /// Physical pixels per logical pixel: text is snapped to it.
    pub scale: f32,
    /// The enclosing canvas's transform. A recording holds *window*
    /// coordinates, already through it, while the rect a replay is placed at
    /// is in the canvas's own coordinates — so a zoom rescales the recording,
    /// and a pan (or the canvas simply learning its own origin on its second
    /// frame) moves it, neither of which the replay's offset can express.
    pub xform: Transform,
    /// Bumped whenever the atlas is repacked, which moves every glyph.
    pub atlas_repacks: u64,
}

impl Default for Env {
    fn default() -> Self {
        Self { scale: 1.0, xform: Transform::IDENTITY, atlas_repacks: 0 }
    }
}

struct Entry {
    deps: u64,
    /// The environment the recording is only valid under: see [`Env`].
    env: Env,
    /// What the pointer was doing when this was recorded. A pointer *resting*
    /// inside is not a reason to rebuild — the hovered widget is the same one
    /// — and refusing to replay under a still pointer is refusing exactly when
    /// a user is reading a panel.
    pointer: Pointer,
    /// Where it was when recorded, so a replay knows how far it has moved.
    rect: Rect,

    /// Its fitted size, so layout reserves the same space without the
    /// children that decided it.
    min: Vec2,
    instances: Span,
    hits: Span,
    rects: Span,
    ids: Span,
    /// Frames since it was last used, so a subtree the app stopped building
    /// does not hold its recording forever.
    idle: u32,
}

/// Recordings of subtrees, and the arenas they live in.
#[derive(Default)]
pub(crate) struct Cache {
    entries: FxMap<Id, Entry>,
    instances: Vec<(TextureId, Instance, Rect)>,
    hits: Vec<(HitList, Id, Rect)>,
    rects: Vec<(Id, Rect)>,
    ids: Vec<Id>,
    /// Ids registered while a recording is open, so a hit can put them back in
    /// `seen` and the subtree's retained state survives not being rebuilt.
    pub(crate) recording: u32,
    pending: Vec<Id>,
    /// Id spans from the build half, waiting for paint to supply the pixels.
    pending_ids: FxMap<Id, Span>,
    /// What the build half saw, read again when paint closes the entry.
    deps: FxMap<Id, Pending>,
    pub(crate) hits_this_frame: u32,
    pub(crate) misses_this_frame: u32,
}

impl Cache {
    /// An id registered inside an open recording.
    pub fn saw(&mut self, id: Id) {
        self.pending.push(id);
    }

    pub fn entry_min(&self, id: Id) -> Option<Vec2> {
        self.entries.get(&id).map(|e| e.min)
    }

    /// Where the recording was made, so paint can work out how far the replay
    /// has moved. Build time cannot: it only has last frame's rect, and the
    /// whole point is that this frame's may differ.
    pub fn entry_rect(&self, id: Id) -> Option<Rect> {
        self.entries.get(&id).map(|e| e.rect)
    }

    /// Whether `id` can be replayed, and the offset it would land at.
    ///
    /// Size is never in question: a replayed node is `Fixed` at the size it
    /// recorded, so only its position can differ, and a difference there is
    /// something the replay can carry.
    pub fn can_replay(&self, id: Id, deps: u64, env: Env, rect: Rect, pointer: Pointer) -> Option<()> {
        let e = self.entries.get(&id)?;
        if e.deps != deps || e.pointer != pointer || e.env != env {
            return None;
        }
        // A subtree that moved under a pointer inside it has a different
        // widget under that pointer now, so its hover would be wrong. Where it
        // moved *to* is paint's to work out; all that is decided here is
        // whether a replay is allowed at all.
        let moved = rect.x != e.rect.x || rect.y != e.rect.y;
        if moved && pointer.0.is_some() {
            return None;
        }
        Some(())
    }

    /// Open the *build* half of a recording: from here until `close_ids`,
    /// every id the subtree registers is kept.
    pub fn open_ids(&mut self) -> u32 {
        self.recording += 1;
        self.ids.len() as u32
    }

    /// Close the build half. The pixels come later, in paint.
    pub fn close_ids(&mut self, id: Id, start: u32) {
        self.recording -= 1;
        self.ids.append(&mut self.pending);
        self.pending_ids.insert(id, Span { start, end: self.ids.len() as u32 });
    }

    /// What the build half hashed `deps` to.
    pub fn set_deps(&mut self, id: Id, deps: u64, pointer: Pointer, env: Env) {
        self.deps.insert(id, Pending { deps, pointer, env });
    }

    /// What the build half recorded alongside the pixels-to-come.
    pub fn pending_of(&self, id: Id) -> Pending {
        self.deps.get(&id).copied().unwrap_or_default()
    }

    /// Open the *paint* half: the arena marks to close it with.
    pub fn open_draw(&mut self) -> (u32, u32, u32) {
        (self.instances.len() as u32, self.hits.len() as u32, self.rects.len() as u32)
    }

    /// Close the paint half, and the entry with it. Without a build half the
    /// recording is incomplete and is dropped rather than half-kept.
    pub fn close_draw(&mut self, id: Id, pending: Pending, rect: Rect, min: Vec2, marks: (u32, u32, u32)) {
        let Some(ids) = self.pending_ids.remove(&id) else { return };
        let e = Entry {
            deps: pending.deps,
            env: pending.env,
            pointer: pending.pointer,
            rect,
            min,
            instances: Span { start: marks.0, end: self.instances.len() as u32 },
            hits: Span { start: marks.1, end: self.hits.len() as u32 },
            rects: Span { start: marks.2, end: self.rects.len() as u32 },
            ids,
            idle: 0,
        };
        self.entries.insert(id, e);
    }

    /// Record one instance the subtree emitted.
    /// `inner` is the clipping the subtree imposed on itself, which moves with
    /// it; whatever clipped it from outside does not, and is re-applied fresh
    /// on every replay.
    pub fn record_instance(&mut self, texture: TextureId, inst: Instance, inner: Rect) {
        self.instances.push((texture, inst, inner));
    }

    pub fn record_hit(&mut self, list: HitList, id: Id, rect: Rect) {
        self.hits.push((list, id, rect));
    }

    pub fn record_rect(&mut self, id: Id, rect: Rect) {
        self.rects.push((id, rect));
    }

    pub fn instances_of(&self, id: Id) -> &[(TextureId, Instance, Rect)] {
        match self.entries.get(&id) {
            Some(e) => &self.instances[e.instances.range()],
            None => &[],
        }
    }

    pub fn hits_of(&self, id: Id) -> &[(HitList, Id, Rect)] {
        match self.entries.get(&id) {
            Some(e) => &self.hits[e.hits.range()],
            None => &[],
        }
    }

    pub fn rects_of(&self, id: Id) -> &[(Id, Rect)] {
        match self.entries.get(&id) {
            Some(e) => &self.rects[e.rects.range()],
            None => &[],
        }
    }

    /// Put a replayed subtree's ids back in `seen`, so the animation, focus,
    /// text and scroll state belonging to widgets that did not run this frame
    /// is not pruned out from under them.
    pub fn mark_seen(&self, id: Id, seen: &mut FxSet<Id>) {
        if let Some(e) = self.entries.get(&id) {
            for &i in &self.ids[e.ids.range()] {
                seen.insert(i);
            }
        }
    }

    /// Drop recordings for subtrees the app has stopped building, and compact
    /// the arenas when enough of them have gone. Called once a frame.
    pub fn sweep(&mut self, used: &FxSet<Id>) {
        let mut stale = false;
        for (id, e) in self.entries.iter_mut() {
            e.idle = if used.contains(id) { 0 } else { e.idle + 1 };
            stale |= e.idle > 60;
        }
        if !stale {
            return;
        }
        self.entries.retain(|_, e| e.idle <= 60);
        // Arenas only ever grow, so compact by re-recording what is left. The
        // alternative — a free list — buys nothing for a few dozen panels.
        let live: Vec<(Id, Entry)> = self.entries.drain().collect();
        let oi = std::mem::take(&mut self.instances);
        let oh = std::mem::take(&mut self.hits);
        let orr = std::mem::take(&mut self.rects);
        let oid = std::mem::take(&mut self.ids);
        for (id, e) in live {
            let marks =
                (self.instances.len() as u32, self.hits.len() as u32, self.rects.len() as u32, self.ids.len() as u32);
            self.instances.extend_from_slice(&oi[e.instances.range()]);
            self.hits.extend_from_slice(&oh[e.hits.range()]);
            self.rects.extend_from_slice(&orr[e.rects.range()]);
            self.ids.extend_from_slice(&oid[e.ids.range()]);
            self.entries.insert(
                id,
                Entry {
                    instances: Span { start: marks.0, end: self.instances.len() as u32 },
                    hits: Span { start: marks.1, end: self.hits.len() as u32 },
                    rects: Span { start: marks.2, end: self.rects.len() as u32 },
                    ids: Span { start: marks.3, end: self.ids.len() as u32 },
                    ..e
                },
            );
        }
    }

    /// Forget everything. The theme changed, or the app asked.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.instances.clear();
        self.hits.clear();
        self.rects.clear();
        self.ids.clear();
        self.pending.clear();
        self.pending_ids.clear();
        self.deps.clear();
    }
}
