//! The virtualised tree from C: a table of callbacks for the app's tree, and
//! an opaque handle for what the tree keeps between frames.

use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::LibguiModifiers;
use libgui::{SelectKind, TreeSelection, TreeSource, TreeState};
use std::os::raw::c_char;

/// Asked for the roots: `node` is this.
pub const LIBGUI_TREE_ROOT: u64 = u64::MAX;

/// The app's tree, as callbacks over `u64` keys. Called while libgui is
/// building, so **none may call libgui** with the same handle — such calls
/// are refused.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LibguiTreeSource {
    /// Write `node`'s children (the roots when `node` is
    /// `LIBGUI_TREE_ROOT`) into `out`, at most `cap` of them, and return how
    /// many there are. If that is more than `cap`, libgui asks again with
    /// room. Only asked of expanded nodes.
    pub children: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, node: u64, out: *mut u64, cap: u64) -> u64>,
    /// Whether `node` gets a disclosure arrow. Asked of every visible row.
    pub has_children: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, node: u64) -> u8>,
    /// What the row shows. Valid until the next call. NULL shows nothing.
    pub label: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, node: u64) -> *const c_char>,
    /// Whether the row is drawn selected. May be NULL: nothing is.
    pub selected: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, node: u64) -> u8>,
    pub user: *mut std::ffi::c_void,
}

/// The Rust side of the table.
struct Source {
    t: LibguiTreeSource,
}

impl Source {
    fn list(&self, node: u64, out: &mut Vec<u64>) {
        let Some(f) = self.t.children else { return };
        let start = out.len();
        let mut cap = 64u64;
        loop {
            out.resize(start + cap as usize, 0);
            let n = unsafe { f(self.t.user, node, out[start..].as_mut_ptr(), cap) };
            if n <= cap {
                out.truncate(start + n as usize);
                return;
            }
            cap = n;
        }
    }
}

impl TreeSource for Source {
    type Key = u64;
    fn roots(&self, out: &mut Vec<u64>) {
        self.list(LIBGUI_TREE_ROOT, out);
    }
    fn children(&self, node: u64, out: &mut Vec<u64>) {
        self.list(node, out);
    }
    fn has_children(&self, node: u64) -> bool {
        self.t.has_children.is_some_and(|f| unsafe { f(self.t.user, node) } != 0)
    }
    fn label(&self, node: u64) -> std::borrow::Cow<'_, str> {
        // Copied: the C string is only good until the next call.
        let Some(f) = self.t.label else { return "".into() };
        let p = unsafe { f(self.t.user, node) };
        if p.is_null() {
            return "".into();
        }
        unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned().into()
    }
    fn selected(&self, node: u64) -> bool {
        self.t.selected.is_some_and(|f| unsafe { f(self.t.user, node) } != 0)
    }
}

/// What a tree keeps between frames: which nodes are open and the visible
/// rows. Opaque; one per tree, kept for as long as it is shown.
pub struct LibguiTree(TreeState<u64>);

#[no_mangle]
pub extern "C" fn libgui_tree_new() -> *mut LibguiTree {
    Box::into_raw(Box::new(LibguiTree(TreeState::new())))
}

/// # Safety
/// `tree` null or from `libgui_tree_new`, not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_free(tree: *mut LibguiTree) {
    if !tree.is_null() {
        drop(unsafe { Box::from_raw(tree) });
    }
}

macro_rules! tree_op {
    ($(#[$m:meta])* $name:ident($($a:ident: $t:ty),*) $body:expr) => {
        $(#[$m])*
        ///
        /// # Safety
        /// `tree` null or from `libgui_tree_new`.
        #[no_mangle]
        pub unsafe extern "C" fn $name(tree: *mut LibguiTree $(, $a: $t)*) {
            if let Some(t) = unsafe { tree.as_mut() } {
                let f: &dyn Fn(&mut TreeState<u64>) = &$body;
                f(&mut t.0);
            }
        }
    };
}

tree_op!(
    /// Open `key`.
    libgui_tree_expand(key: u64) |t| t.expand(key)
);
tree_op!(
    /// Close `key`.
    libgui_tree_collapse(key: u64) |t| t.collapse(key)
);
tree_op!(
    /// Open or close `key`.
    libgui_tree_toggle(key: u64) |t| t.toggle(key)
);
tree_op!(
    /// The tree changed — nodes added, removed, children loaded — and the
    /// visible rows must be worked out again next frame.
    libgui_tree_invalidate() |t| t.invalidate()
);

/// # Safety
/// `tree` null or from `libgui_tree_new`.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_is_expanded(tree: *const LibguiTree, key: u64) -> u8 {
    unsafe { tree.as_ref() }.is_some_and(|t| t.0.is_expanded(key)) as u8
}

/// The node the keyboard is on; returns 0 if none.
///
/// # Safety
/// `tree` null or from `libgui_tree_new`; `out` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_cursor(tree: *const LibguiTree, out: *mut u64) -> u8 {
    match (unsafe { tree.as_ref() }.and_then(|t| t.0.cursor()), unsafe { out.as_mut() }) {
        (Some(k), Some(o)) => {
            *o = k;
            1
        }
        (Some(_), None) => 1,
        _ => 0,
    }
}

