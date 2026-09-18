use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Stable widget identity. Derived from the parent's id plus a label or key,
/// so the same widget gets the same id every frame. This is what links the
/// immediate-mode API to retained per-widget state (animations, drag, focus).
///
/// Deliberately keeps a strong hash while the per-frame maps keyed *by* `Id`
/// use [`crate::hash::FxHasher`]. A collision here is not a bucket probe: two
/// unrelated widgets would silently share animation, focus and drag state.
/// FxHash measured 1776 collisions over the 960k ids in `libgui_bench --bin
/// collide`, where a 64-bit hash should produce none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Id(pub u64);

impl Id {
    pub fn new(src: impl Hash) -> Id {
        Id(0).with(src)
    }

    pub fn with(self, src: impl Hash) -> Id {
        let mut h = DefaultHasher::new();
        self.0.hash(&mut h);
        src.hash(&mut h);
        Id(h.finish())
    }
}
