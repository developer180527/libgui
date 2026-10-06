//! A tree that builds only what is on screen: a CAD assembly browser with a
//! hundred thousand parts costs what a screenful of rows does.
//!
//! libgui does not know your tree's shape and does not want to. You describe
//! it through [`TreeSource`] — roots, children, labels — and libgui asks for
//! the children of **expanded** nodes only, so a part list that loads on
//! demand never loads what nobody opened.
//!
//! What is on screen comes from a flattened list of the visible rows, kept in
//! a [`TreeState`] the app owns beside its tree. It is rebuilt when something
//! is expanded or collapsed, or when you say the tree changed
//! ([`TreeState::invalidate`]) — not every frame. Each frame then draws only
//! the rows in view, through the virtual list.
//!
//! The keyboard is the platform's: Up and Down move, Right opens a branch or
//! steps into it, Left closes it or steps out to the parent, Enter or a
//! double click activates, Shift extends, and typing jumps by label.
//! Selection stays yours; [`TreeViewResponse`] reports what happened, and
//! [`TreeState::select`] turns it into keys.

use crate::{Branch, ListOptions, Modifiers, NavResponse, SelectKind, Ui};
use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Range;

/// Your tree, as a [`Ui::tree_view`] reads it.
///
/// `Key` names a node and must be stable across frames: an index into an
/// arena, an entity id. `children` is only called for nodes that are
/// expanded, so an implementation may load lazily.
pub trait TreeSource {
    type Key: Copy + Eq + Hash;
    /// The top-level nodes, in order.
    fn roots(&self, out: &mut Vec<Self::Key>);
    /// `node`'s children, in order. Only asked of expanded nodes.
    fn children(&self, node: Self::Key, out: &mut Vec<Self::Key>);
    /// Whether `node` gets a disclosure arrow. Cheap: asked of every visible
    /// row, without loading the children.
    fn has_children(&self, node: Self::Key) -> bool;
    /// What the row shows, and what type-ahead matches. Borrow it when you
    /// have it (`name.as_str().into()`), or build it when you do not.
    fn label(&self, node: Self::Key) -> Cow<'_, str>;
    /// Whether the row is drawn selected. Selection is yours.
    fn selected(&self, _node: Self::Key) -> bool {
        false
    }
}

/// One visible row: a node, how deep it is, and whether it can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow<K> {
    pub key: K,
    pub depth: usize,
    pub branch: Branch,
}

/// What a tree keeps between frames: which nodes are open, and the visible
/// rows that follows from that. Keep one beside your tree.
pub struct TreeState<K> {
    expanded: HashSet<K>,
    rows: Vec<TreeRow<K>>,
    stale: bool,
    /// Where the keyboard is, by key, so it stays on the same node when rows
    /// open or close above it.
    cursor: Option<K>,
    /// Where a range selection starts, by key, for the same reason.
    anchor: Option<K>,
    /// The cursor was put somewhere from code — `reveal` — and the view must
    /// follow it on the next frame, as it follows the keyboard.
    show_cursor: bool,
    // Reused while flattening, so a rebuild does not allocate per node.
    stack: Vec<(K, usize)>,
    kids: Vec<K>,
}

impl<K: Copy + Eq + Hash> Default for TreeState<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Copy + Eq + Hash> TreeState<K> {
    pub fn new() -> Self {
        Self {
            expanded: HashSet::new(),
            rows: Vec::new(),
            stale: true,
            cursor: None,
            anchor: None,
            show_cursor: false,
            stack: Vec::new(),
            kids: Vec::new(),
        }
    }

