//! Saving a dock layout and putting it back.
//!
//! The interesting cases are not the round trip — they are what happens when
//! the app is not the same app that saved it: a panel removed, a panel added,
//! a file edited by hand or copied from a newer version.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// The app's panels. The id is a hash of a stable name, not a discriminant,
/// so adding or reordering panels does not renumber the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Outliner,
    Inspector,
    Viewport,
    Console,
    /// Added by a later version of the "app".
    Timeline,
}

impl Tab {
    const V1: [Tab; 4] = [Tab::Outliner, Tab::Inspector, Tab::Viewport, Tab::Console];
    const V2: [Tab; 5] = [Tab::Outliner, Tab::Inspector, Tab::Viewport, Tab::Console, Tab::Timeline];

    fn name(self) -> &'static str {
        match self {
            Tab::Outliner => "outliner",
            Tab::Inspector => "inspector",
            Tab::Viewport => "viewport",
            Tab::Console => "console",
            Tab::Timeline => "timeline",
        }
    }

    fn id(self) -> u64 {
        Id::new(self.name()).0
    }

    fn from_id(id: u64, offered: &[Tab]) -> Option<Tab> {
        offered.iter().copied().find(|t| t.id() == id)
    }
}

struct Viewer;

impl TabViewer for Viewer {
    type Tab = Tab;
    fn title(&self, tab: &Tab) -> String {
        tab.name().to_string()
    }
    fn id(&self, tab: &Tab) -> u64 {
        tab.id()
    }
    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        ui.label(tab.name());
    }
}

fn new_ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// The editor layout: outliner left, viewport and console stacked centre,
/// inspector right.
fn build(dock: &mut DockState<Tab>) {
    let left = dock.leaf(vec![Tab::Outliner]);
    let centre = dock.leaf(vec![Tab::Viewport, Tab::Console]);
    let right = dock.leaf(vec![Tab::Inspector]);
    let cr = dock.split(Axis::X, 0.7, centre, right);
    let root = dock.split(Axis::X, 0.25, left, cr);
    dock.set_root(SurfaceId::MAIN, root);
}

/// Run frames so the dock lays itself out, as a host would.
fn show(dock: &mut DockState<Tab>, ui: &mut Ui) {
    for _ in 0..3 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(1000.0, 700.0), scale: 1.0, dt: 1.0 / 60.0 });
        dock.show(ui, SurfaceId::MAIN, &mut Viewer);
        let _ = ui.end_frame();
    }
}

/// Tabs in tree order, per surface: what the user actually sees.
fn shape(dock: &DockState<Tab>) -> Vec<Vec<&'static str>> {
    fn walk(node: &DockNode<Tab>, out: &mut Vec<&'static str>) {
        match node {
            DockNode::Leaf(l) => out.extend(l.tabs.iter().map(|t| t.name())),
            DockNode::Split(s) => {
                walk(&s.first, out);
                walk(&s.second, out);
            }
        }
    }
    dock.surfaces()
        .iter()
        .map(|s| {
            let mut v = Vec::new();
            if let Some(r) = &s.root {
                walk(r, &mut v);
            }
            v
        })
        .collect()
}

