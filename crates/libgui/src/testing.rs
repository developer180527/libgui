//! Guards for *your* UI code, not just for libgui's.
//!
//! libgui's own performance is held in place by tests that assert counts
//! rather than times (`crates/libgui/tests/perf.rs`). Counts do not flake on a
//! loaded CI box, they do not change between a laptop and a workstation, and
//! they fail on the commit that caused them rather than on the one that
//! noticed. This module is that same machinery, made public, so a panel you
//! write can be held to the same standard:
//!
//! ```ignore
//! #[test]
//! fn the_inspector_stays_cheap() {
//!     let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
//!     let cost = testing::steady_frame(&mut ui, FrameInfo::default(), |ui| {
//!         my_app::inspector(ui, &mut state);
//!     });
//!     Budget::new().nodes(400).instances(600).assert(&cost);
//! }
//! ```
//!
//! The failure this catches is the ordinary one: somebody adds a `format!` in
//! a loop, or drops the `virtual_list` for a plain `for`, and the UI is still
//! perfectly correct — it is just now doing ten thousand times the work, and
//! nothing says so until a user with a real project complains.
//!
//! What this cannot see is what your code does before it calls a widget. For
//! that, install a counting allocator in the test binary and assert on it too;
//! `crates/libgui/tests/perf_alloc.rs` is a worked example.

use crate::{FrameInfo, Ui};

/// What one frame cost, in counts that are identical on every machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameCost {
    /// Layout nodes built. The closest thing to "how much UI did you describe",
    /// and the number a missing virtualisation blows up.
    pub nodes: usize,
    /// Draw instances emitted. Work the GPU is asked to do.
    pub instances: usize,
    /// Draw batches: one per contiguous run sharing a texture.
    pub batches: usize,
    /// Glyphs rasterised into the atlas. A steady frame rasterises **none** —
    /// anything else means text is churning, usually a string rebuilt with
    /// different content every frame.
    pub glyphs_rasterized: u32,
    /// Strings shaped this frame: a run-cache miss each. A steady frame
    /// shapes **none**. Anything else means text is being rebuilt with
    /// different content every frame — a frame counter, a timestamp, a float
    /// printed at full precision — and every one of those costs a shaping
    /// pass and a slot in a cache that clears wholesale when it fills.
    pub text_shaped: u32,
    /// Nodes built entirely outside their clip: laid out, then thrown away.
    /// A handful is normal at the edges of a scroll area; thousands means the
    /// list should be virtualised.
    pub offscreen_nodes: u32,
    /// Interactive widgets whose id came from *build order* because they
    /// collided with a sibling — a missing [`Ui::with_key`]. Their focus,
    /// animation and drag state move to the neighbour when the list reorders.
    /// Only counted while [`Ui::audit`] is on, which [`steady_frame`] turns on.
    pub unkeyed_duplicates: u32,
}

/// Upper bounds on a [`FrameCost`]. Unset fields are not checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct Budget {
    nodes: Option<usize>,
    instances: Option<usize>,
    batches: Option<usize>,
    glyphs_rasterized: Option<u32>,
    text_shaped: Option<u32>,
    offscreen_nodes: Option<u32>,
    unkeyed_duplicates: Option<u32>,
}

impl Budget {
    /// A budget that checks nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The budget a steady panel should be able to keep: no glyph
    /// rasterisation, no unkeyed duplicates, and `nodes` layout nodes.
    pub fn steady(nodes: usize) -> Self {
        Self::new().nodes(nodes).glyphs_rasterized(0).text_shaped(0).unkeyed_duplicates(0)
    }

    pub fn nodes(mut self, max: usize) -> Self {
        self.nodes = Some(max);
        self
    }

    pub fn instances(mut self, max: usize) -> Self {
        self.instances = Some(max);
        self
    }

    pub fn batches(mut self, max: usize) -> Self {
        self.batches = Some(max);
        self
    }

    pub fn glyphs_rasterized(mut self, max: u32) -> Self {
        self.glyphs_rasterized = Some(max);
        self
    }

    pub fn text_shaped(mut self, max: u32) -> Self {
        self.text_shaped = Some(max);
        self
    }

    pub fn offscreen_nodes(mut self, max: u32) -> Self {
        self.offscreen_nodes = Some(max);
        self
    }

    pub fn unkeyed_duplicates(mut self, max: u32) -> Self {
        self.unkeyed_duplicates = Some(max);
        self
    }

    /// Every overrun, worst first, or empty if the frame is within budget.
    pub fn check(&self, cost: &FrameCost) -> Vec<String> {
        let mut over: Vec<(f64, String)> = Vec::new();
        let mut cmp = |name: &str, got: u64, max: Option<u64>, hint: &str| {
            if let Some(max) = max {
                if got > max {
                    let ratio = if max == 0 { f64::INFINITY } else { got as f64 / max as f64 };
                    over.push((ratio, format!("{name}: {got}, budget {max}{hint}")));
                }
            }
        };
        cmp("nodes", self.nodes.map_or(0, |_| cost.nodes as u64), self.nodes.map(|v| v as u64), "");
        cmp("instances", cost.instances as u64, self.instances.map(|v| v as u64), "");
        cmp("batches", cost.batches as u64, self.batches.map(|v| v as u64), "");
        cmp(
            "glyphs_rasterized",
            cost.glyphs_rasterized as u64,
            self.glyphs_rasterized.map(u64::from),
            " — text is changing every frame, or the atlas is thrashing",
        );
        cmp(
            "text_shaped",
            cost.text_shaped as u64,
            self.text_shaped.map(u64::from),
            " — a string is being rebuilt with different content every frame",
        );
        cmp(
            "offscreen_nodes",
            cost.offscreen_nodes as u64,
            self.offscreen_nodes.map(u64::from),
            " — build only what is visible (`virtual_list` / `virtual_rows`)",
        );
        cmp(
            "unkeyed_duplicates",
            cost.unkeyed_duplicates as u64,
            self.unkeyed_duplicates.map(u64::from),
            " — wrap each item in `ui.with_key(item_id, ..)`",
        );
        over.sort_by(|a, b| b.0.total_cmp(&a.0));
        over.into_iter().map(|(_, m)| m).collect()
    }

    /// [`Budget::check`], panicking with every overrun listed.
    #[track_caller]
    pub fn assert(&self, cost: &FrameCost) {
        let over = self.check(cost);
        assert!(over.is_empty(), "frame over budget:\n  {}", over.join("\n  "));
    }
}

/// Build `body` until the frame repeats, and return what that steady frame
/// cost.
///
/// The first frames of any UI are not representative: glyphs are rasterised
/// once, layout needs a frame to settle, and every hover animation is at its
/// starting value. Asserting on frame one measures start-up, not steady state.
/// This runs four frames and reports the last, with [`Ui::audit`] on.
pub fn steady_frame(ui: &mut Ui, info: FrameInfo, mut body: impl FnMut(&mut Ui)) -> FrameCost {
    let audit = ui.audit;
    ui.audit = true;
    for _ in 0..4 {
        ui.begin_frame(info);
        body(ui);
        let _ = ui.end_frame();
    }
    ui.audit = audit;
    ui.frame_cost()
}
