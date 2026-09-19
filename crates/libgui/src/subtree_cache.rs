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
use crate::{Id, TextureId};

/// Which hit list a recorded rect belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitList {
    Normal,
    Top,
    Scroll,
    Drop,
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

struct Entry {
    deps: u64,
    /// Where it was when recorded. A hit needs the same rect, to the bit.
    rect: Rect,
    /// The clip it was recorded under, for the same reason.
    clip: Rect,
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
    instances: Vec<(TextureId, Instance)>,
    hits: Vec<(HitList, Id, Rect)>,
    rects: Vec<(Id, Rect)>,
    ids: Vec<Id>,
    /// Ids registered while a recording is open, so a hit can put them back in
    /// `seen` and the subtree's retained state survives not being rebuilt.
    pub(crate) recording: u32,
    pending: Vec<Id>,
    /// Id spans from the build half, waiting for paint to supply the pixels.
    pending_ids: FxMap<Id, Span>,
    /// `deps` from the build half, read again when paint closes the entry.
    deps: FxMap<Id, u64>,
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

    /// Whether `id` can be replayed into `rect` under `clip`.
    #[allow(clippy::too_many_arguments)]
    pub fn can_replay(&self, id: Id, deps: u64, rect: Rect, clip: Rect) -> bool {
        match self.entries.get(&id) {
            Some(e) => e.deps == deps && e.rect == rect && e.clip == clip,
            None => false,
        }
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
    pub fn deps_of(&self, id: Id) -> u64 {
        self.deps.get(&id).copied().unwrap_or(0)
    }

    pub fn set_deps(&mut self, id: Id, deps: u64) {
        self.deps.insert(id, deps);
    }

    /// Open the *paint* half: the arena marks to close it with.
    pub fn open_draw(&mut self) -> (u32, u32, u32) {
        (self.instances.len() as u32, self.hits.len() as u32, self.rects.len() as u32)
    }

    /// Close the paint half, and the entry with it. Without a build half the
    /// recording is incomplete and is dropped rather than half-kept.
    #[allow(clippy::too_many_arguments)]
    pub fn close_draw(&mut self, id: Id, deps: u64, rect: Rect, clip: Rect, min: Vec2, marks: (u32, u32, u32)) {
        let Some(ids) = self.pending_ids.remove(&id) else { return };
        let e = Entry {
            deps,
            rect,
            clip,
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
    pub fn record_instance(&mut self, texture: TextureId, inst: Instance) {
        self.instances.push((texture, inst));
    }

    pub fn record_hit(&mut self, list: HitList, id: Id, rect: Rect) {
        self.hits.push((list, id, rect));
    }

    pub fn record_rect(&mut self, id: Id, rect: Rect) {
        self.rects.push((id, rect));
    }

    pub fn instances_of(&self, id: Id) -> &[(TextureId, Instance)] {
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