    /// The tree changed — a node added, removed, renamed under a sort, or
    /// children loaded — and the visible rows must be worked out again on the
    /// next frame. Expanding and collapsing do this themselves.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn is_expanded(&self, key: K) -> bool {
        self.expanded.contains(&key)
    }

    pub fn expand(&mut self, key: K) {
        if self.expanded.insert(key) {
            self.stale = true;
        }
    }

    pub fn collapse(&mut self, key: K) {
        if self.expanded.remove(&key) {
            self.stale = true;
        }
    }

    pub fn toggle(&mut self, key: K) {
        if self.is_expanded(key) {
            self.collapse(key);
        } else {
            self.expand(key);
        }
    }

    /// Open every ancestor of `key` and put the keyboard on it — how a node
    /// selected in the 3D view is shown in the browser. `parent` answers
    /// what contains a node; libgui cannot know.
    pub fn reveal(&mut self, key: K, parent: impl Fn(K) -> Option<K>) {
        let mut at = parent(key);
        while let Some(p) = at {
            self.expand(p);
            at = parent(p);
        }
        self.cursor = Some(key);
        self.show_cursor = true;
    }

    /// The visible rows as of the last frame, for building rows of your own.
    pub fn rows(&self) -> &[TreeRow<K>] {
        &self.rows
    }

    /// Where `key` is among the visible rows, if it is visible.
    pub fn index_of(&self, key: K) -> Option<usize> {
        self.rows.iter().position(|r| r.key == key)
    }

    /// The node the keyboard is on.
    pub fn cursor(&self) -> Option<K> {
        self.cursor
    }

    /// Turn a click or keyboard move into keys, keeping the anchor a range
    /// needs — by key, so opening a branch above it does not move it.
    /// `Range` is every visible row between the anchor and `key`.
    pub fn select(&mut self, key: K, kind: SelectKind) -> TreeSelection<K> {
        match kind {
            SelectKind::Replace => {
                self.anchor = Some(key);
                TreeSelection::Only(key)
            }
            SelectKind::Toggle => {
                self.anchor = Some(key);
                TreeSelection::Toggle(key)
            }
            SelectKind::Range => {
                let to = self.index_of(key);
                let from = self.anchor.and_then(|a| self.index_of(a)).or(to);
                match (from, to) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                        TreeSelection::Range(self.rows[lo..=hi].iter().map(|r| r.key).collect())
                    }
                    _ => TreeSelection::Only(key),
                }
            }
        }
    }

    /// Work out the visible rows again, if anything changed since last time.
    fn refresh<S: TreeSource<Key = K>>(&mut self, source: &S) {
        if !self.stale {
            return;
        }
        self.stale = false;
        self.rows.clear();
        self.stack.clear();
        self.kids.clear();
        source.roots(&mut self.kids);
        // Depth-first, children pushed in reverse so they pop in order.
        for &k in self.kids.iter().rev() {
            self.stack.push((k, 0));
        }
        while let Some((key, depth)) = self.stack.pop() {
            let branch = if !source.has_children(key) {
                Branch::Leaf
            } else if self.expanded.contains(&key) {
                Branch::Expanded
            } else {
                Branch::Collapsed
            };
            self.rows.push(TreeRow { key, depth, branch });
            if branch == Branch::Expanded {
                self.kids.clear();
                source.children(key, &mut self.kids);
                for &c in self.kids.iter().rev() {
                    self.stack.push((c, depth + 1));
                }
            }
        }
    }
}

/// A selection in keys, from [`TreeState::select`]: the tree's form of
/// [`Selection`](crate::Selection), which is in indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeSelection<K> {
    /// Select only this node.
    Only(K),
    /// Flip this node.
    Toggle(K),
    /// Select exactly these nodes, in the order they are shown.
    Range(Vec<K>),
}

/// What a [`Ui::tree_view`] did this frame.
#[derive(Clone, Debug)]
pub struct TreeViewResponse<K> {
    /// A row was clicked, and with which modifiers — feed it to
    /// `libgui_keymap::select_kind` and [`TreeState::select`].
    pub clicked: Option<K>,
    pub modifiers: Modifiers,
    /// Enter on the cursor, or a double click: open it.
    pub activated: Option<K>,
    /// The keyboard moved to this node this frame. With `extend`, Shift was
    /// held: grow the selection rather than replace it.
    pub moved: Option<K>,
    pub extend: bool,
    /// A node was opened or closed this frame — by its arrow, the keyboard,
    /// or a double click. For loading children on demand.
    pub expanded: Option<K>,
    pub collapsed: Option<K>,
    /// How many rows are visible in all, and which were built.
    pub rows: usize,
    pub built: Range<usize>,
}

impl<K> Default for TreeViewResponse<K> {
    fn default() -> Self {
        Self {
            clicked: None,
            modifiers: Modifiers::default(),
            activated: None,
            moved: None,
            extend: false,
            expanded: None,
            collapsed: None,
            rows: 0,
            built: 0..0,
        }
    }
}

