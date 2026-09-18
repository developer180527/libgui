use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Stable widget identity. Derived from the parent's id plus a label or key,
/// so the same widget gets the same id every frame. This is what links the
/// immediate-mode API to retained per-widget state (animations, drag, focus).
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
