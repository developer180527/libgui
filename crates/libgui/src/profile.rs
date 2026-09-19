//! Where a frame's time went. Opt-in, off by default.
//!
//! Every other measurement in this library is a *count*, because counts are
//! identical on every machine and cannot flake on a loaded CI box. Counts are
//! the right thing to assert; they are the wrong thing to optimise against,
//! because they cannot tell you that layout is three per cent of a frame and
//! paint is sixty. So timings live here, behind a feature, and nothing in the
//! library reads the clock unless you turn it on.
//!
//! ```ignore
//! # cargo run --release -p libgui_bench --features libgui/profile -- profile
//! let out = ui.end_frame();
//! println!("{}", out.profile);
//! ```
//!
//! Timings are of whole phases, never of inner loops: a timer around something
//! called once per node would cost more than the node. To attribute inside a
//! phase, subtract two workloads that differ in one thing — which is what
//! `libgui_bench` is built to do.

use std::fmt;

/// One frame's phase timings, in milliseconds, plus the counts that explain
/// them. Zero everywhere unless the `profile` feature is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Profile {
    /// Bottom-up sizing.
    pub measure_ms: f32,
    /// Top-down placement.
    pub place_ms: f32,
    /// The paint walk: clip and transform stacks, paint closures, instance
    /// emission, hit-rect collection.
    pub paint_ms: f32,
    /// `measure + place + paint`. The rest of `end_frame` — retained-state
    /// pruning, focus, the platform output — is what is left over.
    pub end_frame_ms: f32,
    pub nodes: usize,
    pub instances: usize,
    /// Strings handed to the rasteriser this frame, cache hits included.
    pub text_draws: u32,
    /// Subtrees whose instances were replayed instead of rebuilt.
    pub cached_hits: u32,
    pub cached_misses: u32,
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let other = self.end_frame_ms - self.measure_ms - self.place_ms - self.paint_ms;
        write!(
            f,
            "end_frame {:.3}ms = measure {:.3} + place {:.3} + paint {:.3} + other {:.3}  \
             ({} nodes, {} instances, {} text, cache {}/{})",
            self.end_frame_ms,
            self.measure_ms,
            self.place_ms,
            self.paint_ms,
            other,
            self.nodes,
            self.instances,
            self.text_draws,
            self.cached_hits,
            self.cached_hits + self.cached_misses,
        )
    }
}

/// A stopwatch that compiles to nothing without the `profile` feature.
#[derive(Clone, Copy)]
pub(crate) struct Clock {
    #[cfg(feature = "profile")]
    at: std::time::Instant,
}

impl Clock {
    #[inline]
    pub fn start() -> Self {
        Self {
            #[cfg(feature = "profile")]
            at: std::time::Instant::now(),
        }
    }

    /// Milliseconds since `start`, or zero when profiling is off.
    #[inline]
    pub fn ms(self) -> f32 {
        #[cfg(feature = "profile")]
        {
            self.at.elapsed().as_secs_f32() * 1e3
        }
        #[cfg(not(feature = "profile"))]
        {
            0.0
        }
    }
}

/// Whether the `profile` feature is compiled in, so a harness can say so
/// rather than silently reporting zeroes.
pub fn enabled() -> bool {
    cfg!(feature = "profile")
}