impl Ui {
    /// A tree of any size that builds only the rows on screen. See
    /// [`TreeSource`] and [`TreeState`].
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # struct Assembly;
    /// # impl TreeSource for Assembly {
    /// #     type Key = u32;
    /// #     fn roots(&self, out: &mut Vec<u32>) {}
    /// #     fn children(&self, n: u32, out: &mut Vec<u32>) {}
    /// #     fn has_children(&self, n: u32) -> bool { false }
    /// #     fn label(&self, n: u32) -> std::borrow::Cow<'_, str> { "".into() }
    /// # }
    /// # fn f(ui: &mut Ui, model: &Assembly, tree: &mut TreeState<u32>, picked: &mut Vec<u32>) {
    /// let r = ui.tree_view("assembly", tree, model);
    /// if let Some(k) = r.clicked.or(r.moved) {
    ///     let kind = if r.extend { SelectKind::Range } else { SelectKind::Replace };
    ///     match tree.select(k, kind) {
    ///         TreeSelection::Only(k) => *picked = vec![k],
    ///         TreeSelection::Range(keys) => *picked = keys,
    ///         TreeSelection::Toggle(k) => picked.push(k),
    ///     }
    /// }
    /// # }
    /// ```
    pub fn tree_view<S: TreeSource>(&mut self, key: &str, state: &mut TreeState<S::Key>, source: &S) -> TreeViewResponse<S::Key> {
        let mut out = TreeViewResponse::default();
        let id = self.make_id(("tree_view", key));
        self.keep_id(id);

        state.refresh(source);
        let mut nav = self.open_collection(key, state.rows.len());
        // Keep the keyboard on the same node when rows opened or closed above
        // it: the collection remembers an index, the tree a key.
        if let Some(c) = state.cursor.and_then(|k| state.index_of(k)) {
            if !nav.moved && c != nav.cursor {
                nav.cursor = c;
                self.set_cursor(nav.id, c);
            }
        }
        self.type_ahead(&mut nav, state.rows.len(), |i| source.label(state.rows[i].key));
        self.tree_keys(&mut nav, state, &mut out);
        // A branch the keyboard opened shows this frame, not the next.
        state.refresh(source);

        out.rows = state.rows.len();
        out.extend = nav.extend;
        // The view follows the keyboard, and a cursor placed from code.
        let reveal = (nav.moved || std::mem::take(&mut state.show_cursor)).then_some(nav.cursor);
        let row_h = self.theme.selectable.height;
        let opts = ListOptions { reveal, ..ListOptions::new(row_h) };
        let rows = &state.rows;
        // Only allocated on a frame something was clicked.
        let mut events: Vec<(usize, crate::TreeResponse)> = Vec::new();
        out.built = self.virtual_list_with(key, rows.len(), opts, |ui, i| {
            let row = rows[i];
            // Keyed by node, not by position, so a row's hover and its
            // animation stay with it when rows open or close above it.
            let label = source.label(row.key);
            let r = ui.tree_row(("tree_view_row", row.key), row.depth, row.branch, &label, source.selected(row.key));
            if r.toggled || r.response.clicked || r.response.double_clicked {
                events.push((i, r));
            }
        });
        self.close_collection();

        for (i, r) in events {
            let Some(row) = state.rows.get(i).copied() else { continue };
            if r.toggled {
                toggle(state, row, &mut out);
            } else if r.response.double_clicked && row.branch != Branch::Leaf {
                toggle(state, row, &mut out);
                out.activated = Some(row.key);
            } else if r.response.double_clicked {
                out.activated = Some(row.key);
            } else if r.response.clicked {
                out.clicked = Some(row.key);
                out.modifiers = r.response.modifiers;
                state.cursor = Some(row.key);
                self.set_cursor(nav.id, i);
            }
        }
        // What was opened or closed by the pointer is in `rows()` at once.
        // This frame was drawn before the click was seen, so the screen shows
        // it next frame — which the release already asks for: a press is an
        // active gesture until the frame after it ends.
        state.refresh(source);
        out
    }

    /// The tree's own keys: Right opens or steps in, Left closes or steps
    /// out, and every move is remembered by key.
    fn tree_keys<K: Copy + Eq + Hash>(&mut self, nav: &mut NavResponse, state: &mut TreeState<K>, out: &mut TreeViewResponse<K>) {
        let len = state.rows.len();
        if len == 0 {
            return;
        }
        let i = nav.cursor.min(len - 1);
        let row = state.rows[i];
        if nav.expand {
            match row.branch {
                Branch::Collapsed => toggle(state, row, out),
                // Already open: step onto its first child, the next row.
                Branch::Expanded if i + 1 < len => {
                    nav.cursor = i + 1;
                    nav.moved = true;
                    self.set_cursor(nav.id, i + 1);
                }
                _ => {}
            }
        }
        if nav.collapse {
            if row.branch == Branch::Expanded {
                toggle(state, row, out);
            } else if let Some(p) = (0..i).rev().find(|&k| state.rows[k].depth + 1 == row.depth) {
                // Not open: step out to the parent, the nearest row above
                // that is one level shallower.
                nav.cursor = p;
                nav.moved = true;
                self.set_cursor(nav.id, p);
            }
        }
        if nav.activated {
            out.activated = Some(state.rows[nav.cursor.min(len - 1)].key);
        }
        let now = state.rows[nav.cursor.min(len - 1)].key;
        if nav.moved {
            out.moved = Some(now);
        }
        state.cursor = Some(now);
    }
}

fn toggle<K: Copy + Eq + Hash>(state: &mut TreeState<K>, row: TreeRow<K>, out: &mut TreeViewResponse<K>) {
    if state.is_expanded(row.key) {
        state.collapse(row.key);
        out.collapsed = Some(row.key);
    } else {
        state.expand(row.key);
        out.expanded = Some(row.key);
    }
}
