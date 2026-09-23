//! Flat `#[repr(C)]` mirrors of the types that cross the boundary.
//!
//! These are mirrors rather than the Rust structs themselves because some of
//! those are not FFI-safe: `Response::raw_delta` is an `Option<Vec2>`, which
//! has no defined C layout. A mirror is the honest way to cross, and it is
//! also exactly where ABI bugs hide — a field added to one side and not the
//! other changes a struct's size and every read after it is garbage. So each
//! one has a `libgui_sizeof_*` the C side asserts against.

use libgui::{Rect, Response, TextResponse, TreeResponse, Vec2};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiVec2 {
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Mirrors [`libgui::Modifiers`]. `bool` is one byte and FFI-safe in Rust, but
/// spelled `uint8_t` in the header so a C++ compiler with a different `bool`
/// cannot disagree.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibguiModifiers {
    pub shift: u8,
    pub ctrl: u8,
    pub alt: u8,
    pub logo: u8,
}

/// Mirrors [`libgui::Response`].
///
/// `raw_delta` is flattened into a value and a `has_raw_delta` flag, because
/// `Option<Vec2>` has no C layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiResponse {
    pub id: u64,
    pub rect: LibguiRect,
    pub hovered: u8,
    pub focused: u8,
    pub active: u8,
    pub pressed: u8,
    pub clicked: u8,
    pub double_clicked: u8,
    pub secondary_pressed: u8,
    pub middle_pressed: u8,
    pub has_raw_delta: u8,
    pub _pad: [u8; 7],
    pub drag_delta: LibguiVec2,
    pub raw_delta: LibguiVec2,
    pub scroll: LibguiVec2,
    pub mouse_pos: LibguiVec2,
    pub pinch: f32,
    pub _pad2: f32,
    pub modifiers: LibguiModifiers,
}

/// Mirrors [`libgui::TreeResponse`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiTreeResponse {
    pub response: LibguiResponse,
    pub toggled: u8,
    pub _pad: [u8; 7],
}

/// Mirrors [`libgui::TextResponse`]. The caret is a line and a column; the
/// selection is a byte range into the same string the caller passed in.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiTextResponse {
    pub response: LibguiResponse,
    pub changed: u8,
    pub submitted: u8,
    pub can_undo: u8,
    pub can_redo: u8,
    pub _pad: [u8; 4],
    pub caret_line: u64,
    pub caret_column: u64,
    pub selection_start: u64,
    pub selection_end: u64,
}

impl From<Vec2> for LibguiVec2 {
    fn from(v: Vec2) -> Self {
        Self { x: v.x, y: v.y }
    }
}

impl From<Rect> for LibguiRect {
    fn from(r: Rect) -> Self {
        Self { x: r.x, y: r.y, w: r.w, h: r.h }
    }
}

fn b(v: bool) -> u8 {
    v as u8
}

impl From<Response> for LibguiResponse {
    fn from(r: Response) -> Self {
        Self {
            id: r.id.0,
            rect: r.rect.into(),
            hovered: b(r.hovered),
            focused: b(r.focused),
            active: b(r.active),
            pressed: b(r.pressed),
            clicked: b(r.clicked),
            double_clicked: b(r.double_clicked),
            secondary_pressed: b(r.secondary_pressed),
            middle_pressed: b(r.middle_pressed),
            has_raw_delta: b(r.raw_delta.is_some()),
            _pad: [0; 7],
            drag_delta: r.drag_delta.into(),
            raw_delta: r.raw_delta.unwrap_or(Vec2::ZERO).into(),
            scroll: r.scroll.into(),
            mouse_pos: r.mouse_pos.into(),
            pinch: r.pinch,
            _pad2: 0.0,
            modifiers: LibguiModifiers {
                shift: b(r.modifiers.shift),
                ctrl: b(r.modifiers.ctrl),
                alt: b(r.modifiers.alt),
                logo: b(r.modifiers.logo),
            },
        }
    }
}

impl From<TreeResponse> for LibguiTreeResponse {
    fn from(r: TreeResponse) -> Self {
        Self { response: r.response.into(), toggled: b(r.toggled), _pad: [0; 7] }
    }
}

impl From<TextResponse> for LibguiTextResponse {
    fn from(r: TextResponse) -> Self {
        Self {
            response: Default::default(),
            changed: b(r.changed),
            submitted: b(r.submitted),
            can_undo: b(r.can_undo),
            can_redo: b(r.can_redo),
            _pad: [0; 4],
            caret_line: r.caret.0 as u64,
            caret_column: r.caret.1 as u64,
            selection_start: r.selection.0 as u64,
            selection_end: r.selection.1 as u64,
        }
    }
}

macro_rules! sizeof_fns {
    ($($name:ident => $ty:ty),* $(,)?) => {
        $(
            /// Size of the struct as this library lays it out. The C side
            /// `static_assert`s its own `sizeof` against this, so a field added
            /// to one side and not the other fails to build rather than
            /// silently reading the wrong bytes.
            #[no_mangle]
            pub extern "C" fn $name() -> u64 {
                std::mem::size_of::<$ty>() as u64
            }
        )*
    };
}

sizeof_fns! {
    libgui_sizeof_response => LibguiResponse,
    libgui_sizeof_tree_response => LibguiTreeResponse,
    libgui_sizeof_text_response => LibguiTextResponse,
    libgui_sizeof_vec2 => LibguiVec2,
    libgui_sizeof_rect => LibguiRect,
    libgui_sizeof_color => LibguiColor,
    libgui_sizeof_modifiers => LibguiModifiers,
    libgui_sizeof_batch => crate::frame::LibguiBatch,
    libgui_sizeof_globals => crate::frame::LibguiGlobals,
    libgui_sizeof_platform_output => crate::frame::LibguiPlatformOutput,
    libgui_sizeof_surface => crate::dock::LibguiSurface,
    libgui_sizeof_insets => crate::dock::LibguiInsets,
    libgui_sizeof_table_response => crate::table_c::LibguiTableResponse,
    libgui_sizeof_drop_zone => crate::theme_dnd::LibguiDropZone,
}
