//! The virtualised tree, against an assembly the size real ones are.

use libgui::*;
use std::cell::Cell;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const ASSEMBLIES: u32 = 100;
const PARTS: u32 = 1000;

/// 100 sub-assemblies of 1,000 parts: 100,100 nodes. Keys: assembly `a` is
/// `a`, part `p` of it is `ASSEMBLIES + a * PARTS + p`. Every call counted.
struct Assembly {
    names: Vec<String>,
    children_calls: Cell<usize>,
    roots_calls: Cell<usize>,
}

impl Assembly {
    fn new() -> Self {
        let mut names: Vec<String> = (0..ASSEMBLIES).map(|a| format!("Sub-assembly {a:03}")).collect();
        for a in 0..ASSEMBLIES {
            for p in 0..PARTS {
                names.push(format!("Part {a:03}-{p:04}"));
            }
        }
        Self { names, children_calls: Cell::new(0), roots_calls: Cell::new(0) }
    }
    fn part(a: u32, p: u32) -> u32 {
        ASSEMBLIES + a * PARTS + p
    }
    fn parent(k: u32) -> Option<u32> {
        (k >= ASSEMBLIES).then(|| (k - ASSEMBLIES) / PARTS)
    }
}

impl TreeSource for Assembly {
    type Key = u32;
    fn roots(&self, out: &mut Vec<u32>) {
        self.roots_calls.set(self.roots_calls.get() + 1);
        out.extend(0..ASSEMBLIES);
    }
    fn children(&self, node: u32, out: &mut Vec<u32>) {
        self.children_calls.set(self.children_calls.get() + 1);
        if node < ASSEMBLIES {
            out.extend((0..PARTS).map(|p| Assembly::part(node, p)));
        }
    }
    fn has_children(&self, node: u32) -> bool {
        node < ASSEMBLIES
    }
    fn label(&self, node: u32) -> std::borrow::Cow<'_, str> {
        self.names[node as usize].as_str().into()
    }
}

struct World {
    ui: Ui,
    model: Assembly,
    tree: TreeState<u32>,
    last: TreeViewResponse<u32>,
}

impl World {
    fn new() -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut b = KeyBindings::new();
        for (k, n) in [(Key::ArrowDown, Nav::Next), (Key::ArrowUp, Nav::Previous), (Key::ArrowRight, Nav::Expand), (Key::ArrowLeft, Nav::Collapse), (Key::End, Nav::Last)] {
            b.bind(Shortcut::plain(k), UiAction::Navigate(n));
            b.bind(Shortcut::plain(k).shift(), UiAction::NavigateExtend(n));
        }
        b.bind(Shortcut::plain(Key::Enter), UiAction::Activate);
        ui.set_key_bindings(b);
        let mut w = Self { ui, model: Assembly::new(), tree: TreeState::new(), last: TreeViewResponse::default() };
        for _ in 0..3 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) -> TreeViewResponse<u32> {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = self.ui.tree_view("assembly", &mut self.tree, &self.model);
        let _ = self.ui.end_frame();
        self.last = r.clone();
        r
    }

    fn focus(&mut self) {
        // The tree's collection is the first and only focus stop.
        self.ui.push(InputEvent::Action(UiAction::FocusNext));
        self.frame();
    }

    fn key(&mut self, key: Key, shift: bool) -> TreeViewResponse<u32> {
        if shift {
            self.ui.push(InputEvent::ModifiersChanged(Modifiers { shift: true, ..Default::default() }));
        }
        self.ui.push(InputEvent::Key { key, pressed: true, repeat: false });
        let r = self.frame();
        self.ui.push(InputEvent::Key { key, pressed: false, repeat: false });
        if shift {
            self.ui.push(InputEvent::ModifiersChanged(Modifiers::default()));
        }
        self.frame();
        r
    }

    fn cursor(&self) -> u32 {
        self.tree.cursor().expect("no cursor")
    }
}

#[test]
fn a_hundred_thousand_nodes_cost_a_screenful() {
    let mut w = World::new();
    for a in 0..ASSEMBLIES {
        w.tree.expand(a);
    }
    let r = w.frame();
    assert_eq!(r.rows, 100_100, "every node should be a visible row");
    assert!(r.built.len() < 40, "{} rows were built for a 400 px window", r.built.len());

    // A steady frame works nothing out again: no walk of the tree at all.
    let (roots, kids) = (w.model.roots_calls.get(), w.model.children_calls.get());
    for _ in 0..5 {
        w.frame();
    }
    assert_eq!((w.model.roots_calls.get(), w.model.children_calls.get()), (roots, kids), "a steady frame walked the tree");
}

#[test]
fn only_open_nodes_are_asked_for_their_children() {
    let mut w = World::new();
    assert_eq!(w.model.children_calls.get(), 0, "a closed tree loaded children");
    w.tree.expand(7);
    w.frame();
    assert_eq!(w.model.children_calls.get(), 1, "opening one assembly asked for {} child lists", w.model.children_calls.get());
    assert_eq!(w.last.rows, 100 + 1000);
}

