//! Docking: panels, tabs, and tearing one off into a real OS window.
//!
//! Three things had to be solved to get this across.
//!
//! **`DockState<T>` is generic.** Here `T` is `u64`: a tab *is* its own id,
//! and the app maps that to its own data. Nothing is lost — `TabViewer::id`
//! already required a stable `u64` per tab, and saved layouts already store
//! exactly that.
//!
//! **`TabViewer` is a trait.** It becomes a vtable of function pointers and a
//! `void* user`. `title` returns a `String` in Rust and no allocation crosses
//! here, so it inverts: the callback is handed a buffer and writes into it.
//!
//! **Re-entrancy is the real problem.** `show` holds `&mut Ui` and walks the
//! tree recursively; the panel body runs four closures deep with that borrow
//! live. So the callback fires *while libgui holds the borrow*, and
//! immediately wants to draw. Handing it a second handle would mean two `&mut`
//! to one `Ui`, which is undefined behaviour.
//!
//! Instead the callback receives **the host's own handle**, temporarily
//! pointed at the borrow libgui gave us ([`crate::handle::Borrowed`]). Nested
//! calls then reborrow through that one live borrow rather than starting a
//! second, which is what an ordinary Rust callback does — C just needs it
//! spelled out. Nothing can dangle, because no new pointer exists, and a
//! `depth` counter turns the dangerous calls (freeing the handle, ending the
//! frame, docking re-entrantly) into refusals rather than corruption.

use crate::handle::{inside_callback, set_error, with_ui, Borrowed, LibguiUi};
use crate::types::LibguiVec2;
use libgui::{DockState, FloatingMode, Insets, SurfaceId, TabViewer, Ui, Vec2};
use std::os::raw::{c_char, c_void};

/// An opaque dock. One per application, shared by every window.
pub struct LibguiDock {
    state: DockState<u64>,
}

/// How the host draws panels. Every pointer may be null except `ui`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LibguiTabViewer {
    /// Write the tab's title into `buf`, NUL-terminated, at most `cap` bytes.
    pub title: Option<unsafe extern "C" fn(tab: u64, buf: *mut c_char, cap: u64, user: *mut c_void)>,
    /// Draw the panel. The `LibguiUi*` is the handle you already own; do not
    /// store it past this call, and do not end the frame or free it here.
    pub ui: Option<unsafe extern "C" fn(ui: *mut LibguiUi, tab: u64, user: *mut c_void)>,
    /// Wrap the panel in a scroll area. Null means yes; return 0 for a
    /// viewport, which scrolls itself.
    pub scroll: Option<unsafe extern "C" fn(tab: u64, user: *mut c_void) -> u8>,
    /// Padding inside the panel, in logical pixels. Null means 12 all round.
    pub padding: Option<unsafe extern "C" fn(tab: u64, out: *mut LibguiInsets, user: *mut c_void)>,
    pub user: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiInsets {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// One window the host should have. Mirrors `libgui::Surface`, whose
/// `window_pos` is an `Option<Vec2>` and has no C layout — so it flattens into
/// a flag and a value, the same way `LibguiResponse::raw_delta` does.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LibguiSurface {
    pub id: u64,
    /// 0 for the main window, 1 for one that was torn off.
    pub floating: u8,
    /// **Read this before creating a window.** A tab dragged out of its bar
    /// becomes a hidden floating surface immediately, so that dropping it into
    /// another panel never builds and destroys a window, a `Ui` and a renderer
    /// between the drop and the frame that shows the result. Only a tab
    /// dragged onto the desktop becomes visible.
    pub visible: u8,
    /// `window_pos` is meaningful: libgui is moving this window.
    pub has_window_pos: u8,
    pub _pad: u8,
    /// Requested inner top-left, physical screen pixels.
    pub window_pos: LibguiVec2,
    /// Initial inner size, logical pixels.
    pub window_size: LibguiVec2,
    /// In-app floating placement, in the main window's logical coordinates.
    pub rect_x: f32,
    pub rect_y: f32,
    pub rect_w: f32,
    pub rect_h: f32,
    /// How many tabs it holds, across every pane.
    pub tab_count: u64,
}

/// The adapter that turns the vtable into something libgui can call.
struct Viewer {
    v: LibguiTabViewer,
    /// The host's handle, re-pointed for the duration of each callback.
    handle: *mut LibguiUi,
}

impl TabViewer for Viewer {
    type Tab = u64;