/// Open every ancestor of `key`, put the keyboard on it, and scroll it into
/// view next frame — how a node picked in the 3D view is shown. `parent`
/// returns a node's parent, or `LIBGUI_TREE_ROOT` for a top-level node.
///
/// # Safety
/// `tree` null or from `libgui_tree_new`; `parent` null or a valid function.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_reveal(
    tree: *mut LibguiTree,
    key: u64,
    parent: Option<unsafe extern "C" fn(user: *mut std::ffi::c_void, node: u64) -> u64>,
    user: *mut std::ffi::c_void,
) {
    let (Some(t), Some(p)) = (unsafe { tree.as_mut() }, parent) else { return };
    t.0.reveal(key, |k| {
        let up = unsafe { p(user, k) };
        (up != LIBGUI_TREE_ROOT).then_some(up)
    });
}

/// Turn a click or move into keys, keeping a range's anchor by key. `kind` is
/// 0 replace, 1 toggle, 2 range — what `libgui_select_kind` returns. Writes
/// the selected keys into `out` (at most `cap`) and returns how many there
/// are; `*out_kind` says which of the three it was.
///
/// # Safety
/// `tree` null or from `libgui_tree_new`; `out` null or `cap` writable;
/// `out_kind` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_select(
    tree: *mut LibguiTree,
    key: u64,
    kind: i32,
    out: *mut u64,
    cap: u64,
    out_kind: *mut i32,
) -> u64 {
    let Some(t) = (unsafe { tree.as_mut() }) else { return 0 };
    let kind = match kind {
        1 => SelectKind::Toggle,
        2 => SelectKind::Range,
        _ => SelectKind::Replace,
    };
    let (k, keys) = match t.0.select(key, kind) {
        TreeSelection::Only(k) => (0, vec![k]),
        TreeSelection::Toggle(k) => (1, vec![k]),
        TreeSelection::Range(v) => (2, v),
    };
    if let Some(o) = unsafe { out_kind.as_mut() } {
        *o = k;
    }
    if !out.is_null() {
        let n = keys.len().min(cap as usize);
        unsafe { std::ptr::copy_nonoverlapping(keys.as_ptr(), out, n) };
    }
    keys.len() as u64
}

/// What a tree view did this frame. Each event is a key and a flag saying
/// whether it happened.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiTreeViewResponse {
    pub clicked: u64,
    pub activated: u64,
    pub moved: u64,
    pub expanded: u64,
    pub collapsed: u64,
    pub has_clicked: u8,
    pub has_activated: u8,
    pub has_moved: u8,
    pub has_expanded: u8,
    pub has_collapsed: u8,
    /// The move was Shift+arrow: extend the selection.
    pub extend: u8,
    pub _pad: [u8; 2],
    pub modifiers: LibguiModifiers,
    pub _pad2: u32,
    /// Visible rows in all, and the range built.
    pub rows: u64,
    pub built_first: u64,
    pub built_end: u64,
}

/// A tree of any size that builds only the rows on screen.
///
/// # Safety
/// `ui` null or live; `key` a string; `tree` null or from `libgui_tree_new`;
/// `source` null or valid, its callbacks honouring `LibguiTreeSource`.
#[no_mangle]
pub unsafe extern "C" fn libgui_tree_view(
    ui: *mut LibguiUi,
    key: *const c_char,
    tree: *mut LibguiTree,
    source: *const LibguiTreeSource,
) -> LibguiTreeViewResponse {
    let key = unsafe { crate::convert::str_or_empty(key, "libgui_tree_view") };
    let (Some(t), Some(src)) = (unsafe { tree.as_mut() }, unsafe { source.as_ref() }) else {
        set_error("libgui_tree_view: tree or source is null");
        return LibguiTreeViewResponse::default();
    };
    let src = Source { t: *src };
    with_ui(ui, LibguiTreeViewResponse::default(), |u| {
        // The source's callbacks run inside: refuse anything they send back.
        unsafe { (*ui).answering = true };
        let r = u.tree_view(key, &mut t.0, &src);
        unsafe { (*ui).answering = false };
        let some = |k: Option<u64>| (k.unwrap_or(0), k.is_some() as u8);
        let (clicked, has_clicked) = some(r.clicked);
        let (activated, has_activated) = some(r.activated);
        let (moved, has_moved) = some(r.moved);
        let (expanded, has_expanded) = some(r.expanded);
        let (collapsed, has_collapsed) = some(r.collapsed);
        LibguiTreeViewResponse {
            clicked,
            activated,
            moved,
            expanded,
            collapsed,
            has_clicked,
            has_activated,
            has_moved,
            has_expanded,
            has_collapsed,
            extend: r.extend as u8,
            _pad: [0; 2],
            modifiers: LibguiModifiers {
                shift: r.modifiers.shift as u8,
                ctrl: r.modifiers.ctrl as u8,
                alt: r.modifiers.alt as u8,
                logo: r.modifiers.logo as u8,
            },
            _pad2: 0,
            rows: r.rows as u64,
            built_first: r.built.start as u64,
            built_end: r.built.end as u64,
        }
    })
}
