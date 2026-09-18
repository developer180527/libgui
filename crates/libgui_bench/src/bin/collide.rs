//! Collision check for `Id` derivation, over the id shapes libgui generates.
//!
//! An `Id` collision is not a bucket probe: two unrelated widgets would share
//! animation, focus and drag state, silently. So the hash used by `Id::with`
//! has a much higher bar than the one used for the per-frame maps.
use libgui::Id;
use std::collections::HashSet;

fn main() {
    let mut ids = HashSet::new();
    let mut expected = 0usize;
    let root = Id::new("root");
    let kinds = ["button", "label", "toggle", "slider", "selectable", "scroll", "container", "text_input"];
    for k in kinds {
        for i in 0..40_000u32 {
            // ("kind", "Object N"): the common widget shape.
            ids.insert(root.with((k, format!("Object {i}"))));
            // Sequential dedup suffixes off one base.
            ids.insert(root.with(k).with(i));
            // Dock-style numeric tuples.
            ids.insert(root.with((k, "dock_tab", i as u64, (i as u64) << 17)));
            expected += 3;
        }
    }
    let collisions = expected - ids.len();
    println!("distinct inputs {expected}, distinct ids {}, collisions {collisions}", ids.len());
    // A good 64-bit hash over 960k keys should collide ~0 times (birthday bound ~2.5e-8).
    println!("{}", if collisions == 0 { "OK" } else { "UNSUITABLE for Id derivation" });
}
