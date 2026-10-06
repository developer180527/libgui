//! A tab docked into a window that saw no input still shows up at once.
//!
//! Reported from the docking demo: dragging a torn-off window's tab back into
//! the main window took a long time to land. The dock moved the tab on the
//! release, but the host — like every host that idles — rebuilds a window's
//! UI only when that window's `Ui` says it needs a frame, and the main
//! window's `Ui` had seen no input: the pointer was over the other window the
//! whole time. So the main window kept presenting its old frame until the
//! mouse happened to wander back over it.
//!
//! These drive a host the way the demo is written — one `Ui` per OS window,
//! input delivered only to the window under the pointer, a window rebuilt only
//! when its gate passes — with the gate as it was and as it is now.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const MAIN_SIZE: Vec2 = Vec2::new(800.0, 600.0);
/// Where the torn-off window sits on screen.
const FLOAT_AT: Vec2 = Vec2::new(1000.0, 700.0);

struct V;
impl TabViewer for V {
    type Tab = &'static str;
    fn title(&self, t: &&'static str) -> String {
        t.to_string()
    }
    fn id(&self, t: &&'static str) -> u64 {
        t.as_ptr() as u64
    }
    fn ui(&mut self, ui: &mut Ui, t: &mut &'static str) {
        ui.label(t);
    }
}

struct Win {
    ui: Ui,
    surface: SurfaceId,
    size: Vec2,
    /// How many tabs this window's last *built* frame showed: what the user
    /// sees, which is not necessarily what the dock holds.
    drawn_tabs: usize,
    frames_built: usize,
}

struct Host {
    dock: DockState<&'static str>,
    main: Win,
    float: Option<Win>,
    /// The fix: also ask the dock.
    ask_dock: bool,
}

impl Host {
    fn new(ask_dock: bool) -> Self {
        let mut dock = DockState::new();
        let left = dock.leaf(vec!["Hierarchy"]);
        let right = dock.leaf(vec!["Scene View"]);
        let root = dock.split(Axis::X, 0.5, left, right);
        dock.set_root(SurfaceId::MAIN, root);
        dock.set_surface_frame(SurfaceId::MAIN, Vec2::ZERO, 1.0);
        let main = Win { ui: Ui::new(Theme::dark(), FONT).expect("font"), surface: SurfaceId::MAIN, size: MAIN_SIZE, drawn_tabs: 0, frames_built: 0 };
        let mut h = Self { dock, main, float: None, ask_dock };
        for _ in 0..5 {
            h.tick();
        }
        h
    }

    fn draw(dock: &mut DockState<&'static str>, w: &mut Win, ask_dock: bool) {
        let info = FrameInfo { screen_size: w.size, scale: 1.0, dt: 1.0 / 60.0 };
        let gate = w.ui.needs_frame_for(&info, 1.0 / 60.0) || (ask_dock && dock.needs_frame(w.surface));
        if !gate {
            return;
        }
        w.ui.begin_frame(info);
        dock.show(&mut w.ui, w.surface, &mut V);
        drop(w.ui.end_frame());
        w.drawn_tabs = dock.surface(w.surface).map_or(0, |s| s.tab_count());
        w.frames_built += 1;
    }

    /// One turn of the event loop, as the demo's `about_to_wait`.
    fn tick(&mut self) {
        self.dock.update();
        // A floating surface the dock wants shown gets a window.
        let wanted = self.dock.surfaces().iter().find(|s| s.id != SurfaceId::MAIN && s.visible).map(|s| s.id);
        match (wanted, &self.float) {
            (Some(id), None) => {
                self.dock.set_surface_frame(id, FLOAT_AT, 1.0);
                self.float = Some(Win { ui: Ui::new(Theme::dark(), FONT).expect("font"), surface: id, size: Vec2::new(400.0, 300.0), drawn_tabs: 0, frames_built: 0 });
            }
            (None, Some(_)) => self.float = None,
            _ => {}
        }
        Self::draw(&mut self.dock, &mut self.main, self.ask_dock);
        if let Some(f) = self.float.as_mut() {
            Self::draw(&mut self.dock, f, self.ask_dock);
        }
    }

