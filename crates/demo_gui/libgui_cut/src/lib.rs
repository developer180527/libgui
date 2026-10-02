//! A third libgui demo: a non-linear video editor.
//!
//! The first demo shows features one at a time, the second shows density. This
//! one is shaped like the tool people actually spend their day in: two
//! monitors, a project bin, an effect-controls tree, and a **timeline** —
//! which is the interesting part, because a timeline is the widget no UI
//! library ships. It is a custom surface with its own zoom, its own scroll in
//! two directions, clips that drag between tracks, and a ruler and track
//! headers that have to stay glued to it.
//!
//! Everything here is app code drawing through `Painter` and `interact`. The
//! library supplies ids, layout, input, the dock and the theme; it knows
//! nothing about clips.

use libgui::*;

pub mod theme;

mod effects;
mod program;
mod project;
mod state;
mod timeline;
mod topbar;
mod widgets;

pub mod dock;

pub use dock::Tab;
pub use libgui::FrameText;
pub use state::{Clip, Editor, Media, Sequence, Track, TrackKind, BAR, HEADER_W, RULER_H};
pub use theme::theme;

/// The editor: the panels' state, and the dock that arranges them.
///
/// Two fields rather than one because `DockState::show` borrows the dock
/// mutably while the panels it calls need everything else mutably too.
pub struct App {
    pub ed: Editor,
    pub dock: DockState<Tab>,
}

impl Default for App {
    fn default() -> Self {
        Self { ed: Editor::default(), dock: dock::initial() }
    }
}

impl App {
    /// One frame, for one dock surface. A torn-off panel in its own window
    /// draws through here too, with its own id.
    pub fn ui_for(&mut self, ui: &mut Ui, surface: SurfaceId) {
        let t = ui.theme.clone();
        let root = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container(root, Frame { fill: t.palette.bg_app, clip: true, ..Frame::none() }, |ui| {
            let main = surface == SurfaceId::MAIN;
            if main {
                topbar::bar(ui, &mut self.ed);
            }
            let mut viewer = dock::Viewer { ed: &mut self.ed };
            self.dock.show(ui, surface, &mut viewer);
        });
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.ui_for(ui, SurfaceId::MAIN);
    }
}
