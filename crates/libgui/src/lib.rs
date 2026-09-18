//! libgui: a hybrid immediate/retained UI core.
//!
//! You call widgets every frame (immediate API). Behind the scenes the library
//! keeps per-widget state keyed by stable [`Id`]s (retained internals), builds
//! a layout tree, solves it after the frame is built, and emits a
//! backend-agnostic [`DrawList`]. It never touches the GPU; see [`Backend`].

mod backend;
mod dock;
mod draw;
mod hash;
mod id;
mod input;
mod input_state;
mod layout;
mod math;
mod painter;
pub mod render_contract;
mod text;
mod theme;
#[cfg(feature = "theme-toml")]
mod theme_file;
#[cfg(feature = "theme-watch")]
mod theme_watch;
mod ui;
mod text_edit;
mod widgets;

pub use backend::{Backend, Globals, INSTANCE_STRIDE, VERTICES_PER_INSTANCE};
pub use dock::{DockConfig, FloatingMode, DockNode, DockState, DropKind, DropTarget, Side, Surface, SurfaceId, TabViewer};
pub use draw::{Batch, DrawList, Instance, TextureId};
pub use id::{Id, StableHasher};
pub use input::{
    Cursor, FrameInfo, FrameInput, Gesture, InputEvent, Key, Modifiers, PlatformOutput, PointerButton, PointerKind, Shortcut, Touch,
    TouchPhase, VirtualCursor, WheelUnit,
};
pub use layout::{Align, Axis, Insets, Layout, Size};
pub use math::{Color, Rect, Vec2};
pub use painter::Painter;
pub use text::{Atlas, FontError, FontId, Fonts};
pub use theme::{
    ButtonStyle, Density, DropPreviewStyle, Metrics, Palette, PanelStyle, PlotStyle, ScrollbarStyle, SegmentedStyle,
    SelectableStyle, Shadow, SliderStyle, SplitterStyle, StateColors, TabStyle, TextInputStyle, Theme, ToggleStyle,
    ViewportStyle,
};
#[cfg(feature = "theme-toml")]
pub use theme_file::ThemeError;
#[cfg(feature = "theme-watch")]
pub use theme_watch::ThemeWatcher;
pub use text_edit::TextResponse;
pub use ui::{Frame, FrameOutput, LeafOptions, ListOptions, Response, ScrollOptions, Ui};
pub use widgets::{Branch, TreeResponse};