    fn title(&self, tab: &u64) -> String {
        let Some(f) = self.v.title else { return String::new() };
        let mut buf = [0u8; 256];
        unsafe { f(*tab, buf.as_mut_ptr() as *mut c_char, buf.len() as u64, self.v.user) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }

    fn id(&self, tab: &u64) -> u64 {
        // A tab is its own id, which is what makes the generic concrete.
        *tab
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut u64) {
        let Some(f) = self.v.ui else { return };
        // The whole point: the handle points at *this* borrow while the
        // callback runs, so everything it calls reborrows through it.
        let _guard = Borrowed::new(self.handle, ui);
        unsafe { f(self.handle, *tab, self.v.user) };
    }

    fn scroll(&self, tab: &u64) -> bool {
        match self.v.scroll {
            Some(f) => (unsafe { f(*tab, self.v.user) }) != 0,
            None => true,
        }
    }

    fn padding(&self, tab: &u64) -> Insets {
        match self.v.padding {
            Some(f) => {
                let mut i = LibguiInsets { left: 12.0, right: 12.0, top: 12.0, bottom: 12.0 };
                unsafe { f(*tab, &mut i, self.v.user) };
                Insets { left: i.left, right: i.right, top: i.top, bottom: i.bottom }
            }
            None => Insets::all(12.0),
        }
    }
}

/// Create a dock. One per application; free it with [`libgui_dock_free`].
#[no_mangle]
pub extern "C" fn libgui_dock_new() -> *mut LibguiDock {
    Box::into_raw(Box::new(LibguiDock { state: DockState::new() }))
}

/// # Safety
/// `dock` must have come from [`libgui_dock_new`] and not be used again.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_free(dock: *mut LibguiDock) {
    if !dock.is_null() {
        drop(unsafe { Box::from_raw(dock) });
    }
}

fn with_dock<R>(dock: *mut LibguiDock, fallback: R, body: impl FnOnce(&mut DockState<u64>) -> R) -> R {
    match unsafe { dock.as_mut() } {
        Some(d) => body(&mut d.state),
        None => {
            set_error("null dock handle");
            fallback
        }
    }
}

/// Panels float inside the main window instead of becoming OS windows. For a
/// tablet, a console, or any host that cannot make windows — the same layout
/// and the same panel code work either way.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_set_in_app_floating(dock: *mut LibguiDock, in_app: u8) {
    with_dock(dock, (), |d| {
        d.config.floating_mode = if in_app != 0 { FloatingMode::InApp } else { FloatingMode::OsWindows };
    });
}

/// Build a leaf holding one tab, and return a handle to it for
/// [`libgui_dock_split`] or [`libgui_dock_set_root`].
///
/// Nodes are held by the dock while a layout is being built; the number is an
/// index into that scratch list, not something to keep.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_leaf(dock: *mut LibguiDock, tab: u64) -> u64 {
    with_dock(dock, u64::MAX, |d| {
        let node = d.leaf(vec![tab]);
        crate::dock_build::push(node)
    })
}

/// Split two nodes. `axis` is 0 for side by side, 1 for stacked; `fraction` is
/// the first one's share, 0..1.
///
/// # Safety
/// `dock` must be null or live; `first` and `second` must come from
/// [`libgui_dock_leaf`] or a previous split, and are consumed.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_split(
    dock: *mut LibguiDock,
    axis: u32,
    fraction: f32,
    first: u64,
    second: u64,
) -> u64 {
    with_dock(dock, u64::MAX, |d| {
        let (Some(a), Some(b)) = (crate::dock_build::take(first), crate::dock_build::take(second)) else {
            set_error("libgui_dock_split: a node handle was already used");
            return u64::MAX;
        };
        let axis = if axis == 1 { libgui::Axis::Y } else { libgui::Axis::X };
        let node = d.split(axis, fraction, a, b);
        crate::dock_build::push(node)
    })
}

/// Make `node` the root of a surface. `surface` is 0 for the main window.
///
/// # Safety
/// As above; `node` is consumed.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_set_root(dock: *mut LibguiDock, surface: u64, node: u64) -> i32 {
    with_dock(dock, 1, |d| match crate::dock_build::take(node) {
        Some(n) => {
            d.set_root(SurfaceId(surface), n);
            0
        }
        None => {
            set_error("libgui_dock_set_root: a node handle was already used");
            1
        }
    })
}

/// Open a tab into an existing window.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_add_tab(dock: *mut LibguiDock, surface: u64, tab: u64) {
    with_dock(dock, (), |d| d.add_tab(SurfaceId(surface), tab));
}

/// Where the pointer is on the **desktop**, in physical screen pixels, and
/// whether the primary button is down.
///
/// Global rather than per-window because a tab being dragged between windows
/// is not inside any one window's coordinate space.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_set_pointer(dock: *mut LibguiDock, x: f32, y: f32, down: u8) {
    with_dock(dock, (), |d| d.set_pointer(Vec2::new(x, y), down != 0));
}

/// Where a window is: the **inner** (client area) top-left in physical screen
/// pixels, and its DPI scale. Not the outer frame — getting that wrong drops
/// tabs exactly one title bar away from where they were aimed.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_set_surface_frame(
    dock: *mut LibguiDock,
    surface: u64,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
) {
    with_dock(dock, (), |d| d.set_surface_frame(SurfaceId(surface), Vec2::new(origin_x, origin_y), scale));
}

/// Run the drag state machine. Call once per loop iteration, after the pointer
/// and the window frames are reported.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_update(dock: *mut LibguiDock) {
    with_dock(dock, (), |d| d.update());
}