#[test]
fn clicking_the_arrow_opens_and_closes_and_says_so() {
    let mut w = World::new();
    // The first row's arrow, at its left, inside the row's indent.
    let r = w.frame();
    assert!(r.built.contains(&0));
    let arrow = Vec2::new(16.0, 12.0);
    w.ui.push(InputEvent::PointerMoved { pos: arrow });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.expanded, Some(0), "clicking the arrow did not open the assembly");
    assert!(r.clicked.is_none(), "clicking the arrow also selected the row");
    assert!(w.tree.is_expanded(0));
    assert_eq!(w.tree.rows().len(), 1100, "the opened branch is not in rows() in the same frame");
}

#[test]
fn right_opens_then_steps_in_and_left_steps_out_then_closes() {
    let mut w = World::new();
    w.focus();
    assert_eq!(w.cursor(), 0);
    let r = w.key(Key::ArrowRight, false);
    assert_eq!(r.expanded, Some(0), "Right did not open the assembly");
    assert_eq!(w.cursor(), 0, "Right on a closed branch should open it, not move");
    w.key(Key::ArrowRight, false);
    assert_eq!(w.cursor(), Assembly::part(0, 0), "Right on an open branch should step onto its first child");
    w.key(Key::ArrowDown, false);
    assert_eq!(w.cursor(), Assembly::part(0, 1));
    w.key(Key::ArrowLeft, false);
    assert_eq!(w.cursor(), 0, "Left on a leaf should step out to its parent");
    let r = w.key(Key::ArrowLeft, false);
    assert_eq!(r.collapsed, Some(0), "Left on an open branch should close it");
    assert_eq!(w.tree.rows().len(), 100);
}

#[test]
fn end_reaches_the_last_row_of_an_open_tree() {
    let mut w = World::new();
    for a in 0..ASSEMBLIES {
        w.tree.expand(a);
    }
    w.focus();
    w.key(Key::End, false);
    for _ in 0..90 {
        w.frame();
    }
    assert_eq!(w.cursor(), Assembly::part(ASSEMBLIES - 1, PARTS - 1));
    assert!(w.last.built.contains(&100_099), "the last row was never brought into view: built {:?}", w.last.built);
}

/// The keyboard is on a node, not a row number: opening a branch above it
/// pushes it down the list, and the cursor goes with it.
#[test]
fn the_cursor_stays_on_its_node_when_rows_open_above_it() {
    let mut w = World::new();
    w.focus();
    for _ in 0..5 {
        w.key(Key::ArrowDown, false);
    }
    assert_eq!(w.cursor(), 5);
    w.tree.expand(2); // above the cursor: a thousand rows appear
    w.frame();
    w.frame();
    assert_eq!(w.cursor(), 5, "the cursor stayed on the row number and left its node");
    w.key(Key::ArrowDown, false);
    assert_eq!(w.cursor(), 6, "the next move started from the wrong place");
}

#[test]
fn shift_extends_a_selection_of_keys_and_the_anchor_survives_opening() {
    let mut w = World::new();
    w.focus();
    w.key(Key::ArrowDown, false); // on 1
    let r = w.last.clone();
    let _ = r;
    assert_eq!(w.tree.select(1, SelectKind::Replace), TreeSelection::Only(1));
    // Open 0, above the anchor: the anchor is a key and does not move.
    w.tree.expand(0);
    w.frame();
    let r = w.key(Key::ArrowDown, true);
    assert!(r.extend, "Shift+Down was not reported as extending");
    let picked = w.tree.select(r.moved.expect("no move"), SelectKind::Range);
    assert_eq!(picked, TreeSelection::Range(vec![1, 2]), "the range was not from the anchor's node");
}

#[test]
fn typing_jumps_to_a_node_by_its_label() {
    let mut w = World::new();
    w.focus();
    w.ui.push(InputEvent::Text("sub-assembly 04".into()));
    w.frame();
    assert_eq!(w.cursor(), 40, "type-ahead went to {}", w.model.label(w.cursor()));
}

#[test]
fn revealing_a_node_opens_its_parents_and_shows_it() {
    let mut w = World::new();
    let target = Assembly::part(73, 512);
    w.tree.reveal(target, Assembly::parent);
    for _ in 0..90 {
        w.frame();
    }
    assert!(w.tree.is_expanded(73), "its assembly was not opened");
    let i = w.tree.index_of(target).expect("not visible");
    assert_eq!(w.cursor(), target);
    assert!(w.last.built.contains(&i), "revealed node {i} is not on screen: built {:?}", w.last.built);
}

#[test]
fn a_double_click_opens_a_branch_and_activates_a_leaf() {
    let mut w = World::new();
    let activated = std::cell::Cell::new(None);
    let dbl = |w: &mut World, y: f32| {
        let at = Vec2::new(150.0, y);
        for _ in 0..2 {
            w.ui.push(InputEvent::PointerMoved { pos: at });
            w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
            // A double click is reported on the press that makes it one.
            if let Some(k) = w.frame().activated {
                activated.set(Some(k));
            }
            w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
            if let Some(k) = w.frame().activated {
                activated.set(Some(k));
            }
        }
    };
    dbl(&mut w, 12.0); // row 0: an assembly
    assert!(w.tree.is_expanded(0), "a double click did not open the branch");
    activated.set(None);
    // A person sees the opened branch before clicking in it: a moment passes.
    for _ in 0..40 {
        w.frame();
    }
    let row_h = w.ui.theme.selectable.height;
    dbl(&mut w, row_h + 12.0); // row 1: its first part, now visible
    assert_eq!(activated.get(), Some(Assembly::part(0, 0)), "a double click on a leaf did not activate it");
}