/// TOML is one way to store a layout, and the one libgui bundles; the layout
/// itself is plain data and does not need it.
#[cfg(feature = "theme-toml")]
#[test]
fn a_layout_survives_a_round_trip_through_text() {
    let mut dock = DockState::new();
    let mut ui = new_ui();
    build(&mut dock);
    show(&mut dock, &mut ui);
    let before = shape(&dock);
    let saved = dock.layout(&Viewer);

    let text = saved.to_toml().expect("serialise");
    assert!(text.contains("version"), "the layout should say what wrote it:\n{text}");
    let parsed = DockLayout::from_toml(&text).expect("parse");
    assert_eq!(parsed, saved, "the layout changed on its way through TOML");

    // A fresh dock, as if the app had just started.
    let mut restored = DockState::new();
    let mut ui2 = new_ui();
    let report = restored.restore(&parsed, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
    show(&mut restored, &mut ui2);
    assert_eq!(shape(&restored), before, "the panels came back in a different arrangement");
    assert!(report.dropped.is_empty());
    assert_eq!(report.placed.len(), 4);
    assert_eq!(restored.layout(&Viewer), saved, "saving the restored layout gives a different file");
}

/// Splits, fractions and which tab is in front are part of the layout.
/// The same round trip without the TOML: the dock's own save and restore.
#[test]
fn a_layout_survives_a_round_trip_in_memory() {
    let mut dock = DockState::new();
    let mut ui = new_ui();
    build(&mut dock);
    show(&mut dock, &mut ui);
    let before = shape(&dock);
    let saved = dock.layout(&Viewer);

    let mut restored = DockState::new();
    let mut ui2 = new_ui();
    let report = restored.restore(&saved, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
    show(&mut restored, &mut ui2);
    assert_eq!(shape(&restored), before, "the panels came back in a different arrangement");
    assert!(report.dropped.is_empty());
    assert_eq!(report.placed.len(), 4);
    assert_eq!(restored.layout(&Viewer), saved, "saving the restored layout gives a different snapshot");
}

#[test]
fn fractions_and_active_tabs_come_back() {
    let mut dock = DockState::new();
    let mut ui = new_ui();
    build(&mut dock);
    // Bring the console to the front of its stack.
    if let Some(DockNode::Split(root)) = dock.surface(SurfaceId::MAIN).and_then(|s| s.root.as_ref()) {
        let _ = root;
    }
    show(&mut dock, &mut ui);
    let saved = dock.layout(&Viewer);
    let mut with_console = saved.clone();
    fn set_active(n: &mut NodeLayout) {
        match n {
            NodeLayout::Leaf { tabs, active, .. } => {
                if tabs.len() > 1 {
                    *active = 1;
                }
            }
            NodeLayout::Split { first, second, .. } => {
                set_active(first);
                set_active(second);
            }
        }
    }
    set_active(with_console.surfaces[0].root.as_mut().unwrap());

    let mut restored = DockState::new();
    restored.restore(&with_console, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
    let again = restored.layout(&Viewer);
    assert_eq!(again, with_console, "the active tab or a fraction was lost");
}

/// The app dropped a panel since the layout was saved: it goes, and so does
/// the pane it was alone in — no empty hole.
#[test]
fn a_panel_the_app_no_longer_has_is_dropped() {
    let mut dock = DockState::new();
    let mut ui = new_ui();
    build(&mut dock);
    show(&mut dock, &mut ui);
    let saved = dock.layout(&Viewer);

    // This version has no inspector.
    let offered = [Tab::Outliner, Tab::Viewport, Tab::Console];
    let mut restored = DockState::new();
    let mut ui2 = new_ui();
    let report = restored.restore(&saved, |id| Tab::from_id(id, &offered)).expect("restore");
    show(&mut restored, &mut ui2);
    assert_eq!(report.dropped, vec![Tab::Inspector.id()]);
    assert_eq!(shape(&restored), vec![vec!["outliner", "viewport", "console"]]);
    // The split that held it collapsed rather than leaving a blank pane.
    let layout = restored.layout(&Viewer);
    let mut leaves = 0;
    fn count(n: &NodeLayout, leaves: &mut usize) {
        match n {
            NodeLayout::Leaf { .. } => *leaves += 1,
            NodeLayout::Split { first, second, .. } => {
                count(first, leaves);
                count(second, leaves);
            }
        }
    }
    count(layout.surfaces[0].root.as_ref().unwrap(), &mut leaves);
    assert_eq!(leaves, 2, "the emptied pane was left behind");
}

/// The app gained a panel since: it is not in the layout, and must not be
/// silently lost.
#[test]
fn a_panel_the_layout_never_saw_is_reported() {
    let mut dock = DockState::new();
    let mut ui = new_ui();
    build(&mut dock);
    show(&mut dock, &mut ui);
    let saved = dock.layout(&Viewer);

    let mut restored = DockState::new();
    let report = restored.restore(&saved, |id| Tab::from_id(id, &Tab::V2)).expect("restore");
    let missing = report.missing_from(Tab::V2.iter().map(|t| t.id()));
    assert_eq!(missing, vec![Tab::Timeline.id()], "the new panel was not reported");
    assert!(!report.is_empty());

    // And the app can then dock it itself, either bluntly...
    let mut blunt = DockState::new();
    blunt.restore(&saved, |id| Tab::from_id(id, &Tab::V2)).expect("restore");
    blunt.add_tab(SurfaceId::MAIN, Tab::Timeline);
    assert_eq!(shape(&blunt)[0].len(), 5);

    // ...or somewhere specific, by wrapping the restored tree.
    let leaf = restored.leaf(vec![Tab::Timeline]);
    let old = restored.take_root(SurfaceId::MAIN).expect("a restored tree");
    let root = restored.split(Axis::Y, 0.8, old, leaf);
    restored.set_root(SurfaceId::MAIN, root);
    assert_eq!(shape(&restored)[0], vec!["outliner", "viewport", "console", "inspector", "timeline"]);
}

/// A layout from a newer version is refused rather than half-applied.
#[test]
fn a_newer_layout_is_refused() {
    let mut dock = DockState::new();
    build(&mut dock);
    let mut saved = dock.layout(&Viewer);
    saved.version = DockLayout::VERSION + 7;

    let mut restored = DockState::new();
    build(&mut restored);
    let before = shape(&restored);
    let err = restored.restore(&saved, |id| Tab::from_id(id, &Tab::V1)).unwrap_err();
    assert!(matches!(err, LayoutError::Version { found, supported } if found == DockLayout::VERSION + 7 && supported == DockLayout::VERSION));
    assert_eq!(shape(&restored), before, "a refused layout still changed the dock");
    assert!(err.to_string().contains("newer"), "{err}");
}

/// Layout files get hand-edited and copied between machines. Nothing in one
/// should be able to produce a broken dock.
#[test]
fn nonsense_in_a_file_is_repaired_not_trusted() {
    let mut dock = DockState::new();
    build(&mut dock);
    let mut saved = dock.layout(&Viewer);
    let s = &mut saved.surfaces[0];
    s.size = Vec2::new(f32::NAN, -5.0);
    s.rect = Rect::new(f32::INFINITY, 0.0, 0.0, f32::NAN);
    fn wreck(n: &mut NodeLayout) {
        match n {
            NodeLayout::Leaf { active, .. } => *active = 99,
            NodeLayout::Split { fraction, first, second, .. } => {
                *fraction = f32::NAN;
                wreck(first);
                wreck(second);
            }
        }
    }
    wreck(s.root.as_mut().unwrap());

    let mut restored = DockState::new();
    let mut ui = new_ui();
    restored.restore(&saved, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
    show(&mut restored, &mut ui);
    let out = restored.layout(&Viewer);
    let surface = &out.surfaces[0];
    assert!(surface.size.x.is_finite() && surface.size.x > 0.0, "size {:?}", surface.size);
    assert!(surface.rect.x.is_finite() && surface.rect.h.is_finite(), "rect {:?}", surface.rect);
    fn check(n: &NodeLayout) {
        match n {
            NodeLayout::Leaf { tabs, active, .. } => assert!(*active < tabs.len(), "active {active} of {}", tabs.len()),
            NodeLayout::Split { fraction, first, second, .. } => {
                assert!((0.0..=1.0).contains(fraction), "fraction {fraction}");
                check(first);
                check(second);
            }
        }
    }
    check(surface.root.as_ref().unwrap());
    // And it still draws.
    assert_eq!(shape(&restored)[0].len(), 4);
}

/// An empty or unusable layout says so, so the app can fall back to its
/// default rather than starting with a blank window.
#[test]
fn a_layout_with_nothing_left_is_empty() {
    let mut dock = DockState::new();
    build(&mut dock);
    let saved = dock.layout(&Viewer);

    let mut restored = DockState::new();
    let report = restored.restore(&saved, |_| None).expect("restore");
    assert!(report.is_empty(), "nothing was recognised, but the report does not say so");
    assert_eq!(report.dropped.len(), 4);
    assert_eq!(shape(&restored), vec![Vec::<&str>::new()], "an empty dock should have no panels");
}

/// Torn-off windows are part of the layout too.
#[test]
fn floating_windows_come_back() {
    let mut dock = DockState::new();
    build(&mut dock);
    let saved = dock.layout(&Viewer);
    // A second surface, as a tear-off produces.
    let mut with_window = saved.clone();
    with_window.surfaces.push(SurfaceLayout {
        floating: true,
        size: Vec2::new(420.0, 300.0),
        rect: Rect::new(120.0, 90.0, 420.0, 300.0),
        root: Some(NodeLayout::Leaf { tabs: vec![Tab::Console.id()], active: 0, titles: Vec::new() }),
    });

    let mut restored = DockState::new();
    restored.restore(&with_window, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
    assert_eq!(restored.surfaces().len(), 2, "the torn-off window did not come back");
    let floating = &restored.surfaces()[1];
    assert!(floating.floating);
    assert_eq!(floating.window_size, Vec2::new(420.0, 300.0));
    assert_eq!(shape(&restored)[1], vec!["console"]);

    // A floating window whose only tab is gone is not reopened empty.
    let mut restored2 = DockState::new();
    let offered = [Tab::Outliner, Tab::Inspector, Tab::Viewport];
    restored2.restore(&with_window, |id| Tab::from_id(id, &offered)).expect("restore");
    assert_eq!(restored2.surfaces().len(), 1, "an empty window was reopened");
}

/// The titles in a file are a note to whoever reads it. A restore must not
/// care what they say, or a hand-edited file could contradict itself.
#[cfg(feature = "theme-toml")]
#[test]
fn titles_are_a_hint_and_nothing_more() {
    let mut dock = DockState::new();
    build(&mut dock);
    let saved = dock.layout(&Viewer);
    let text = saved.to_toml().expect("serialise");
    assert!(text.contains("outliner"), "the file names no panels:\n{text}");

    // A file whose titles are wrong, and one where they are missing entirely
    // (written by a build that had not thought of them yet).
    let lying = text.replace("\"outliner\"", "\"something else entirely\"");
    // A file from a build that had not thought of titles yet: they are
    // skipped when empty, so clearing them is the same as never writing them.
    let mut without = saved.clone();
    fn strip(n: &mut NodeLayout) {
        match n {
            NodeLayout::Leaf { titles, .. } => titles.clear(),
            NodeLayout::Split { first, second, .. } => {
                strip(first);
                strip(second);
            }
        }
    }
    strip(without.surfaces[0].root.as_mut().unwrap());
    let bare = without.to_toml().expect("serialise");
    assert!(!bare.contains("titles"), "titles were written even when empty:\n{bare}");
    for (what, file) in [("wrong titles", lying), ("no titles", bare)] {
        let parsed = DockLayout::from_toml(&file).unwrap_or_else(|e| panic!("{what}: {e}"));
        let mut restored = DockState::new();
        restored.restore(&parsed, |id| Tab::from_id(id, &Tab::V1)).expect("restore");
        assert_eq!(shape(&restored), shape(&dock), "{what} changed where the panels went");
        assert_eq!(restored.layout(&Viewer), saved, "{what}: the titles were not rewritten from the app");
    }
}

/// Hashed tab ids use all 64 bits. TOML integers are signed 64-bit, so an id
/// above `i64::MAX` written as a bare integer is out of spec — the `toml`
/// crate happens to accept it, other readers need not, and a JavaScript JSON
/// reader rounds anything above 2^53. So ids are written as strings.
#[cfg(feature = "theme-toml")]
#[test]
fn every_id_is_written_in_a_form_any_reader_accepts() {
    let saved = DockLayout {
        version: DockLayout::VERSION,
        surfaces: vec![SurfaceLayout {
            floating: false,
            size: Vec2::ZERO,
            rect: Rect::default(),
            // The two ends of the range, and one a hash would plausibly give.
            root: Some(NodeLayout::Leaf { tabs: vec![0, u64::MAX, 0x9f3c_52d1_07aa_e6b4], active: 0, titles: vec![] }),
        }],
    };
    let text = saved.to_toml().expect("serialise");

    // No bare integer in the file is above i64::MAX: every token that parses
    // as a number at all must also parse as a signed one.
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Ok(n) = token.parse::<u64>() {
            assert!(i64::try_from(n).is_ok(), "{n} is out of range for a TOML integer:\n{text}");
        }
    }
    assert!(text.contains("\"0xffffffffffffffff\""), "ids are written as hex strings:\n{text}");
    assert_eq!(DockLayout::from_toml(&text).expect("parse"), saved, "and read back exactly");
}

/// Version-1 layouts wrote ids as integers, including the out-of-range ones,
/// and people have those files on disk. They must keep loading.
#[cfg(feature = "theme-toml")]
#[test]
fn a_version_one_layout_with_integer_ids_still_loads() {
    let v1 = r#"
version = 1

[[surfaces]]
floating = false
size = { x = 0.0, y = 0.0 }
rect = { x = 0.0, y = 0.0, w = 0.0, h = 0.0 }

[surfaces.root.leaf]
tabs = [7, 18446744073709551615]
active = 1
"#;
    let parsed = DockLayout::from_toml(v1).expect("a version-1 file is still a layout");
    assert_eq!(parsed.version, 1);
    match &parsed.surfaces[0].root {
        Some(NodeLayout::Leaf { tabs, active, .. }) => {
            assert_eq!(tabs, &vec![7, u64::MAX], "integer ids read back as the same ids");
            assert_eq!(*active, 1);
        }
        other => panic!("expected one leaf, got {other:?}"),
    }
}

/// A file someone has edited by hand: an id that is not one is a parse
/// error, not a tab with a made-up id.
#[cfg(feature = "theme-toml")]
#[test]
fn an_id_that_is_not_one_is_refused() {
    let with = |tabs: &str| {
        format!(
            "version = 2\n[[surfaces]]\nfloating = false\nsize = {{ x = 0.0, y = 0.0 }}\n\
             rect = {{ x = 0.0, y = 0.0, w = 0.0, h = 0.0 }}\n[surfaces.root.leaf]\ntabs = {tabs}\nactive = 0\n"
        )
    };
    assert!(DockLayout::from_toml(&with(r#"["0x2a", "42"]"#)).is_ok(), "hex and decimal strings are both ids");
    for bad in [r#"["outliner"]"#, "[-1]", r#"["0x"]"#, r#"["0x1ffffffffffffffff"]"#] {
        assert!(
            matches!(DockLayout::from_toml(&with(bad)), Err(LayoutError::Parse(_))),
            "{bad} should not parse as tab ids"
        );
    }
}
