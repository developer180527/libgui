//! The editor body as a dock tree: four panels, each a leaf whose tab strip is
//! the row of tabs the real tool wears ("Effect Controls", "Audio Clip Mixer",
//! …). They are real dock tabs, so they switch, reorder, move between panels
//! and tear off into their own window for free.

use libgui::*;

use crate::{effects, program, project, timeline, Editor};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Source,
    EffectControls,
    AudioClipMixer,
    Metadata,
    Program,
    Project,
    MediaBrowser,
    Libraries,
    Info,
    Timeline,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Source => "Source: (no clips)",
            Tab::EffectControls => "Effect Controls",
            Tab::AudioClipMixer => "Audio Clip Mixer: Sequence 01",
            Tab::Metadata => "Metadata",
            Tab::Program => "Program: Sequence 01",
            Tab::Project => "Project: Hiking",
            Tab::MediaBrowser => "Media Browser",
            Tab::Libraries => "Libraries",
            Tab::Info => "Info",
            Tab::Timeline => "Sequence 01",
        }
    }

    /// Identity in a saved layout: a name, not a discriminant, so adding a
    /// panel later does not reshuffle everyone's workspace.
    pub fn key(self) -> u64 {
        let name = match self {
            Tab::Source => "source",
            Tab::EffectControls => "effect-controls",
            Tab::AudioClipMixer => "audio-clip-mixer",
            Tab::Metadata => "metadata",
            Tab::Program => "program",
            Tab::Project => "project",
            Tab::MediaBrowser => "media-browser",
            Tab::Libraries => "libraries",
            Tab::Info => "info",
            Tab::Timeline => "timeline",
        };
        Id::from_name(name).0
    }
}

/// The monitors and the timeline place their content to the pixel; the rest
/// are happier in a scroll area.
fn scrolls(tab: Tab) -> bool {
    matches!(tab, Tab::Metadata | Tab::Libraries | Tab::Info | Tab::MediaBrowser)
}

pub struct Viewer<'a> {
    pub ed: &'a mut Editor,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn title(&self, tab: &Tab) -> String {
        tab.title().to_string()
    }

    fn id(&self, tab: &Tab) -> u64 {
        tab.key()
    }

    fn scroll(&self, tab: &Tab) -> bool {
        scrolls(*tab)
    }

    fn padding(&self, tab: &Tab) -> Insets {
        match tab {
            Tab::Program | Tab::Timeline | Tab::EffectControls | Tab::Project => Insets::all(0.0),
            _ => Insets::all(10.0),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::EffectControls => effects::panel(ui, self.ed),
            Tab::Program => program::panel(ui, self.ed),
            Tab::Project => project::panel(ui, self.ed),
            Tab::Timeline => timeline::panel(ui, self.ed),
            other => placeholder(ui, other.title()),
        }
    }
}

/// The panels this demo does not draw: a name, so a tab that is dragged
/// somewhere still shows something honest.
fn placeholder(ui: &mut Ui, title: &str) {
    let t = ui.theme.clone();
    ui.container(
        Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).align(Align::Center, Align::Center),
        Frame::none(),
        |ui| {
            ui.text_with(title, t.metrics.font_size, t.palette.text_faint);
        },
    );
}

/// Two monitors on top, bin and timeline below — the layout every NLE opens
/// with.
pub fn initial() -> DockState<Tab> {
    let mut dock = DockState::new();
    let mut top_left = dock.leaf(vec![Tab::Source, Tab::EffectControls, Tab::AudioClipMixer, Tab::Metadata]);
    if let DockNode::Leaf(l) = &mut top_left {
        l.active = 1;
    }
    let top_right = dock.leaf(vec![Tab::Program]);
    let top = dock.split(Axis::X, 0.505, top_left, top_right);

    let bin = dock.leaf(vec![Tab::Project, Tab::MediaBrowser, Tab::Libraries, Tab::Info]);
    let seq = dock.leaf(vec![Tab::Timeline]);
    let bottom = dock.split(Axis::X, 0.325, bin, seq);

    let root = dock.split(Axis::Y, 0.615, top, bottom);
    dock.set_root(SurfaceId::MAIN, root);
    dock
}

/// Every panel this build offers, for restoring a saved layout.
pub const ALL: [Tab; 10] = [
    Tab::Source,
    Tab::EffectControls,
    Tab::AudioClipMixer,
    Tab::Metadata,
    Tab::Program,
    Tab::Project,
    Tab::MediaBrowser,
    Tab::Libraries,
    Tab::Info,
    Tab::Timeline,
];

pub fn from_key(key: u64) -> Option<Tab> {
    ALL.iter().copied().find(|t| t.key() == key)
}