    /// The pointer at `screen`, its events to the window under it only.
    fn pointer(&mut self, screen: Vec2, down: bool, in_float: bool) {
        self.dock.set_pointer(screen, down);
        let (w, origin) = match (in_float, self.float.as_mut()) {
            (true, Some(f)) => (f, FLOAT_AT),
            _ => (&mut self.main, Vec2::ZERO),
        };
        w.ui.push(InputEvent::PointerMoved { pos: screen - origin });
    }

    fn button(&mut self, down: bool, in_float: bool) {
        self.dock.set_pointer_down(down);
        let w = match (in_float, self.float.as_mut()) {
            (true, Some(f)) => f,
            _ => &mut self.main,
        };
        w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: down });
    }

    /// Tear "Scene View" off into its own window and let everything settle.
    fn tear_off(&mut self) {
        let grab = Vec2::new(460.0, 8.0); // the right panel's tab
        self.pointer(grab, false, false);
        self.tick();
        self.button(true, false);
        self.tick();
        for i in 1..=12 {
            let p = grab + (FLOAT_AT + Vec2::new(60.0, 8.0) - grab) * (i as f32 / 12.0);
            self.pointer(p, true, false);
            self.tick();
        }
        self.button(false, false);
        self.tick();
        assert!(self.float.is_some(), "the tab did not tear off into a window");
        for _ in 0..120 {
            self.tick(); // every animation settles; the windows go idle
        }
    }
}

fn redock(ask_dock: bool) -> Host {
    let mut h = Host::new(ask_dock);
    h.tear_off();
    assert_eq!(h.main.drawn_tabs, 1, "the main window does not show the tab gone");

    // Press the floating window's tab and drag the window back over the main
    // window's left panel. Every event goes to the floating window: it is
    // under the pointer the whole way.
    let start = FLOAT_AT + Vec2::new(60.0, 8.0);
    h.pointer(start, false, true);
    h.tick();
    h.button(true, true);
    h.tick();
    let over = Vec2::new(200.0, 300.0);
    for i in 1..=12 {
        let p = start + (over - start) * (i as f32 / 12.0);
        h.pointer(p, true, true);
        h.tick();
    }
    assert!(h.dock.drop_target().is_some(), "the drag never found the main window");
    h.button(false, true);
    for _ in 0..3 {
        h.tick();
    }
    h
}

#[test]
fn a_tab_dragged_back_shows_in_the_main_window_at_once() {
    let h = redock(true);
    assert_eq!(h.dock.surface(SurfaceId::MAIN).unwrap().tab_count(), 2, "the dock did not take the tab back");
    assert!(h.float.is_none(), "the floating window outlived its last tab");
    assert_eq!(h.main.drawn_tabs, 2, "the main window still shows the tab missing, three frames after the drop");
}

/// The control: gated on `needs_frame` alone, as the demos were, the main
/// window never shows the tab — it saw no input to make it look.
#[test]
fn without_asking_the_dock_the_main_window_stays_stale() {
    let mut h = redock(false);
    assert_eq!(h.dock.surface(SurfaceId::MAIN).unwrap().tab_count(), 2);
    for _ in 0..60 {
        h.tick();
    }
    assert_eq!(h.main.drawn_tabs, 1, "the report reproduces: the drop is not shown until something sends the main window input");
}

/// And the fix costs nothing at rest: once everything has drawn the dock as it
/// is, no window is rebuilt for the dock's sake.
#[test]
fn an_idle_dock_asks_for_nothing() {
    let mut h = redock(true);
    for _ in 0..120 {
        h.tick();
    }
    let main = h.main.frames_built;
    for _ in 0..60 {
        h.tick();
    }
    assert_eq!(h.main.frames_built, main, "an idle main window was rebuilt for the dock");
    assert!(!h.dock.needs_frame(SurfaceId::MAIN));
}
