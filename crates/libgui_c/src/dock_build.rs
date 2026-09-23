//! Node handles for building a layout from C.
//!
//! `DockNode<u64>` is a Rust value that cannot cross, so building a tree gives
//! back numbers instead. They are consumed when used — a node cannot be the
//! child of two splits — and using one twice is a refused call rather than a
//! silent duplicate.

use libgui::DockNode;
use std::cell::RefCell;

thread_local! {
    static NODES: RefCell<Vec<Option<DockNode<u64>>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn push(node: DockNode<u64>) -> u64 {
    NODES.with(|n| {
        let mut n = n.borrow_mut();
        n.push(Some(node));
        (n.len() - 1) as u64
    })
}

pub(crate) fn take(handle: u64) -> Option<DockNode<u64>> {
    NODES.with(|n| n.borrow_mut().get_mut(handle as usize).and_then(|s| s.take()))
}