/// The user closed a window.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_close_surface(dock: *mut LibguiDock, surface: u64) {
    with_dock(dock, (), |d| d.close_surface(SurfaceId(surface)));
}

/// Is a tab being dragged? Useful for the host's own cursor or feedback.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_is_dragging(dock: *mut LibguiDock) -> u8 {
    with_dock(dock, 0, |d| d.is_dragging() as u8)
}

/// Abandon a drag — what Escape should do.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_cancel_drag(dock: *mut LibguiDock) {
    with_dock(dock, (), |d| d.cancel_drag());
}

/// How many windows the host should have.
///
/// # Safety
/// `dock` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_surface_count(dock: *mut LibguiDock) -> u64 {
    with_dock(dock, 0, |d| d.surfaces().len() as u64)
}

/// Describe the `i`th window. Returns 0 on success, 1 if `i` is out of range.
///
/// # Safety
/// `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_surface_at(dock: *mut LibguiDock, i: u64, out: *mut LibguiSurface) -> i32 {
    with_dock(dock, 1, |d| {
        let Some(s) = d.surfaces().get(i as usize) else {
            return 1;
        };
        let v = LibguiSurface {
            id: s.id.0,
            floating: s.floating as u8,
            visible: s.visible as u8,
            has_window_pos: s.window_pos.is_some() as u8,
            _pad: 0,
            window_pos: s.window_pos.unwrap_or(Vec2::ZERO).into(),
            window_size: s.window_size.into(),
            rect_x: s.rect.x,
            rect_y: s.rect.y,
            rect_w: s.rect.w,
            rect_h: s.rect.h,
            tab_count: s.tab_count() as u64,
        };
        if let Some(slot) = unsafe { out.as_mut() } {
            *slot = v;
        }
        0
    })
}

/// Draw one window's panels. Calls the viewer's `ui` once per visible panel.
///
/// # Safety
/// `ui` and `dock` must be null or live; `viewer`'s function pointers must be
/// null or callable.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_show(
    dock: *mut LibguiDock,
    ui: *mut LibguiUi,
    surface: u64,
    viewer: *const LibguiTabViewer,
) {
    if inside_callback(ui, "libgui_dock_show") {
        return;
    }
    let Some(v) = (unsafe { viewer.as_ref() }) else {
        set_error("libgui_dock_show: null viewer");
        return;
    };
    let v = *v;
    let dock_ptr = dock;
    with_ui(ui, (), move |u| {
        let mut adapter = Viewer { v, handle: ui };
        if let Some(d) = unsafe { dock_ptr.as_mut() } {
            d.state.show(u, SurfaceId(surface), &mut adapter);
        }
    });
}

/// Write the layout as TOML into a buffer the caller owns.
///
/// `snprintf`'s contract: returns the length needed, not the length written.
/// Call with `cap` 0 to size the buffer, allocate, call again. Returns -1 if
/// the layout could not be rendered — which matters, because the usual next
/// step is writing over the file holding the last good one.
///
/// # Safety
/// `buf` must be null or point to `cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_layout_to_toml(dock: *mut LibguiDock, buf: *mut c_char, cap: u64) -> i64 {
    with_dock(dock, -1, |d| {
        // The viewer is only consulted for tab ids, and a tab *is* its id.
        struct Ids;
        impl TabViewer for Ids {
            type Tab = u64;
            fn title(&self, _: &u64) -> String {
                String::new()
            }
            fn id(&self, tab: &u64) -> u64 {
                *tab
            }
            fn ui(&mut self, _: &mut Ui, _: &mut u64) {}
        }
        let Ok(text) = d.layout(&Ids).to_toml() else {
            set_error("libgui_dock_layout_to_toml: could not render the layout");
            return -1;
        };
        let needed = text.len() as i64;
        if !buf.is_null() && cap > 0 {
            let room = (cap - 1) as usize;
            let mut end = room.min(text.len());
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr() as *const c_char, buf, end);
                *buf.add(end) = 0;
            }
        }
        needed
    })
}

/// Rebuild a layout from TOML. Returns 0 on success, 1 on failure — a layout
/// from a newer version of the library is refused rather than guessed at.
///
/// Tabs the layout names but the app no longer has are dropped, and any pane
/// or window left empty goes with them.
///
/// # Safety
/// `toml` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_dock_restore_from_toml(dock: *mut LibguiDock, toml: *const c_char) -> i32 {
    let Some(text) = (unsafe { crate::convert::str_from(toml) }) else {
        set_error("libgui_dock_restore_from_toml: null or not UTF-8");
        return 1;
    };
    with_dock(dock, 1, |d| {
        let layout = match libgui::DockLayout::from_toml(text) {
            Ok(l) => l,
            Err(e) => {
                set_error(&format!("libgui_dock_restore_from_toml: {e}"));
                return 1;
            }
        };
        // Every id in the file is a tab, because a tab is its id.
        match d.restore(&layout, Some) {
            Ok(_) => 0,
            Err(e) => {
                set_error(&format!("libgui_dock_restore_from_toml: {e}"));
                1
            }
        }
    })
}
