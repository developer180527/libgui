//! Every exported function, called at least once.
//!
//! A C ABI is a promise about bytes, and an export nothing calls is a promise
//! nobody has checked. These are not deep behavioural tests — `abi.rs` and the
//! Rust suite do that — they are the guarantee that each entry point is
//! reachable, tolerates what C hands it, and leaves the handle usable.
//!
//! They are in Rust so they also run under Miri, which is the only way the
//! aliasing in the callback paths — a table's cells, a dock panel, a paint
//! closure, all of which hand a `LibguiUi*` back into a live `&mut Ui` — is
//! checked by a machine rather than by reading.

#![allow(unused_unsafe)]

use libgui_c::*;
use std::ffi::CString;
use std::os::raw::c_void;

/// The header's `#define`s, which do not cross into Rust. Spelled out here so
/// a change to either side shows up as a failing test rather than silence.
const KEY_A: u32 = 100; // A..Z are 100..125
const KEY_S: u32 = KEY_A + 18;
const PLATFORM_MAC: i32 = 0;
const SURFACE_MAIN: u64 = 0;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

unsafe fn ui() -> *mut LibguiUi {
    let p = unsafe { libgui_ui_new(FONT.as_ptr(), FONT.len() as u64) };
    assert!(!p.is_null());
    p
}

/// A fixed-size leaf, which several tests want.
fn leaf_layout(w: f32, h: f32) -> LibguiLayout {
    LibguiLayout {
        axis: 0,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Fixed, value: w },
        height: LibguiSize { kind: LibguiSizeKind::Fixed, value: h },
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        gap: 0.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    }
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn frame(ui: *mut LibguiUi, body: impl FnOnce()) {
    unsafe {
        libgui_begin_frame(ui, 400.0, 300.0, 1.0, 1.0 / 60.0);
        body();
        libgui_end_frame(ui);
    }
    assert_eq!(unsafe { libgui_ui_poisoned(ui) }, 0, "a call poisoned the handle");
}

/// The widgets that had no caller. One frame, every one of them, and the
/// handle still good at the end.
#[test]
fn every_widget_is_callable() {
    unsafe {
        let u = ui();
        let mut on: u8 = 1;
        let mut value: f32 = 0.5;
        let mut sel: u64 = 0;
        let opts = [c("One"), c("Two")];
        let ptrs: Vec<*const std::os::raw::c_char> = opts.iter().map(|o| o.as_ptr()).collect();

        frame(u, || {
            libgui_section(u, c("Section").as_ptr());
            libgui_label_muted(u, c("Muted").as_ptr());
            libgui_paragraph(u, c("A paragraph that wraps.").as_ptr());
            libgui_separator(u);
            libgui_space(u, 4.0);
            libgui_flex(u);
            libgui_progress(u, c("Work").as_ptr(), 0.3);
            libgui_progress(u, c("Spinner").as_ptr(), -1.0);
            let _ = libgui_button_primary(u, c("Go").as_ptr());
            let _ = libgui_button_keyed(u, 7, c("Row").as_ptr());
            let _ = libgui_toggle(u, c("On").as_ptr(), &mut on);
            let _ = libgui_drag_value(u, c("X").as_ptr(), &mut value, 0.01);
            let _ = libgui_slider_vertical(u, c("V").as_ptr(), &mut value, 0.0, 1.0, 60.0);
            let _ = libgui_selectable_keyed(u, 3, c("Item").as_ptr(), 1);
            let _ = libgui_combo(u, c("Mode").as_ptr(), &mut sel, ptrs.as_ptr(), ptrs.len() as u64);
            let _ = libgui_segmented(u, c("seg").as_ptr(), &mut sel, ptrs.as_ptr(), ptrs.len() as u64);
            let _ = libgui_tree_row(u, 1, 0, 1, c("Branch").as_ptr(), 0);

            // Text area over a caller-owned buffer, the way C owns strings.
            let mut buf = vec![0u8; 64];
            buf[..5].copy_from_slice(b"notes");
            let mut len: u64 = 0;
            let _ = libgui_text_area(u, c("notes").as_ptr(), buf.as_mut_ptr().cast(), 64, 3, &mut len);
            assert_eq!(len, 5, "the text area reported the wrong length");

            // Containers that open and close.
            libgui_open_scroll_area(u, c("scroll").as_ptr());
            libgui_label(u, c("inside").as_ptr());
            libgui_close_scroll_area(u);

            // A collection and its cursor.
            let col = libgui_open_collection(u, c("list").as_ptr(), 4);
            libgui_set_cursor(u, col, 2);
            let _ = libgui_nav_cursor();
            let _ = libgui_nav_focused();
            let _ = libgui_nav_moved();
            let _ = libgui_nav_activated();
            let _ = libgui_nav_expand();
            let _ = libgui_nav_collapse();
            libgui_close_collection(u);

            // Menus.
            if libgui_open_menu(u, c("File").as_ptr()) != 0 {
                let _ = libgui_menu_item(u, c("New").as_ptr());
                let _ = libgui_menu_item_shortcut(u, c("Open").as_ptr(), c("Ctrl+O").as_ptr());
                let _ = libgui_menu_item_ex(u, c("Save").as_ptr(), c("Ctrl+S").as_ptr(), 0);
                libgui_menu_separator(u);
                libgui_close_menu(u);
            }

            let id = libgui_id_from_name(c("thing").as_ptr());
            let _ = libgui_interact(u, id);
            libgui_tooltip(u, id, c("What it does").as_ptr());
            if libgui_open_context_menu(u, id) != 0 {
                libgui_close_menu(u);
            }
            libgui_scroll_to(u, id);
        });

        let _ = libgui_needs_frame(u, 1.0 / 60.0);
        let mut clear = LibguiColor::default();
        libgui_frame_clear_color(u, &mut clear);
        assert!(clear.a > 0.0, "the clear colour is transparent");
        let _ = libgui_frame_copied_text(u);
        assert_eq!(libgui_set_theme(u, c("light").as_ptr()), 0);
        assert_ne!(libgui_set_theme(u, c("nonsense").as_ptr()), 0, "an unknown theme should be refused");
        libgui_ui_free(u);
    }
}

/// Every input a host can push, including the ones only a tablet sends.
#[test]
fn every_input_is_accepted() {
    unsafe {
        let u = ui();
        frame(u, || libgui_label(u, c("x").as_ptr()));

        libgui_push_modifiers(u, LibguiModifiers { shift: 1, ctrl: 0, alt: 0, logo: 0 });
        libgui_push_key(u, KEY_A, 1, 0);
        libgui_push_key(u, KEY_A, 0, 0);
        libgui_push_wheel(u, 0.0, -3.0, 0);
        libgui_push_pointer_delta(u, 1.0, 2.0);
        libgui_push_pointer_left(u);
        libgui_push_paste(u, c("pasted").as_ptr());
        libgui_push_ime_preedit(u, c("comp").as_ptr(), 2);
        // The iPad: down, moved, up.
        for phase in 0..3 {
            libgui_push_touch(u, 1, phase, 10.0, 20.0);
        }
        libgui_push_focus_lost(u);

        frame(u, || libgui_label(u, c("x").as_ptr()));
        libgui_ui_free(u);
    }
}

/// The keyboard convention, which libgui refuses to decide: a platform's
/// bindings are installed, and a chord is claimed by the app.
#[test]
fn the_keymap_and_shortcuts_work_from_c() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_MAC), 0);
        assert_ne!(libgui_install_keymap(u, 99), 0, "an unknown platform should be refused");

        // Cmd+S, the way a Mac sends it.
        let mods = LibguiModifiers { shift: 0, ctrl: 0, alt: 0, logo: 1 };
        libgui_push_modifiers(u, mods);
        libgui_push_key(u, KEY_S, 1, 0);
        let mut claimed = 0;
        frame(u, || claimed = libgui_consume_shortcut(u, KEY_S, mods));
        assert_eq!(claimed, 1, "the app did not get its own chord");

        // Multi-select: what a click with modifiers means is the platform's.
        let kind = libgui_select_kind(PLATFORM_MAC, LibguiModifiers { shift: 1, ctrl: 0, alt: 0, logo: 0 });
        assert_eq!(kind, 2, "shift+click on a Mac is a range select");
        let cmd = libgui_select_kind(PLATFORM_MAC, LibguiModifiers { shift: 0, ctrl: 0, alt: 0, logo: 1 });
        assert_eq!(cmd, 1, "Cmd+click on a Mac toggles");
        assert_eq!(libgui_select_kind(99, LibguiModifiers::default()), -1, "an unknown platform should be refused");
        let mut out_kind = -1;
        let (mut lo, mut hi) = (0u64, 0u64);
        frame(u, || {
            let col = libgui_open_collection(u, c("rows").as_ptr(), 8);
            libgui_set_cursor(u, col, 1);
            // A range extends from the anchor a plain click left, not from the
            // keyboard cursor: shift-dragging up and down a list grows and
            // shrinks one range instead of ratcheting a new one.
            libgui_select(u, col, 1, 0, &mut out_kind, &mut lo, &mut hi);
            assert_eq!((out_kind, lo, hi), (0, 1, 1), "a plain click selects only what was clicked");
            libgui_select(u, col, 5, kind, &mut out_kind, &mut lo, &mut hi);
            libgui_close_collection(u);
        });
        assert_eq!(out_kind, 2);
        assert_eq!((lo, hi), (1, 5), "the range ran from the anchor to the clicked row");
        libgui_ui_free(u);
    }
}

extern "C" fn paint_all(p: *mut LibguiPainter, r: LibguiRect, user: *mut c_void) {
    // A real pointer, not a made-up integer: an invalid one is exactly what
    // Miri cannot tell from a valid one, so a bogus value here would hide the
    // bug this test exists to find.
    let seen = unsafe { &mut *(user as *mut u32) };
    *seen += 1;
    unsafe {
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        libgui_painter_rect(p, r, white, 2.0);
        libgui_painter_rect_bordered(p, r, white, 2.0, 1.0, white);
        libgui_painter_line(p, r.x, r.y, r.x + r.w, r.y + r.h, 1.0, white);
        libgui_painter_text_left(p, r, 12.0, white, c("label").as_ptr());
        libgui_painter_image(p, r, 3, 2.0);
        libgui_painter_image_uv(p, r, 3, 0.0, 0.0, 0.5, 0.5, 0.0, white);
    }
}

/// Custom painting, including the two texture calls a 3D view needs.
#[test]
fn every_painter_call_reaches_the_frame() {
    unsafe {
        let u = ui();
        let mut painted: u32 = 0;
        frame(u, || {
            let layout = LibguiLayout {
                axis: 1,
                _pad: [0; 3],
                width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 80.0 },
                height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 40.0 },
                pad_left: 0.0,
                pad_right: 0.0,
                pad_top: 0.0,
                pad_bottom: 0.0,
                gap: 0.0,
                align_main: 0,
                align_cross: 0,
                _pad2: [0; 2],
            };
            libgui_add_leaf(
                u,
                libgui_id_from_name(c("custom").as_ptr()),
                layout,
                1,
                LibguiPaintFn { paint: Some(paint_all), drop_user: None, user: &mut painted as *mut u32 as *mut c_void },
            );
        });
        assert_eq!(painted, 1, "the paint callback did not run");
        libgui_ui_free(u);
    }
}

/// The 3D view: a viewport reaches the host as a batch naming its texture.
/// Without this, a C++ engine has a UI it cannot put its scene into.
#[test]
fn a_viewport_reaches_the_host_as_a_texture_batch() {
    unsafe {
        let u = ui();
        frame(u, || {
            libgui_label(u, c("above").as_ptr());
            let r = libgui_viewport(u, c("scene").as_ptr(), 7);
            // The response is what a camera is driven from.
            assert_eq!(r.hovered, 0);
        });

        let mut n = 0u64;
        let batches = libgui_frame_batches(u, &mut n);
        assert!(n > 0, "no batches");
        let batches = std::slice::from_raw_parts(batches, n as usize);
        let mine = batches.iter().find(|b| b.texture_kind == 1).expect("no user-texture batch: the viewport drew nothing");
        assert_eq!(mine.texture_index, 7, "the batch named a different texture");
        assert!(mine.count > 0, "the viewport's batch is empty");
        libgui_ui_free(u);
    }
}

/// The triangle form, for a renderer with no per-instance attributes.
#[test]
fn the_mesh_is_the_same_frame_expanded_into_quads() {
    unsafe {
        let u = ui();

        // Off by default: a renderer that can instance pays nothing for this.
        frame(u, || libgui_label(u, c("x").as_ptr()));
        let mut n = 1u64;
        assert!(libgui_mesh_vertices(u, &mut n).is_null() || n == 0, "the mesh was built without being asked for");

        libgui_enable_mesh(u, 1);
        frame(u, || {
            libgui_label(u, c("Hello").as_ptr());
            let _ = libgui_button(u, c("Go").as_ptr());
            let _ = libgui_viewport(u, c("scene").as_ptr(), 2);
        });

        let mut instances = 0u64;
        libgui_frame_instances(u, &mut instances);
        assert!(instances > 0);

        let (mut verts, mut idx) = (0u64, 0u64);
        let vp = libgui_mesh_vertices(u, &mut verts);
        let ip = libgui_mesh_indices(u, &mut idx);
        assert!(!vp.is_null() && !ip.is_null());
        // One quad per primitive: four vertices and six indices each.
        assert_eq!(verts, instances * 4, "a primitive did not become one quad");
        assert_eq!(idx, instances * 6);
        assert_eq!(libgui_mesh_fits_u16(u), 1, "a frame this small should fit 16-bit indices");

        // Every index addresses a vertex that exists.
        let indices = std::slice::from_raw_parts(ip, idx as usize);
        assert!(indices.iter().all(|&i| (i as u64) < verts), "an index left the buffer");

        // The batches partition the index buffer, in order and with no gaps,
        // and say the same things about textures as the instanced form.
        let (mut mn, mut fn_) = (0u64, 0u64);
        let mb = std::slice::from_raw_parts(libgui_mesh_batches(u, &mut mn), mn as usize);
        let fb = std::slice::from_raw_parts(libgui_frame_batches(u, &mut fn_), fn_ as usize);
        assert_eq!(mn, fn_, "the two forms disagree about how many draws there are");
        let mut at = 0;
        for (m, f) in mb.iter().zip(fb) {
            assert_eq!(m.first, at, "the mesh batches do not tile the index buffer");
            assert_eq!(m.count, f.count * 6, "a batch's instances did not become six indices each");
            assert_eq!((m.texture_kind, m.texture_index), (f.texture_kind, f.texture_index));
            at += m.count;
        }
        assert_eq!(at as u64, idx);

        // The layout is described rather than assumed, so a host does not
        // hard-code offsets that could move.
        // A vertex is wider than an instance: it carries what the shader's
        // varyings are, already computed. Derived rather than hard-coded, so
        // this says the description is consistent rather than restating it.
        let stride = libgui_vertex_stride();
        assert_ne!(stride, libgui_instance_stride(), "the mesh is not the instance");
        let count = libgui_vertex_attribute_count();
        assert!(count > 0);
        let (mut floats, mut offset) = (0u32, 0u32);
        assert_eq!(libgui_vertex_attribute(0, &mut floats, &mut offset), 1);
        assert_eq!((floats, offset), (2, 0), "the first attribute is the position");
        // The attributes tile the vertex exactly: no gap, no overlap, no slack.
        let mut at = 0u32;
        for i in 0..count {
            assert_eq!(libgui_vertex_attribute(i, &mut floats, &mut offset), 1);
            assert_eq!(offset, at, "attribute {i} does not follow the one before it");
            at += floats * 4;
        }
        assert_eq!(at as u64, stride, "the attributes do not add up to the stride");
        assert_eq!(libgui_vertex_attribute(count, &mut floats, &mut offset), 0, "past the end should report nothing");

        // And it can be turned off again.
        libgui_enable_mesh(u, 0);
        frame(u, || libgui_label(u, c("x").as_ptr()));
        let mut after = 1u64;
        libgui_mesh_vertices(u, &mut after);
        assert_eq!(after, 0, "the mesh kept being built after it was turned off");
        libgui_ui_free(u);
    }
}

extern "C" fn cell(ui: *mut LibguiUi, row: u64, col: u64, user: *mut c_void) {
    let seen = unsafe { &mut *(user as *mut u32) };
    *seen += 1;
    // The point of the test: building through the handle the host owns, while
    // libgui holds a live `&mut Ui` for the table walk.
    let label = CString::new(format!("r{row}c{col}")).unwrap();
    unsafe { libgui_label(ui, label.as_ptr()) };
}

/// A table's cells reborrow the host's handle, the same trick docking uses.
/// In Rust so Miri checks the aliasing; nothing else drives a table from C.
#[test]
fn a_table_builds_its_cells_through_the_hosts_own_handle() {
    unsafe {
        let u = ui();
        let t = libgui_table_new();
        assert!(!t.is_null());
        libgui_table_add_column(t, c("Name").as_ptr(), 120.0, 1.0, 1, 1, 0);
        libgui_table_add_column(t, c("Value").as_ptr(), 80.0, 0.0, 1, 0, 0);
        libgui_table_set_frozen(t, 1);

        let mut cells: u32 = 0;
        let mut out = LibguiTableResponse::default();
        frame(u, || {
            libgui_table_show(u, t, c("rows").as_ptr(), 4, Some(cell), &mut cells as *mut u32 as *mut c_void, &mut out);
        });
        assert!(cells > 0, "no cell was built");
        assert_eq!(out.row_clicked, 0);

        // Columns can be replaced between frames.
        libgui_table_clear_columns(t);
        libgui_table_add_column(t, c("Only").as_ptr(), 100.0, 1.0, 0, 0, 0);
        let before = cells;
        frame(u, || {
            libgui_table_show(u, t, c("rows").as_ptr(), 4, Some(cell), &mut cells as *mut u32 as *mut c_void, &mut out);
        });
        assert!(cells > before, "the table stopped building cells after its columns changed");
        libgui_table_free(t);
        libgui_ui_free(u);
    }
}

/// Closing and cancelling, which a host needs when a window's close box is hit
/// or a drag is interrupted.
#[test]
fn a_dock_surface_can_be_closed_and_a_drag_cancelled() {
    unsafe {
        let d = libgui_dock_new();
        assert!(!d.is_null());
        let a = libgui_dock_leaf(d, 1);
        let b = libgui_dock_leaf(d, 2);
        let root = libgui_dock_split(d, 0, 0.5, a, b);
        assert_eq!(libgui_dock_set_root(d, SURFACE_MAIN, root), 0);

        assert_eq!(libgui_dock_is_dragging(d), 0);
        // Cancelling when nothing is dragging is not an error: a host calls it
        // when it loses the pointer and cannot know.
        libgui_dock_cancel_drag(d);
        assert_eq!(libgui_dock_is_dragging(d), 0);

        let before = libgui_dock_surface_count(d);
        // Closing the main surface is refused; it is the window the app is.
        libgui_dock_close_surface(d, SURFACE_MAIN);
        assert_eq!(libgui_dock_surface_count(d), before, "the main surface was closed");
        libgui_dock_free(d);
    }
}

/// Popups, layers and the fallback chain: the last of the reviewer's gaps.
#[test]
fn popups_layers_and_a_font_chain_all_work() {
    unsafe {
        let u = ui();
        let pid = libgui_id_from_name(c("props_popup").as_ptr());
        let anchor = LibguiRect { x: 10.0, y: 10.0, w: 80.0, h: 24.0 };

        // Nothing open to begin with.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        assert_eq!(libgui_any_popup_open(u), 0, "a popup was open before one was asked for");
        assert_eq!(libgui_popup_open(u, pid), 0);
        libgui_end_frame(u);

        // Open it, then build its body on the frames where it is open.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_popup(u, pid, anchor);
        libgui_end_frame(u);

        let mut built = 0;
        for _ in 0..3 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            if libgui_open_popup_body(u, pid, 160.0) != 0 {
                built += 1;
                let label = c("Rename");
                libgui_menu_item(u, label.as_ptr());
                libgui_close_popup_body(u);
            }
            libgui_end_frame(u);
        }
        assert!(built > 0, "the popup never opened its body");
        assert_eq!(libgui_popup_open(u, pid), 1, "the popup closed by itself");
        assert_eq!(libgui_any_popup_open(u), 1, "any_popup_open disagrees with popup_open");

        // A submenu, which is the only way the chain is ever deeper than one
        // and so the only thing that tells close_popup from close_popups.
        let sub = libgui_id_from_name(c("props_submenu").as_ptr());
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_child_popup(u, pid, sub, anchor);
        libgui_end_frame(u);
        assert_eq!(libgui_popup_open(u, sub), 1, "the submenu never opened");
        assert_eq!(libgui_popup_open(u, pid), 1, "the submenu replaced its parent");

        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_close_popup(u, sub);
        libgui_end_frame(u);
        assert_eq!(libgui_popup_open(u, sub), 0, "close_popup left the submenu open");
        assert_eq!(libgui_popup_open(u, pid), 1, "close_popup closed the parent too");

        // Escape's job.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_close_popups(u);
        libgui_end_frame(u);
        assert_eq!(libgui_any_popup_open(u), 0, "close_popups left one open");

        // A modal: a scrim over the window, then the dialog above it.
        let scrim = LibguiFrame {
            fill: LibguiColor { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
            border: LibguiColor::default(),
            border_width: 0.0,
            radius: 0.0,
            clip: 1,
            shadow: 0,
            _pad: [0; 6],
        };
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_layer(u, libgui_id_from_name(c("scrim").as_ptr()), LIBGUI_LAYER_POPUP,
                          LibguiRect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 }, scrim);
        libgui_close_layer(u);
        libgui_open_layer(u, libgui_id_from_name(c("dialog").as_ptr()), LIBGUI_LAYER_POPUP,
                          LibguiRect { x: 80.0, y: 60.0, w: 240.0, h: 140.0 }, scrim);
        let msg = c("Discard changes?");
        libgui_heading(u, msg.as_ptr());
        libgui_close_layer(u);
        libgui_end_frame(u);
        assert_eq!(libgui_ui_poisoned(u), 0, "the modal poisoned the handle");
        assert_eq!(libgui_open_depth(u), 0, "the layers did not close");
        libgui_ui_free(u);

        // A fallback chain. Inter twice is a chain of two as far as the ABI is
        // concerned; what matters here is that the array form is accepted and
        // that a bad entry is refused rather than crashing.
        let fonts = [FONT.as_ptr(), FONT.as_ptr()];
        let lens = [FONT.len() as u64, FONT.len() as u64];
        let chained = libgui_ui_new_with_fallbacks(fonts.as_ptr(), lens.as_ptr(), 2);
        assert!(!chained.is_null(), "a two-font chain was refused");
        libgui_begin_frame(chained, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let label = c("Body");
        libgui_label(chained, label.as_ptr());
        libgui_end_frame(chained);
        assert_eq!(libgui_ui_poisoned(chained), 0);
        libgui_ui_free(chained);

        assert!(libgui_ui_new_with_fallbacks(std::ptr::null(), std::ptr::null(), 0).is_null());
        let bad = [std::ptr::null::<u8>()];
        let badlen = [0u64];
        assert!(libgui_ui_new_with_fallbacks(bad.as_ptr(), badlen.as_ptr(), 1).is_null(),
                "an empty font was accepted");
    }
}

/// The rest of the painter, and subtree caching: the two places a C host had
/// strictly less than a Rust one.
#[test]
fn the_whole_painter_and_the_subtree_cache_reach_c() {
    unsafe extern "C" fn paint_everything(p: *mut LibguiPainter, r: LibguiRect, user: *mut c_void) {
        let hit = unsafe { &mut *(user as *mut u32) };
        *hit += 1;
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let txt = CString::new("Extrude").unwrap();

        // The one a custom widget cannot do without: how big is my text?
        let (mut w, mut h) = (0.0f32, 0.0f32);
        unsafe { libgui_painter_measure(p, 13.0, txt.as_ptr(), &mut w, &mut h) };
        assert!(w > 0.0 && h > 0.0, "measure returned nothing: {w}x{h}");

        // A crisp single-pixel rule, which is most of what a CAD drawing is.
        let mut line = LibguiRect::default();
        unsafe { libgui_painter_hairline(p, r.x, r.y, 1.0, r.h, &mut line) };
        assert!(line.w > 0.0, "hairline returned an empty rect");
        unsafe { libgui_painter_rect(p, line, white, 0.0) };

        let mut snapped = LibguiRect::default();
        unsafe { libgui_painter_snap_rect(p, r, &mut snapped) };
        assert_eq!(snapped.x, snapped.x.round(), "snap_rect did not snap");

        unsafe {
            libgui_painter_shadow(p, r, 4.0, 12.0, white);
            libgui_painter_text(p, r.x, r.y, 13.0, white, txt.as_ptr());
            libgui_painter_text_right(p, r, 13.0, white, txt.as_ptr());
            libgui_painter_text_centered(p, r, 13.0, white, txt.as_ptr());
            libgui_painter_text_wrapped(p, r, 13.0, white, 1, txt.as_ptr());
            let pts = [r.x, r.y, r.x + 10.0, r.y + 10.0, r.x + 20.0, r.y];
            libgui_painter_polyline(p, pts.as_ptr(), 3, 2.0, white);
            libgui_painter_bezier(p, r.x, r.y, r.x + 5.0, r.y + 5.0, r.x + 15.0, r.y + 5.0, r.x + 20.0, r.y, 2.0, white);
            libgui_painter_wire(p, r.x, r.y, r.x + 30.0, r.y + 20.0, 2.0, white);
            libgui_painter_chevron(p, r, 8.0, 1, white);
            libgui_painter_image_tinted(p, r, 0, 0.0, 0.0, 1.0, 1.0, 0.0, white);
            let black = LibguiColor { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
            libgui_painter_gradient(p, r, white, black, 0);
            libgui_painter_gradient(p, r, LibguiColor::default(), black, 1);
            libgui_painter_gradient(std::ptr::null_mut(), r, white, black, 0);
            // Nulls, which a host will pass eventually.
            libgui_painter_text(p, 0.0, 0.0, 13.0, white, std::ptr::null());
            libgui_painter_polyline(p, std::ptr::null(), 0, 1.0, white);
            libgui_painter_measure(p, 13.0, txt.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut());
        }
    }

    unsafe {
        let u = ui();
        let mut hits: u32 = 0;
        let layout = leaf_layout(80.0, 30.0);
        let id = libgui_id_from_name(c("gizmo").as_ptr());

        for _ in 0..3 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            let cb = LibguiPaintFn {
                paint: Some(paint_everything),
                drop_user: None,
                user: &mut hits as *mut u32 as *mut c_void,
            };
            libgui_add_leaf(u, id, layout, 1, cb);
            libgui_end_frame(u);
        }
        assert!(hits > 0, "the paint callback never ran");
        assert_eq!(libgui_ui_poisoned(u), 0, "the painter calls poisoned the handle");

        // --- the subtree cache -------------------------------------------
        // Same deps: built once, replayed after. That is the whole point.
        let key = c("panel");
        let mut builds = 0;
        for _ in 0..5 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            if libgui_open_cached(u, key.as_ptr(), 7) != 0 {
                builds += 1;
                libgui_label(u, c("Fillet 2mm").as_ptr());
                libgui_close_cached(u);
            }
            libgui_end_frame(u);
        }
        assert!(builds < 5, "the cache never replayed: built {builds} of 5 frames");
        assert!(builds > 0, "the cache never built it at all");

        // Changing deps rebuilds — which is how a section runs at its own rate.
        let before = builds;
        for tick in 0..3u64 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            if libgui_open_cached(u, key.as_ptr(), 100 + tick) != 0 {
                builds += 1;
                libgui_label(u, c("Fillet 2mm").as_ptr());
                libgui_close_cached(u);
            }
            libgui_end_frame(u);
        }
        assert_eq!(builds - before, 3, "a changed dep did not rebuild every time");
        assert_eq!(libgui_ui_poisoned(u), 0, "caching poisoned the handle");
        libgui_ui_free(u);
    }
}

// ---------------------------------------------------------------------------
// Misuse of the open/close pairs
//
// A C caller has no borrow checker and no destructors. The pairs are the one
// place where a mistake is easy to make and was, until these, easy to miss:
// both of the cases below produced a quietly wrong frame in a release build
// and a handle that reported itself healthy.
// ---------------------------------------------------------------------------

/// Builds a frame in which an inner cache replays while the outer one is still
/// building, and returns the instance count and whether the handle survived.
/// With `stray`, the caller closes whatever `libgui_open_cached` returned —
/// the likeliest mistake there is, since ignoring a return value compiles.
unsafe fn cached_frames(stray: bool) -> (u64, u8) {
    unsafe {
        let u = ui();
        let mut instances = 0;
        for i in 0..6u64 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            // Builds on the first two frames and replays after, so the second
            // frame's recording is what every later frame draws.
            if libgui_open_cached(u, c("outer").as_ptr(), i.min(1)) != 0 {
                libgui_label(u, c("outer content").as_ptr());
                let inner = libgui_open_cached(u, c("inner").as_ptr(), 1);
                libgui_label(u, c("inner content").as_ptr());
                if stray || inner != 0 {
                    libgui_close_cached(u);
                }
                // The caller believes it is still inside the outer subtree.
                libgui_label(u, c("after the inner one").as_ptr());
                libgui_close_cached(u);
            }
            libgui_end_frame(u);
            libgui_frame_instances(u, &mut instances);
        }
        let poisoned = libgui_ui_poisoned(u);
        libgui_ui_free(u);
        (instances, poisoned)
    }
}

/// The pair used correctly: a nested cache replays inside one that is
/// rebuilding, and the frame is whole.
#[test]
fn a_cache_inside_a_cache_replays_and_keeps_the_frame_whole() {
    unsafe {
        let (instances, poisoned) = cached_frames(false);
        assert_eq!(poisoned, 0, "the correct program was refused");
        assert!(instances > 0, "nothing was drawn");
    }
}

/// Closing on a frame that replayed must not quietly pop the *enclosing*
/// cache. It used to: the outer recording ended early, everything after the
/// inner subtree fell outside it, and every later replay drew a subtree with a
/// piece missing — 52 instances become 36, with the handle reporting itself
/// healthy. `debug_assert` made that a release-only fault, which is the build
/// an application ships.
#[test]
fn a_stray_close_cached_is_refused_rather_than_losing_content() {
    unsafe {
        let (whole, _) = cached_frames(false);
        let (after_stray, poisoned) = cached_frames(true);
        assert_ne!(poisoned, 0, "a stray close_cached was accepted silently");
        assert!(
            after_stray != whole || poisoned != 0,
            "the frame lost content ({whole} instances -> {after_stray}) without saying so"
        );
    }
}

/// A container the caller never closed. libgui's own check is a
/// `debug_assert`, so in release the frame was simply wrong; the boundary
/// names the caller instead.
#[test]
fn a_container_left_open_is_reported_rather_than_drawn_wrong() {
    unsafe {
        let u = ui();
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_scroll_area(u, c("list").as_ptr());
        libgui_label(u, c("inside").as_ptr());
        // ... and no close.
        libgui_end_frame(u);

        assert_ne!(libgui_ui_poisoned(u), 0, "an unbalanced frame was accepted");
        let err = libgui_last_error();
        assert!(!err.is_null(), "nothing said why");
        let err = std::ffi::CStr::from_ptr(err).to_string_lossy();
        assert!(err.contains("still open"), "the error does not name the mistake: {err}");
        libgui_ui_free(u);
    }
}

/// A popup is closed most of the time, so `libgui_open_popup_body` returns 0
/// on most frames — which makes ignoring its answer both the easiest mistake
/// and the worst one. Closing anyway used to close whatever container the
/// caller had open, with no check at all in either build; the panic that
/// eventually followed named a later `close_container`, so the call that
/// actually did the damage never appeared.
#[test]
fn a_stray_close_popup_body_names_itself() {
    unsafe {
        let u = ui();
        let mut layout: LibguiLayout = std::mem::zeroed();
        layout.axis = 1;
        layout.width = LibguiSize { kind: LibguiSizeKind::Grow, value: 1.0 };
        layout.height = LibguiSize { kind: LibguiSizeKind::Fit, value: 0.0 };
        let frame_: LibguiFrame = std::mem::zeroed();

        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_container(u, libgui_id_from_name(c("panel").as_ptr()), layout, frame_);
        assert_eq!(libgui_open_popup_body(u, 77, 200.0), 0, "nothing opened this popup");
        libgui_close_popup_body(u); // the mistake
        libgui_end_frame(u);

        assert_ne!(libgui_ui_poisoned(u), 0, "a stray close_popup_body was accepted");
        let err = std::ffi::CStr::from_ptr(libgui_last_error()).to_string_lossy().into_owned();
        assert!(err.contains("close_popup_body"), "the error blames the wrong call: {err}");
        libgui_ui_free(u);
    }
}

/// A polyline is read from the caller's array in place, with no copy: this is
/// a paint callback, it runs for every polyline of every frame, and a CAD
/// drawing is mostly polylines.
///
/// The check that matters is that reading it in place still reads the right
/// points, so the same polyline is drawn through C and through the Rust API
/// and the two frames are compared byte for byte.
#[test]
fn a_polyline_is_read_in_place_and_reads_the_same_points() {
    // A shape with distinct, asymmetric coordinates: swapped or shifted
    // components would still draw *something*, and this notices.
    const PTS: [f32; 10] = [10.0, 20.0, 60.0, 25.0, 70.0, 80.0, 30.0, 95.0, 12.0, 55.0];

    unsafe extern "C" fn paint(p: *mut LibguiPainter, _r: LibguiRect, _user: *mut c_void) {
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        unsafe { libgui_painter_polyline(p, PTS.as_ptr(), 5, 2.0, white) };
        // Degenerate inputs are not a crash.
        unsafe { libgui_painter_polyline(p, std::ptr::null(), 5, 2.0, white) };
        unsafe { libgui_painter_polyline(p, PTS.as_ptr(), 0, 2.0, white) };
    }

    let layout = LibguiLayout {
        axis: 1,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 100.0 },
        height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 100.0 },
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        gap: 0.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    };

    // Through C.
    let from_c = unsafe {
        let u = ui();
        libgui_begin_frame(u, 200.0, 200.0, 1.0, 1.0 / 60.0);
        libgui_add_leaf(
            u,
            libgui_id_from_name(c("pl").as_ptr()),
            layout,
            0,
            LibguiPaintFn { paint: Some(paint), drop_user: None, user: std::ptr::null_mut() },
        );
        libgui_end_frame(u);
        let mut n = 0u64;
        let p = libgui_frame_instances(u, &mut n) as *const u8;
        let bytes = std::slice::from_raw_parts(p, n as usize * libgui_instance_stride() as usize).to_vec();
        assert!(!bytes.is_empty(), "the polyline drew nothing");
        libgui_ui_free(u);
        bytes
    };

    // The same polyline, through the Rust API.
    let from_rust = {
        let mut ui = libgui::Ui::new(libgui::Theme::dark(), FONT).expect("font");
        ui.begin_frame(libgui::FrameInfo {
            screen_size: libgui::Vec2::new(200.0, 200.0),
            scale: 1.0,
            dt: 1.0 / 60.0,
        });
        let pts: Vec<libgui::Vec2> = PTS.chunks_exact(2).map(|q| libgui::Vec2::new(q[0], q[1])).collect();
        ui.add_leaf(
            libgui::Id::from_name("pl"),
            libgui::Layout::leaf(libgui::Size::Fixed(100.0), libgui::Size::Fixed(100.0)),
            libgui::Vec2::ZERO,
            false,
            move |p, _r| p.polyline(&pts, 2.0, libgui::Color::WHITE),
        );
        let out = ui.end_frame();
        let inst = out.draw.instances.as_slice();
        let bytes = unsafe {
            std::slice::from_raw_parts(inst.as_ptr() as *const u8, std::mem::size_of_val(inst))
        }
        .to_vec();
        drop(out);
        bytes
    };

    assert_eq!(from_c, from_rust, "the polyline read through C is not the one the caller passed");
}

/// A texture id is whatever the host's renderer calls a texture — a bgfx
/// handle, a GL name, a pointer. It used to be cut to 32 bits on the way
/// through, so anything above 4 billion came back as a different texture and
/// the host drew the wrong thing with no error anywhere.
#[test]
fn a_64_bit_texture_id_survives_the_trip() {
    unsafe {
        // A value with bits set above 32, the way a pointer or a packed
        // handle has: truncation would return the low half.
        const ID: u64 = 0x1234_5678_9abc_def0;
        let u = ui();
        frame(u, || {
            let _ = libgui_viewport(u, c("scene").as_ptr(), ID);
        });
        let mut n = 0u64;
        let batches = std::slice::from_raw_parts(libgui_frame_batches(u, &mut n), n as usize);
        let mine = batches.iter().find(|b| b.texture_kind == 1).expect("the viewport drew nothing");
        assert_eq!(mine.texture_index, ID, "the texture id was cut on the way through");
        libgui_ui_free(u);
    }
}

// ---------------------------------------------------------------------------
// The conformance kit
// ---------------------------------------------------------------------------

/// The reference image a host checks its own renderer against must be the
/// same image `libgui_soft` produces directly — otherwise the kit certifies
/// the wrong thing, which is worse than having no kit.
#[test]
fn the_reference_pixels_are_the_reference_renderers_own() {
    use libgui_soft::scenes::SCENES;

    // A scene that needs no input, so both paths can build it the same way.
    let (index, scene) = SCENES
        .iter()
        .enumerate()
        .find(|(_, s)| s.pointer == libgui_soft::scenes::Pointer::None)
        .expect("no input-free scene");

    let scale = 1.5; // where pixel-snapping bugs show
    let (w, h) = scene.size;

    let through_c = unsafe {
        let u = ui();
        libgui_enable_reference_render(u, 1);
        // Twice: layout is solved after a frame is built.
        for _ in 0..2 {
            libgui_begin_frame(u, w, h, scale, 1.0);
            assert_eq!(libgui_conformance_build(u, index as u32), 1, "the scene did not build");
            libgui_end_frame(u);
        }
        let (mut rw, mut rh) = (0u32, 0u32);
        let p = libgui_reference_pixels(u, &mut rw, &mut rh);
        assert!(!p.is_null(), "no reference image");
        assert_eq!((rw, rh), scene.pixels(scale), "the reference is the wrong size");
        let px = std::slice::from_raw_parts(p, (rw * rh * 4) as usize).to_vec();
        libgui_ui_free(u);
        px
    };

    // The same scene, rendered by libgui_soft directly.
    let (pw, ph) = scene.pixels(scale);
    let direct = scene.run(libgui::Theme::dark(), scale, FONT, |out, _| {
        libgui_soft::SoftRenderer::new().render_to_image(out, pw, ph).data
    });

    assert_eq!(through_c.len(), direct.len(), "the two paths disagree about the image size");
    let differing = through_c.iter().zip(&direct).filter(|(a, b)| a != b).count();
    assert_eq!(differing, 0, "{differing} bytes differ between the C path and the reference renderer");
    assert!(through_c.iter().any(|&b| b != 0), "the reference image is blank");
}

/// The gallery describes itself, so a host can loop over it without knowing
/// what is in it.
#[test]
fn the_scene_gallery_describes_itself() {
    unsafe {
        let n = libgui_conformance_scene_count();
        assert!(n > 5, "a gallery of {n} scenes does not cover much");
        let mut input_free = 0;
        for i in 0..n {
            let name = libgui_conformance_scene_name(i);
            assert!(!name.is_null(), "scene {i} has no name");
            let name = std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned();
            assert!(!name.is_empty());

            let (mut w, mut h) = (0.0f32, 0.0f32);
            assert_eq!(libgui_conformance_scene_size(i, &mut w, &mut h), 1, "{name} has no size");
            assert!(w > 0.0 && h > 0.0, "{name} is {w}x{h}");
            input_free += (libgui_conformance_scene_needs_input(i) == 0) as u32;
        }
        assert!(input_free >= 4, "only {input_free} scenes can be checked without driving input");

        // Out of range is an answer, not a crash.
        assert!(libgui_conformance_scene_name(n).is_null());
        assert_eq!(libgui_conformance_scene_size(n, std::ptr::null_mut(), std::ptr::null_mut()), 0);
        let u = ui();
        libgui_begin_frame(u, 100.0, 100.0, 1.0, 1.0 / 60.0);
        assert_eq!(libgui_conformance_build(u, n), 0, "an out-of-range scene claimed to build");
        libgui_end_frame(u);
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// Off by default: it rasterises the whole frame in software.
#[test]
fn the_reference_render_is_off_until_asked_for() {
    unsafe {
        let u = ui();
        frame(u, || libgui_label(u, c("x").as_ptr()));
        let (mut w, mut h) = (1u32, 1u32);
        assert!(libgui_reference_pixels(u, &mut w, &mut h).is_null(), "it rendered without being asked");
        assert_eq!((w, h), (0, 0));
        libgui_ui_free(u);
    }
}

/// Width and offset describe a vertex buffer; the name connects it to a
/// shader. Without it a host hard-codes the mapping, which is a silent
/// dependency on an order that could change.
#[test]
fn every_vertex_attribute_says_what_it_is() {
    unsafe {
        let count = libgui_vertex_attribute_count();
        let names: Vec<String> = (0..count)
            .map(|i| {
                let p = libgui_vertex_attribute_name(i);
                assert!(!p.is_null(), "attribute {i} has no name");
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            })
            .collect();
        // The names are the shader's varyings, in the order the vertex packs
        // them, and they must agree with what libgui itself calls them.
        let want: Vec<&str> =
            libgui::render_contract::VERTEX_ATTRIBUTES.iter().map(|&(_, name, _, _)| name).collect();
        assert_eq!(names, want, "the C names have drifted from the contract");
        assert!(libgui_vertex_attribute_name(count).is_null(), "past the end should be null");
    }
}

/// A renderer rarely has a texture the exact size of the widget: pooled or
/// fixed-size targets, a scene rendered at half resolution, several views in
/// one atlas. Showing the whole texture and nothing else meant rebuilding the
/// widget by hand out of add_leaf, painter_image_uv and interact.
#[test]
fn a_viewport_can_show_part_of_a_texture() {
    unsafe {
        let u = ui();
        frame(u, || {
            // The top-left quarter of a screen-sized target.
            let _ = libgui_viewport_uv(u, c("scene").as_ptr(), 9, 0.0, 0.0, 0.5, 0.5);
        });
        let mut n = 0u64;
        let batches = std::slice::from_raw_parts(libgui_frame_batches(u, &mut n), n as usize);
        assert!(batches.iter().any(|b| b.texture_kind == 1 && b.texture_index == 9), "it drew no texture");

        // And the uv actually reaches the instance rather than being ignored.
        let full = {
            let u2 = ui();
            libgui_begin_frame(u2, 400.0, 300.0, 1.0, 1.0 / 60.0);
            let _ = libgui_viewport(u2, c("scene").as_ptr(), 9);
            libgui_end_frame(u2);
            let mut m = 0u64;
            let p = libgui_frame_instances(u2, &mut m) as *const u8;
            let bytes = std::slice::from_raw_parts(p, m as usize * libgui_instance_stride() as usize).to_vec();
            libgui_ui_free(u2);
            bytes
        };
        let mut m = 0u64;
        let p = libgui_frame_instances(u, &mut m) as *const u8;
        let part = std::slice::from_raw_parts(p, m as usize * libgui_instance_stride() as usize).to_vec();
        assert_ne!(part, full, "the sub-rect drew the same instances as the whole texture");
        libgui_ui_free(u);
    }
}

/// A renderer streaming into a fixed per-frame buffer loses draw calls once a
/// frame passes its ceiling — bgfx's default transient buffer is about
/// thirteen thousand quads. The mesh can be cut to fit, and the cut has to
/// describe itself well enough to upload without arithmetic.
#[test]
fn the_mesh_can_be_cut_to_fit_a_fixed_buffer() {
    unsafe {
        let u = ui();
        libgui_enable_mesh(u, 1);
        libgui_set_mesh_limits(u, 64, 96); // 16 quads

        frame(u, || {
            for i in 0..40 {
                let _ = libgui_button(u, c(&format!("Row {i}")).as_ptr());
            }
        });

        let (mut nv, mut ni, mut nb, mut nc) = (0u64, 0u64, 0u64, 0u64);
        libgui_mesh_vertices(u, &mut nv);
        let all_indices = libgui_mesh_indices(u, &mut ni);
        let all_indices = std::slice::from_raw_parts(all_indices, ni as usize);
        let batches = std::slice::from_raw_parts(libgui_mesh_batches(u, &mut nb), nb as usize);
        let chunks = std::slice::from_raw_parts(libgui_mesh_chunks(u, &mut nc), nc as usize);
        assert!(nc > 1, "a frame this size should have been cut into chunks, got {nc}");
        assert_eq!(libgui_mesh_fits_u16(u), 1, "chunks this small must fit 16-bit indices");

        let (mut v_seen, mut i_seen, mut b_seen) = (0u32, 0u32, 0u32);
        for (n, ch) in chunks.iter().enumerate() {
            assert!(ch.vertex_count <= 64, "chunk {n} has {} vertices", ch.vertex_count);
            assert!(ch.index_count <= 96, "chunk {n} has {} indices", ch.index_count);
            // The chunks tile the arrays in order, so nothing is drawn twice
            // and nothing is skipped.
            assert_eq!(ch.vertex_first, v_seen, "chunk {n} does not follow the one before it");
            assert_eq!(ch.index_first, i_seen);
            assert_eq!(ch.batch_first, b_seen);
            v_seen += ch.vertex_count;
            i_seen += ch.index_count;
            b_seen += ch.batch_count;

            // Its indices address its own vertices, and its batches its own
            // indices: upload the slices, draw, no arithmetic.
            for &idx in &all_indices[ch.index_first as usize..(ch.index_first + ch.index_count) as usize] {
                assert!(idx < ch.vertex_count, "chunk {n} indexes vertex {idx} of {}", ch.vertex_count);
            }
            for b in &batches[ch.batch_first as usize..(ch.batch_first + ch.batch_count) as usize] {
                assert!(b.first + b.count <= ch.index_count, "a batch runs past chunk {n}");
            }
        }
        assert_eq!((v_seen as u64, i_seen as u64, b_seen as u64), (nv, ni, nb), "the chunks do not cover the mesh");

        // No limit is the default, and puts it back to one upload.
        libgui_set_mesh_limits(u, 0, 0);
        frame(u, || {
            for i in 0..40 {
                let _ = libgui_button(u, c(&format!("Row {i}")).as_ptr());
            }
        });
        let mut one = 0u64;
        libgui_mesh_chunks(u, &mut one);
        assert_eq!(one, 1, "with no limit the mesh is a single chunk");
        libgui_ui_free(u);
    }
}

/// A torn-off window draws from the window it came from: one atlas,
/// rasterised once and uploaded once. Each window used to carry its own, which
/// for a CJK interface is thousands of glyphs and megabytes of texture per
/// window.
#[test]
fn a_second_ui_can_share_the_first_ones_atlas() {
    unsafe {
        let main = ui();
        frame(main, || libgui_label(main, c("Geometry Spreadsheet").as_ptr()));
        let (mut size_a, mut ver_a) = (0u32, 0u64);
        let atlas_a = libgui_frame_atlas(main, &mut size_a, &mut ver_a);
        assert!(!atlas_a.is_null());

        let torn = libgui_ui_new_sharing_fonts(main);
        assert!(!torn.is_null(), "sharing failed: {:?}", std::ffi::CStr::from_ptr(libgui_last_error()));
        frame(torn, || libgui_label(torn, c("Geometry Spreadsheet").as_ptr()));

        let (mut size_b, mut ver_b) = (0u32, 0u64);
        let atlas_b = libgui_frame_atlas(torn, &mut size_b, &mut ver_b);
        // The same image: a host uploads it once and both windows sample it.
        assert_eq!(atlas_a, atlas_b, "the windows report different atlases");
        assert_eq!((size_a, ver_a), (size_b, ver_b), "the windows disagree about the atlas");

        // A window given its own fonts still has its own.
        let alone = ui();
        frame(alone, || libgui_label(alone, c("Geometry Spreadsheet").as_ptr()));
        let (mut size_c, mut ver_c) = (0u32, 0u64);
        assert_ne!(libgui_frame_atlas(alone, &mut size_c, &mut ver_c), atlas_a, "an unshared window shared one");

        // Freed in the wrong order on purpose: the font system outlives
        // whichever handle goes first.
        libgui_ui_free(main);
        frame(torn, || libgui_label(torn, c("Still here").as_ptr()));
        assert_eq!(libgui_ui_poisoned(torn), 0, "the survivor was poisoned by the other window closing");
        libgui_ui_free(torn);
        libgui_ui_free(alone);

        // Null and poisoned handles are answers, not crashes.
        assert!(libgui_ui_new_sharing_fonts(std::ptr::null_mut()).is_null());
    }
}

/// The canvas, the transform under it, and the animation a custom widget
/// moves by — the four things a C++ sketcher needs and could not reach.
///
/// The claim worth testing is not that the calls are callable but that a
/// widget built inside a canvas sees **canvas** coordinates: that is the whole
/// reason to use one rather than a viewport plus arithmetic, and it is what
/// lets a sketcher's snapping be written once in model units.
#[test]
fn a_canvas_puts_a_c_caller_in_canvas_coordinates() {
    unsafe {
        let u = ui();
        let mut st = LibguiCanvasState::default();
        libgui_canvas_state_default(&mut st);
        assert_eq!(st.zoom, 1.0, "the defaults did not reach the caller");
        assert_eq!(st.wheel_zooms, 1);

        // Zoom to 2x about the canvas widget's own origin, so the mapping is a
        // clean doubling and a wrong one cannot look right by accident.
        let origin = LibguiVec2 { x: 0.0, y: 0.0 };
        libgui_canvas_zoom_at(&mut st, origin, origin, 2.0);
        assert_eq!(st.zoom, 2.0, "zoom_at did not zoom");

        // Put the pointer somewhere unambiguous and settle a frame, so the
        // canvas has a rect and the hit test has something to answer with.
        libgui_push_pointer_moved(u, 200.0, 120.0);
        let mut view = LibguiCanvasView::default();
        let mut seen = LibguiVec2 { x: 0.0, y: 0.0 };
        let mut bg_mouse = LibguiVec2 { x: 0.0, y: 0.0 };
        let mut window_mouse = LibguiVec2 { x: 0.0, y: 0.0 };
        for _ in 0..3 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            // Outside the canvas: window coordinates.
            let out = libgui_button(u, c("outside").as_ptr());
            window_mouse = out.mouse_pos;

            let bg = libgui_open_canvas(u, c("sketch").as_ptr(), &mut st, &mut view);
            bg_mouse = bg.mouse_pos;
            // A widget *inside* is the one that sees canvas coordinates.
            seen = libgui_button(u, c("inside").as_ptr()).mouse_pos;
            libgui_close_canvas(u);
            libgui_end_frame(u);
        }
        assert_eq!(libgui_ui_poisoned(u), 0, "the canvas poisoned the handle");

        // The view was reported and the state kept its zoom across frames.
        assert_eq!(view.zoom, 2.0, "the view did not carry the zoom");
        assert_eq!(view.xform_zoom, 2.0);
        assert_eq!(st.zoom, 2.0, "the canvas lost the app's zoom");
        assert!(view.visible.w > 0.0, "the visible rect was never written");
        // The state is the app's copy, and libgui writes back into it: this is
        // the field a sketcher culls against, so a lost write-back means it
        // builds every entity in the model every frame.
        assert_eq!(
            (st.visible.w, st.visible.h),
            (view.visible.w, view.visible.h),
            "libgui's update to the canvas state never reached the caller"
        );
        assert!(st.visible.w > 0.0, "the caller's own visible rect stayed empty");

        // The point of the whole thing: at 2x the canvas sees half the window
        // coordinate, because the canvas origin is the widget's own here.
        assert_eq!(window_mouse.x, 200.0, "a widget outside the canvas moved");
        assert!(
            (seen.x - 100.0).abs() < 0.01,
            "inside a 2x canvas the pointer read {:?}, not half the window position",
            seen
        );
        // The background's own response is the exception, and the header says
        // so: it is produced before the transform is pushed, so it carries
        // WINDOW coordinates. Pinned here because a sketcher that assumed
        // otherwise would misplace every click on empty canvas by the zoom.
        assert_eq!(bg_mouse.x, 200.0, "the background response is documented as window-space");

        // The same mapping, exposed for a host translating its own events.
        let pan = view.xform_pan;
        let there = libgui_transform_point(pan, view.xform_zoom, LibguiVec2 { x: 100.0, y: 60.0 });
        let back = libgui_transform_inv_point(pan, view.xform_zoom, there);
        assert!(
            (back.x - 100.0).abs() < 0.01 && (back.y - 60.0).abs() < 0.01,
            "point and inv_point do not round-trip: {back:?}"
        );

        // The raw transform, for a view the app drives itself.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_open_transform(u, 77, LibguiVec2 { x: 10.0, y: 5.0 }, 3.0);
        libgui_label(u, c("under a transform").as_ptr());
        libgui_close_transform(u);
        libgui_end_frame(u);
        assert_eq!(libgui_ui_poisoned(u), 0, "the transform poisoned the handle");

        libgui_ui_free(u);
    }
}

/// A custom widget's motion: the value is retained, eases rather than jumping,
/// and keeps the host awake until it arrives.
#[test]
fn a_c_caller_can_animate_its_own_widget() {
    unsafe {
        let u = ui();
        let id = libgui_id_from_name(c("my_widget").as_ptr());

        // First call starts *at* the target: nothing to ease from.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        assert_eq!(libgui_animate_bool(u, id, 0, 0), 0.0);
        libgui_end_frame(u);

        // Now aim at 1 and watch it approach rather than arrive.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let first = libgui_animate_bool(u, id, 0, 1);
        libgui_end_frame(u);
        assert!(first > 0.0 && first < 1.0, "the animation jumped straight to {first}");

        let mut plat = LibguiPlatformOutput::default();
        libgui_frame_platform(u, &mut plat);
        assert!(plat.repaint_after >= 0.0, "an unfinished animation let the host sleep");

        // It keeps going, and it is retained: a second slot is its own value.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let second = libgui_animate_bool(u, id, 0, 1);
        let other = libgui_animate(u, id, 1, 0.0);
        libgui_end_frame(u);
        assert!(second > first, "the animation did not advance: {first} then {second}");
        assert_eq!(other, 0.0, "slot 1 was not its own value");

        // Enough frames and it settles exactly, rather than creeping forever.
        for _ in 0..200 {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            libgui_animate_bool(u, id, 0, 1);
            libgui_end_frame(u);
        }
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let settled = libgui_animate_bool(u, id, 0, 1);
        libgui_end_frame(u);
        assert_eq!(settled, 1.0, "the animation never settled");

        // An explicit rate, and a jump that the next ease starts from.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_set_anim(u, id, 0, 0.0);
        let after_jump = libgui_animate_with_speed(u, id, 0, 1.0, 4.0);
        libgui_end_frame(u);
        assert!(after_jump < 0.5, "set_anim did not take, or the speed was ignored: {after_jump}");

        // keep_id on its own: an animation whose widget was not built this
        // frame is still there when it comes back.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_set_anim(u, id, 2, 0.5);
        libgui_end_frame(u);
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_keep_id(u, id); // the widget is off screen this frame
        libgui_end_frame(u);
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let kept = libgui_animate_with_speed(u, id, 2, 0.5, 20.0);
        libgui_end_frame(u);
        assert_eq!(kept, 0.5, "the animation was forgotten while its widget was away");

        // A repaint asked for with nothing moving.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_request_repaint(u);
        libgui_end_frame(u);
        libgui_frame_platform(u, &mut plat);
        assert_eq!(plat.repaint_after, 0.0, "request_repaint did not wake the host");

        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

const KEY_ENTER: u32 = 11;
const KEY_ESCAPE: u32 = 14;

/// The app's side of a validated field, behind `user`: its grammar (here
/// libgui_units, standing in for vCAD's own) and a parameter table it may
/// change between frames, resolved when asked.
struct App {
    units: *mut LibguiUnits,
    width: f64,
    asked: Vec<String>,
    /// Set to make the validator do what it must not: call libgui.
    misbehave: *mut LibguiUi,
}

unsafe extern "C" fn app_validate(
    user: *mut c_void,
    text: *const std::os::raw::c_char,
    len: u64,
    error: *mut std::os::raw::c_char,
    error_cap: u64,
    error_at: *mut u64,
) -> u8 {
    unsafe {
        let app = &mut *(user as *mut App);
        let t = std::ffi::CStr::from_ptr(text).to_str().unwrap();
        assert_eq!(t.len() as u64, len, "the length and the NUL disagree");
        app.asked.push(t.to_string());
        if !app.misbehave.is_null() {
            libgui_label(app.misbehave, c("from inside a validator").as_ptr());
        }
        let w = c("w");
        let vars = [LibguiVar { name: w.as_ptr(), value: app.width, dim: 1, _pad: 0 }];
        let mut v = 0.0;
        libgui_units_eval(app.units, text, vars.as_ptr(), 1, &mut v, error, error_cap, error_at)
    }
}

/// A validated field from C, used the way a parametric CAD uses it: the
/// buffer keeps the **expression**, the grammar is the app's, names resolve
/// when asked, and nothing is handed back to free.
#[test]
fn a_c_caller_keeps_the_expression_and_its_own_grammar() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_MAC), 0);
        let mut app = App { units: libgui_units_length_mm(), width: 40.0, asked: Vec::new(), misbehave: std::ptr::null_mut() };
        let user = &mut app as *mut App as *mut c_void;

        let mut source = [0 as std::os::raw::c_char; 64];
        let src = c("w / 2");
        std::ptr::copy_nonoverlapping(src.as_ptr(), source.as_mut_ptr(), src.as_bytes().len() + 1);
        let shown = c("20 mm");
        let mut why = [0 as std::os::raw::c_char; 96];
        let mut opts = std::mem::zeroed::<LibguiValidatedOptions>();
        libgui_validated_options_default(&mut opts);
        assert_eq!(opts.select_on_focus, 1, "the defaults did not arrive");
        opts.display = shown.as_ptr();
        opts.error = why.as_mut_ptr();
        opts.error_cap = why.len() as u64;

        let last = std::cell::Cell::new(LibguiValidatedResponse::default());
        let commits = std::cell::Cell::new(0usize);
        let src_ptr = source.as_mut_ptr();
        let step = || {
            frame(u, || {
                let r = libgui_validated_input(u, c("height").as_ptr(), src_ptr, 64, &opts, Some(app_validate), user, std::ptr::null_mut());
                last.set(r);
                commits.set(commits.get() + r.committed as usize);
            });
        };
        let read = |p: *const std::os::raw::c_char| std::ffi::CStr::from_ptr(p).to_str().unwrap().to_string();
        for _ in 0..3 {
            step();
        }
        let rect = last.get().response.rect;
        assert!(rect.w > 0.0, "the field's own rect did not reach C");
        let (x, y) = (rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
        let focus = || {
            libgui_push_pointer_moved(u, x, y);
            libgui_push_pointer_button(u, 0, 1);
            step();
            libgui_push_pointer_button(u, 0, 0);
            step();
        };
        let key = |k: u32| {
            libgui_push_key(u, k, 1, 0);
            step();
            libgui_push_key(u, k, 0, 0);
            step();
        };

        // Editing starts from the source, not the display.
        focus();
        key(KEY_ENTER);
        assert_eq!((*(user as *mut App)).asked.last().map(String::as_str), Some("w / 2"), "the validator was handed the display");
        assert_eq!(read(source.as_ptr()), "w / 2");
        assert_eq!(commits.get(), 0, "an unchanged commit was reported");

        // A new expression is kept as text: the parametric link survives.
        focus();
        libgui_push_text(u, c("w/2 + 3/8\"").as_ptr());
        step();
        key(KEY_ENTER);
        assert_eq!(read(source.as_ptr()), "w/2 + 3/8\"", "the expression was not what the buffer received");
        assert_eq!(commits.get(), 1);
        assert_eq!(read(why.as_ptr()), "");

        // Refused: the source is untouched, the reason and position arrive,
        // and focus stays for the fix.
        focus();
        libgui_push_text(u, c("2w").as_ptr());
        step();
        key(KEY_ENTER);
        let r = last.get();
        assert_eq!(r.has_error, 1);
        assert_eq!(r.error_at, 1, "the validator's position did not arrive");
        assert_eq!(r.focused, 1, "a refused Enter threw the user out");
        assert!(read(why.as_ptr()).contains("use *"), "the reason did not arrive: {}", read(why.as_ptr()));
        assert_eq!(read(source.as_ptr()), "w/2 + 3/8\"", "a refused edit reached the buffer");

        // Fixed, the stale reason goes.
        libgui_push_text(u, c("*").as_ptr());
        step();
        key(KEY_ENTER);
        assert_eq!(read(source.as_ptr()), "2*w");
        assert_eq!(read(why.as_ptr()), "", "the previous refusal's reason was left in the buffer");

        // Escape reports itself and leaves the buffer alone.
        focus();
        libgui_push_text(u, c("999").as_ptr());
        step();
        libgui_push_key(u, KEY_ESCAPE, 1, 0);
        step();
        assert_eq!(last.get().cancelled, 1);
        libgui_push_key(u, KEY_ESCAPE, 0, 0);
        step();
        assert_eq!(read(source.as_ptr()), "2*w");

        // A validator that calls libgui is refused, not undefined behaviour.
        (*(user as *mut App)).misbehave = u;
        focus();
        libgui_push_text(u, c("w").as_ptr());
        step();
        key(KEY_ENTER);
        let err = std::ffi::CStr::from_ptr(libgui_last_error()).to_str().unwrap();
        assert!(err.contains("validator"), "the re-entrant call was not refused: {err}");
        assert_eq!(libgui_ui_poisoned(u), 0);
        assert_eq!(read(source.as_ptr()), "w", "the refusal of the call spoiled the commit");
        (*(user as *mut App)).misbehave = std::ptr::null_mut();
        step();

        // No validator: commit-and-cancel with nothing refused.
        let mut free = [0 as std::os::raw::c_char; 16];
        frame(u, || { libgui_validated_input(u, c("free").as_ptr(), free.as_mut_ptr(), 16, std::ptr::null(), None, std::ptr::null_mut(), std::ptr::null_mut()); });

        // The evaluator on its own, and the formatter.
        let mm = (*(user as *mut App)).units;
        let mut out = 0.0;
        let mut at = 0u64;
        assert_eq!(libgui_units_eval(mm, c("1ft + 1in").as_ptr(), std::ptr::null(), 0, &mut out, why.as_mut_ptr(), 96, &mut at), 1);
        assert!((out - 330.2).abs() < 1e-9);
        assert_eq!(libgui_units_eval(mm, c("12 +").as_ptr(), std::ptr::null(), 0, &mut out, why.as_mut_ptr(), 96, &mut at), 0);
        assert_eq!(at, 4, "the error's position did not arrive");
        let mut small = [0 as std::os::raw::c_char; 4];
        let need = libgui_units_format(mm, 25.4, 3, small.as_mut_ptr(), small.len() as u64);
        assert_eq!(need, "25.4 mm".len() as u64);
        assert_eq!(read(small.as_ptr()), "25.");

        // A table of the app's own.
        let own = libgui_units_new(c("m").as_ptr());
        libgui_units_add(own, c("km").as_ptr(), 1000.0);
        libgui_units_set_display(own, c("km").as_ptr());
        assert_eq!(libgui_units_eval(own, c("2").as_ptr(), std::ptr::null(), 0, &mut out, std::ptr::null_mut(), 0, std::ptr::null_mut()), 1);
        assert_eq!(out, 2000.0);
        assert!(libgui_units_new(std::ptr::null()).is_null());
        let deg = libgui_units_angle_deg();
        let none = libgui_units_none();
        assert_eq!(libgui_units_eval(none, c("6*7").as_ptr(), std::ptr::null(), 0, &mut out, std::ptr::null_mut(), 0, std::ptr::null_mut()), 1);
        assert_eq!(out, 42.0);
        assert_eq!(libgui_units_eval(std::ptr::null(), c("1").as_ptr(), std::ptr::null(), 0, &mut out, std::ptr::null_mut(), 0, std::ptr::null_mut()), 0);
        libgui_units_add(std::ptr::null_mut(), c("x").as_ptr(), 1.0);
        libgui_units_free(std::ptr::null_mut());
        for t in [mm, own, deg, none] {
            libgui_units_free(t);
        }
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// A paste longer than the caller's buffer arrives whole, through
/// `libgui_text_overflow` — and a second call to the field, the tempting fix,
/// is shown to lose it, which is why the header forbids it.
#[test]
fn a_paste_that_outgrows_the_buffer_is_not_lost() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_MAC), 0);
        let mut buf = vec![0 as std::os::raw::c_char; 8];
        let mut rect = LibguiRect::default();
        let mut id = 0u64;
        let mut need = 0u64;
        let run = |buf: &mut Vec<std::os::raw::c_char>, need: &mut u64| {
            let r = libgui_text_input(u, c("path").as_ptr(), buf.as_mut_ptr(), buf.len() as u64, std::ptr::null(), need);
            (r.response.rect, r.response.id)
        };
        for _ in 0..3 {
            frame(u, || (rect, id) = run(&mut buf, &mut need));
        }
        libgui_push_pointer_moved(u, rect.x + 4.0, rect.y + 4.0);
        libgui_push_pointer_button(u, 0, 1);
        frame(u, || { run(&mut buf, &mut need); });
        libgui_push_pointer_button(u, 0, 0);
        frame(u, || { run(&mut buf, &mut need); });

        let long = "/Users/someone/Projects/bracket-v3.step";
        libgui_push_paste(u, c(long).as_ptr());
        let fetched: String;
        let retried: String;
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        run(&mut buf, &mut need);
        assert_eq!(need, long.len() as u64, "the length the text is did not arrive");
        assert!(need >= buf.len() as u64, "the test did not overflow its buffer");
        {
            let mut big = vec![0 as std::os::raw::c_char; need as usize + 1];
            let got = libgui_text_overflow(u, id, big.as_mut_ptr(), big.len() as u64);
            assert_eq!(got, need);
            fetched = std::ffi::CStr::from_ptr(big.as_ptr()).to_str().unwrap().to_string();

            // The wrong way, for contrast: the field again, with room.
            let mut again = vec![0 as std::os::raw::c_char; need as usize + 1];
            std::ptr::copy_nonoverlapping(buf.as_ptr(), again.as_mut_ptr(), buf.len());
            let mut n2 = 0;
            libgui_text_input(u, c("path").as_ptr(), again.as_mut_ptr(), again.len() as u64, std::ptr::null(), &mut n2);
            retried = std::ffi::CStr::from_ptr(again.as_ptr()).to_str().unwrap().to_string();
        }
        libgui_end_frame(u);
        assert_eq!(fetched, long, "the overflow did not hold the whole paste");
        assert_ne!(retried, long, "calling the field twice worked after all; the header is wrong to forbid it");

        // It is this frame's only: the next frame starts empty.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        let mut small = [0 as std::os::raw::c_char; 4];
        assert_eq!(libgui_text_overflow(u, id, small.as_mut_ptr(), 4), 0, "the overflow outlived its frame");
        assert_eq!(small[0], 0, "nothing to fetch still has to leave an empty string");
        libgui_end_frame(u);

        // Nulls.
        assert_eq!(libgui_text_overflow(std::ptr::null_mut(), id, std::ptr::null_mut(), 0), 0);
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// What a validator calls on its own handle while libgui holds `&mut Ui`
/// across the callback. The frame-level calls — ending or beginning a frame,
/// freeing the handle, showing a dock or a table, sharing its fonts — do not
/// go through the per-widget guard, so they need refusing on their own:
/// ending the frame from inside a validator would lay out and close a tree
/// that the field is still in the middle of building.
static FRAME_CALL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

unsafe extern "C" fn validator_that_ends_the_frame(
    user: *mut c_void,
    _text: *const std::os::raw::c_char,
    _len: u64,
    _error: *mut std::os::raw::c_char,
    _error_cap: u64,
    _error_at: *mut u64,
) -> u8 {
    let u = user as *mut LibguiUi;
    unsafe {
        match FRAME_CALL.load(std::sync::atomic::Ordering::Relaxed) {
            0 => libgui_end_frame(u),
            1 => libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0),
            2 => {
                let other = libgui_ui_new_sharing_fonts(u);
                if !other.is_null() {
                    libgui_ui_free(other);
                }
            }
            _ => libgui_ui_free(u),
        }
    }
    1
}

#[test]
fn a_validator_cannot_end_begin_share_or_free_the_frame_it_runs_in() {
    for (n, what) in ["libgui_end_frame", "libgui_begin_frame", "libgui_ui_new_sharing_fonts", "libgui_ui_free"]
        .iter()
        .enumerate()
    {
        FRAME_CALL.store(n as u8, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            let u = ui();
            assert_eq!(libgui_install_keymap(u, PLATFORM_MAC), 0);
            let mut buf = [0 as std::os::raw::c_char; 32];
            let buf_ptr = buf.as_mut_ptr();
            let last = std::cell::Cell::new(LibguiValidatedResponse::default());
            let step = || {
                frame(u, || {
                    last.set(libgui_validated_input(
                        u,
                        c("f").as_ptr(),
                        buf_ptr,
                        32,
                        std::ptr::null(),
                        Some(validator_that_ends_the_frame),
                        u as *mut c_void,
                        std::ptr::null_mut(),
                    ));
                });
            };
            for _ in 0..3 {
                step();
            }
            let r = last.get().response.rect;
            libgui_push_pointer_moved(u, r.x + r.w / 2.0, r.y + r.h / 2.0);
            libgui_push_pointer_button(u, 0, 1);
            step();
            libgui_push_pointer_button(u, 0, 0);
            step();
            libgui_push_text(u, c("7").as_ptr());
            step();
            libgui_push_key(u, KEY_ENTER, 1, 0);
            step(); // the commit: the validator runs here

            let err = std::ffi::CStr::from_ptr(libgui_last_error()).to_string_lossy().into_owned();
            assert!(err.contains("validator"), "{what} from inside a validator was not refused: {err:?}");
            assert_eq!(libgui_ui_poisoned(u), 0, "{what} from inside a validator poisoned the handle");
            let got = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
            assert_eq!(got, "7", "{what}: the refusal spoiled the commit");
            libgui_ui_free(u);
        }
    }
}

/// An icon's alpha has to be able to count. The mode reaches the instance —
/// `params[1]` of an image, which is what every backend reads — and an
/// unknown value is opaque, the one reading that never makes part of an
/// image disappear.
#[test]
fn an_image_draw_says_how_to_read_its_alpha() {
    unsafe extern "C" fn paint(p: *mut LibguiPainter, r: LibguiRect, _user: *mut c_void) {
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        for mode in [0u32, 1, 2, 99] {
            unsafe { libgui_painter_image_alpha(p, r, 5, 0.0, 0.0, 1.0, 1.0, 0.0, white, mode) };
        }
    }
    unsafe {
        let u = ui();
        let layout = LibguiLayout {
            axis: 1,
            _pad: [0; 3],
            width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 32.0 },
            height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 32.0 },
            pad_left: 0.0,
            pad_right: 0.0,
            pad_top: 0.0,
            pad_bottom: 0.0,
            gap: 0.0,
            align_main: 0,
            align_cross: 0,
            _pad2: [0; 2],
        };
        frame(u, || {
            libgui_add_leaf(
                u,
                libgui_id_from_name(c("icons").as_ptr()),
                layout,
                0,
                LibguiPaintFn { paint: Some(paint), drop_user: None, user: std::ptr::null_mut() },
            );
        });
        let mut n = 0u64;
        let p = libgui_frame_instances(u, &mut n) as *const libgui::Instance;
        let images: Vec<f32> = std::slice::from_raw_parts(p, n as usize)
            .iter()
            .filter(|i| i.params[3] == libgui::render_contract::PrimitiveKind::Image.code())
            .map(|i| i.params[1])
            .collect();
        assert_eq!(images, vec![0.0, 1.0, 2.0, 0.0], "the alpha modes did not reach the instances");
        libgui_ui_free(u);
    }
}

/// An icon from C is the same icon as from Rust: the verbs and points become
/// the same outline, rasterised into the same atlas slot and drawn as the
/// same glyph instance. And a malformed path draws nothing at all rather than
/// part of a shape.
#[test]
fn a_path_filled_from_c_is_the_path_filled_from_rust() {
    // A play triangle and a curved notch: every verb kind.
    const VERBS: [u8; 7] = [0, 1, 1, 4, 0, 2, 3];
    // MOVE 1 + LINE 1 + LINE 1 + MOVE 1 + QUAD 2 + CUBIC 3 = 9 points.
    const POINTS: [f32; 18] = [6.0, 4.0, 20.0, 12.0, 6.0, 20.0, 2.0, 2.0, 4.0, 1.0, 6.0, 2.0, 5.0, 4.0, 3.0, 4.0, 2.0, 2.0];
    static MALFORMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    unsafe extern "C" fn paint(p: *mut LibguiPainter, r: LibguiRect, _user: *mut c_void) {
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let n_points = if MALFORMED.load(std::sync::atomic::Ordering::Relaxed) { 5 } else { 9 };
        unsafe { libgui_painter_fill_path(p, r, 24.0, 24.0, VERBS.as_ptr(), 7, POINTS.as_ptr(), n_points, 1, white) };
    }

    let layout = LibguiLayout {
        axis: 1,
        _pad: [0; 3],
        width: LibguiSize { kind: LibguiSizeKind::Fixed, value: 48.0 },
        height: LibguiSize { kind: LibguiSizeKind::Fixed, value: 48.0 },
        pad_left: 0.0,
        pad_right: 0.0,
        pad_top: 0.0,
        pad_bottom: 0.0,
        gap: 0.0,
        align_main: 0,
        align_cross: 0,
        _pad2: [0; 2],
    };
    let draw_c = |malformed: bool| unsafe {
        MALFORMED.store(malformed, std::sync::atomic::Ordering::Relaxed);
        let u = ui();
        for _ in 0..2 {
            libgui_begin_frame(u, 200.0, 200.0, 1.5, 1.0 / 60.0);
            libgui_add_leaf(
                u,
                libgui_id_from_name(c("icon").as_ptr()),
                layout,
                0,
                LibguiPaintFn { paint: Some(paint), drop_user: None, user: std::ptr::null_mut() },
            );
            libgui_end_frame(u);
        }
        // Null when nothing has gone wrong, which is the case worth handling.
        let e = libgui_last_error();
        let err = if e.is_null() { String::new() } else { std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned() };
        let mut n = 0u64;
        let p = libgui_frame_instances(u, &mut n) as *const u8;
        let bytes = std::slice::from_raw_parts(p, n as usize * libgui_instance_stride() as usize).to_vec();
        libgui_ui_free(u);
        (bytes, err)
    };

    let from_rust = {
        let path = libgui::Path::new(24.0, 24.0)
            .move_to(libgui::Vec2::new(6.0, 4.0))
            .line_to(libgui::Vec2::new(20.0, 12.0))
            .line_to(libgui::Vec2::new(6.0, 20.0))
            .close()
            .move_to(libgui::Vec2::new(2.0, 2.0))
            .quad_to(libgui::Vec2::new(4.0, 1.0), libgui::Vec2::new(6.0, 2.0))
            .cubic_to(libgui::Vec2::new(5.0, 4.0), libgui::Vec2::new(3.0, 4.0), libgui::Vec2::new(2.0, 2.0))
            .fill_rule(libgui::FillRule::EvenOdd);
        let mut ui = libgui::Ui::new(libgui::Theme::dark(), FONT).expect("font");
        let info = libgui::FrameInfo { screen_size: libgui::Vec2::new(200.0, 200.0), scale: 1.5, dt: 1.0 / 60.0 };
        let mut bytes = Vec::new();
        for _ in 0..2 {
            ui.begin_frame(info);
            let p = path.clone();
            ui.add_leaf(
                libgui::Id::from_name("icon"),
                libgui::Layout::leaf(libgui::Size::Fixed(48.0), libgui::Size::Fixed(48.0)),
                libgui::Vec2::ZERO,
                false,
                move |pt, r| pt.fill_path(&p, r, libgui::Color::WHITE),
            );
            let out = ui.end_frame();
            let inst = out.draw.instances.as_slice();
            bytes = unsafe { std::slice::from_raw_parts(inst.as_ptr() as *const u8, std::mem::size_of_val(inst)) }.to_vec();
        }
        bytes
    };

    let (from_c, _) = draw_c(false);
    assert!(!from_c.is_empty(), "the path drew nothing");
    assert_eq!(from_c, from_rust, "the outline from C is not the one from Rust");

    // Five points where nine are needed: refused whole, and said so.
    let (broken, err) = draw_c(true);
    assert!(broken.is_empty(), "a malformed path drew part of a shape");
    assert!(err.contains("fill_path"), "a malformed path was refused silently: {err:?}");
}

/// Springs from C: the theme's spring when given zeros, an explicit one
/// otherwise, a release velocity that carries, and the pointer's speed.
#[test]
fn springs_cross_to_c() {
    unsafe {
        let u = ui();
        let id = libgui_id_from_name(c("puck").as_ptr());
        let step = |target: f32, response: f32, damping: f32| {
            libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
            let v = libgui_animate_spring(u, id, 0, target, response, damping);
            libgui_end_frame(u);
            v
        };
        assert_eq!(step(0.0, 0.0, 0.0), 0.0, "a new spring starts at its target");
        let moving = step(100.0, 0.0, 0.0);
        assert!(moving > 0.0 && moving < 100.0, "the theme's spring did not move: {moving}");
        assert!(libgui_spring_velocity(u, id, 0) > 0.0);

        // Thrown hard toward 100 with a bouncy spring: it sails past.
        libgui_set_spring(u, id, 0, 0.0, 4000.0);
        let mut furthest = 0.0f32;
        for _ in 0..60 {
            furthest = furthest.max(step(100.0, 0.4, 0.5));
        }
        assert!(furthest > 115.0, "a 4000 px/s throw only reached {furthest}");

        // The pointer, moving 8 px a frame at 60 Hz.
        let mut x = 0.0;
        for _ in 0..30 {
            x += 8.0;
            libgui_push_pointer_moved(u, x, 10.0);
            libgui_begin_frame(u, 800.0, 300.0, 1.0, 1.0 / 60.0);
            libgui_end_frame(u);
        }
        let (mut vx, mut vy) = (0.0f32, 0.0f32);
        libgui_pointer_velocity(u, &mut vx, &mut vy);
        assert!((vx - 480.0).abs() < 10.0 && vy.abs() < 1.0, "measured ({vx}, {vy}) for 480 px/s");
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// After `end_frame`, `libgui_rect_of` is where the view was drawn **this**
/// frame — what a host sizes its scene from — while the response's rect is
/// last frame's, which is the one-frame lag the header warns about. And at a
/// fractional scale the rect is whole physical pixels, so `w * scale` needs
/// no rounding of its own.
#[test]
fn rect_of_is_this_frames_rect_and_on_the_pixel_grid() {
    unsafe {
        let u = ui();
        let scale = 1.5f32;
        let mut width = 300.0f32;
        let step = |width: f32| -> (LibguiResponse, LibguiRect, LibguiRect) {
            libgui_begin_frame(u, 600.0, 300.0, scale, 1.0 / 60.0);
            let none = LibguiFrame {
                fill: LibguiColor::default(),
                border: LibguiColor::default(),
                border_width: 0.0,
                radius: 0.0,
                clip: 0,
                shadow: 0,
                _pad: [0; 6],
            };
            libgui_open_container(u, 9, leaf_layout(width, 200.0), none);
            let vp = libgui_viewport(u, c("scene").as_ptr(), 77);
            libgui_close_container(u);
            libgui_end_frame(u);
            let mut now = LibguiRect::default();
            assert_eq!(libgui_rect_of(u, vp.id, &mut now), 1, "a widget built this frame has no rect");
            // Where the texture was drawn: the instance its batch starts at.
            let mut nb = 0u64;
            let batches = std::slice::from_raw_parts(libgui_frame_batches(u, &mut nb), nb as usize);
            let b = batches.iter().find(|b| b.texture_kind == 1 && b.texture_index == 77).expect("the view was not drawn");
            let mut ni = 0u64;
            let base = libgui_frame_instances(u, &mut ni) as *const f32;
            let stride = libgui_instance_stride() as usize / 4;
            let r = std::slice::from_raw_parts(base.add(b.first as usize * stride), 4);
            (vp, now, LibguiRect { x: r[0], y: r[1], w: r[2], h: r[3] })
        };
        for _ in 0..3 {
            step(width);
        }
        // The view's container moves by a fraction this frame.
        width -= 3.37;
        let (vp, now, drawn) = step(width);
        assert_eq!((now.w, now.h), (drawn.w, drawn.h), "rect_of is not where the view was drawn");
        assert_ne!(vp.rect.w, now.w, "the response caught up already: the test did not move the view");
        let phys = now.w * scale;
        assert_eq!(phys, phys.round(), "the rect is not whole physical pixels: {phys}");

        // An id that was not built, and nulls.
        assert_eq!(libgui_rect_of(u, 0xdead, &mut LibguiRect::default()), 0);
        assert_eq!(libgui_rect_of(u, vp.id, std::ptr::null_mut()), 1);
        assert_eq!(libgui_rect_of(std::ptr::null_mut(), vp.id, std::ptr::null_mut()), 0);
        libgui_ui_free(u);
    }
}

/// `axis` 0 runs the ramp across, 1 runs it down: read back from the ramp
/// instance's uv, which spans the atlas along the gradient and is a single
/// point across it.
#[test]
fn a_c_gradient_runs_the_way_its_axis_says() {
    unsafe extern "C" fn paint(p: *mut LibguiPainter, r: LibguiRect, user: *mut c_void) {
        let axis = unsafe { *(user as *const u32) };
        let black = LibguiColor { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
        unsafe { libgui_painter_gradient(p, r, LibguiColor::default(), black, axis) };
    }
    unsafe {
        for mut axis in [0u32, 1] {
            let u = ui();
            let id = libgui_id_from_name(c("ramp").as_ptr());
            libgui_begin_frame(u, 200.0, 200.0, 1.0, 1.0 / 60.0);
            let cb = LibguiPaintFn { paint: Some(paint), drop_user: None, user: &mut axis as *mut u32 as *mut c_void };
            libgui_add_leaf(u, id, leaf_layout(100.0, 50.0), 0, cb);
            libgui_end_frame(u);
            let mut n = 0u64;
            let base = libgui_frame_instances(u, &mut n) as *const f32;
            let stride = libgui_instance_stride() as usize / 4;
            assert_eq!(n, 1, "a gradient from transparent is one instance");
            let inst = std::slice::from_raw_parts(base, stride);
            let uv = &inst[4..8];
            let (across, down) = (uv[0] != uv[2], uv[1] != uv[3]);
            assert_eq!((across, down), (axis == 0, axis == 1), "axis {axis} ran the other way: uv {uv:?}");
            libgui_ui_free(u);
        }
    }
}

/// A click in the picker's square writes through to the host's own colour,
/// and the drag that ends is the one `finished` names.
#[test]
fn a_c_colour_picker_edits_the_hosts_colour() {
    unsafe {
        let u = ui();
        let mut color = LibguiColor { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let mut opts = std::mem::zeroed::<LibguiColorPickerOptions>();
        libgui_color_picker_options_default(&mut opts);
        assert_eq!((opts.alpha, opts.hex), (1, 1));
        let finished = std::cell::Cell::new(0);
        let step = |color: &mut LibguiColor| {
            frame(u, || {
                libgui_open_container(u, 5, leaf_layout(240.0, 300.0), std::mem::zeroed());
                let r = libgui_color_picker(u, c("layer").as_ptr(), color, &opts);
                finished.set(finished.get() + r.finished as u32);
                libgui_close_container(u);
            });
        };
        for _ in 0..3 {
            step(&mut color);
        }
        libgui_push_pointer_moved(u, 1.0, 1.0); // the square's top-left: white
        libgui_push_pointer_button(u, 0, 1);
        step(&mut color);
        libgui_push_pointer_button(u, 0, 0);
        step(&mut color);
        assert!(color.r > 0.98 && color.g > 0.98 && color.b > 0.98, "the click did not reach the host's colour: {color:?}");
        assert_eq!(finished.get(), 1, "one click, one finished edit");

        // The swatch, and nulls.
        frame(u, || {
            libgui_color_button(u, c("swatch").as_ptr(), &mut color, std::ptr::null());
        });
        let r = libgui_color_picker(u, c("x").as_ptr(), std::ptr::null_mut(), std::ptr::null());
        assert_eq!(r.changed, 0);
        libgui_color_picker_options_default(std::ptr::null_mut());
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

const KEY_TAB: u32 = 13;
const KEY_SPACE: u32 = 15;
const KEY_DOWN: u32 = 4;
const PLATFORM_WINDOWS: i32 = 1;

unsafe fn key(u: *mut LibguiUi, k: u32, mut frame: impl FnMut()) {
    unsafe {
        libgui_push_key(u, k, 1, 0);
        if k == KEY_SPACE {
            libgui_push_text(u, c(" ").as_ptr());
        }
        frame();
        libgui_push_key(u, k, 0, 0);
        frame();
    }
}

/// The keyboard from C, through the keymap a C host installs: Space presses
/// a focused button, and a menu is opened, walked and chosen from.
#[test]
fn a_c_host_gets_buttons_and_menus_from_the_keyboard() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_WINDOWS), 0);
        let clicks = std::cell::Cell::new(0);
        let chosen = std::cell::RefCell::new(Vec::<&str>::new());
        let step = || {
            frame(u, || {
                if libgui_button(u, c("OK").as_ptr()).clicked != 0 {
                    clicks.set(clicks.get() + 1);
                }
                if libgui_open_menu(u, c("File").as_ptr()) != 0 {
                    for name in ["New", "Open", "Quit"] {
                        if libgui_menu_item(u, c(name).as_ptr()).clicked != 0 {
                            chosen.borrow_mut().push(name);
                        }
                    }
                    libgui_close_menu(u);
                }
            })
        };
        step();
        key(u, KEY_TAB, step);
        key(u, KEY_SPACE, step);
        assert_eq!(clicks.get(), 1, "Space did not press the focused C button");
        key(u, KEY_TAB, step); // to File
        key(u, KEY_SPACE, step); // opens on New
        key(u, KEY_DOWN, step); // Open
        key(u, KEY_SPACE, step);
        assert_eq!(*chosen.borrow(), ["Open"], "the C menu was not walked from the keyboard");
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

struct Labels {
    names: Vec<CString>,
    misbehave: *mut LibguiUi,
}

unsafe extern "C" fn label_of(user: *mut c_void, i: u64) -> *const std::os::raw::c_char {
    unsafe {
        let l = &*(user as *const Labels);
        if !l.misbehave.is_null() {
            libgui_label(l.misbehave, c("from a label callback").as_ptr());
        }
        l.names.get(i as usize).map(|n| n.as_ptr()).unwrap_or(std::ptr::null())
    }
}

/// Type-ahead from C, the labels through a callback; Shift+Down extends; and a
/// label callback that calls libgui is refused rather than undefined.
#[test]
fn a_c_list_has_type_ahead_and_extends_with_shift() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_WINDOWS), 0);
        let mut labels = Labels {
            names: ["Axle", "bearing", "Bolt", "Bracket", "Bushing"].iter().map(|s| c(s)).collect(),
            misbehave: std::ptr::null_mut(),
        };
        let user = &mut labels as *mut Labels as *mut c_void;
        let (cursor, extend) = (std::cell::Cell::new(0u64), std::cell::Cell::new(0u8));
        let step = || {
            frame(u, || {
                libgui_open_collection(u, c("parts").as_ptr(), 5);
                libgui_type_ahead(u, 5, Some(label_of), user);
                cursor.set(libgui_nav_cursor());
                extend.set(libgui_nav_extend());
                for i in 0..5u64 {
                    libgui_selectable_keyed(u, i, c("row").as_ptr(), (cursor.get() == i) as u8);
                }
                libgui_close_collection(u);
            })
        };
        step();
        key(u, KEY_TAB, step);
        libgui_push_text(u, c("br").as_ptr());
        step();
        assert_eq!(cursor.get(), 3, "br did not reach Bracket from C");

        libgui_push_modifiers(u, LibguiModifiers { shift: 1, ctrl: 0, alt: 0, logo: 0 });
        libgui_push_key(u, KEY_DOWN, 1, 0);
        step();
        assert_eq!((cursor.get(), extend.get()), (4, 1), "Shift+Down did not extend from C");
        libgui_push_key(u, KEY_DOWN, 0, 0);
        libgui_push_modifiers(u, LibguiModifiers::default());
        step();

        (*(user as *mut Labels)).misbehave = u;
        libgui_push_text(u, c("a").as_ptr());
        step();
        let err = std::ffi::CStr::from_ptr(libgui_last_error()).to_str().unwrap();
        assert!(err.contains("label callback"), "a label callback calling libgui was not refused: {err}");
        assert_eq!(libgui_ui_poisoned(u), 0);
        (*(user as *mut Labels)).misbehave = std::ptr::null_mut();
        libgui_type_ahead(u, 5, None, std::ptr::null_mut());
        libgui_ui_free(u);
    }
}

/// A C app's tree: 50 assemblies of 200 parts. Keys: assembly `a` is `a`,
/// part `p` of it is `1000 + a * 200 + p`.
struct CTree {
    names: std::collections::HashMap<u64, CString>,
    children_calls: u32,
    misbehave: *mut LibguiUi,
}

unsafe extern "C" fn ct_children(user: *mut c_void, node: u64, out: *mut u64, cap: u64) -> u64 {
    unsafe {
        let t = &mut *(user as *mut CTree);
        t.children_calls += 1;
        if !t.misbehave.is_null() {
            libgui_label(t.misbehave, c("from a tree callback").as_ptr());
        }
        let kids: Vec<u64> = if node == LIBGUI_TREE_ROOT {
            (0..50).collect()
        } else if node < 50 {
            (0..200).map(|p| 1000 + node * 200 + p).collect()
        } else {
            Vec::new()
        };
        for (i, k) in kids.iter().take(cap as usize).enumerate() {
            *out.add(i) = *k;
        }
        kids.len() as u64
    }
}
unsafe extern "C" fn ct_has(_: *mut c_void, node: u64) -> u8 {
    (node < 50) as u8
}
unsafe extern "C" fn ct_label(user: *mut c_void, node: u64) -> *const std::os::raw::c_char {
    unsafe {
        let t = &mut *(user as *mut CTree);
        t.names
            .entry(node)
            .or_insert_with(|| if node < 50 { c(&format!("Assembly {node:02}")) } else { c(&format!("Part {node}")) })
            .as_ptr()
    }
}
unsafe extern "C" fn ct_parent(_: *mut c_void, node: u64) -> u64 {
    if node < 50 { LIBGUI_TREE_ROOT } else { (node - 1000) / 200 }
}

#[test]
fn a_c_tree_builds_a_screenful_of_a_large_tree() {
    unsafe {
        let u = ui();
        assert_eq!(libgui_install_keymap(u, PLATFORM_WINDOWS), 0);
        let mut ct = CTree { names: Default::default(), children_calls: 0, misbehave: std::ptr::null_mut() };
        let user = &mut ct as *mut CTree as *mut c_void;
        let src = LibguiTreeSource { children: Some(ct_children), has_children: Some(ct_has), label: Some(ct_label), selected: None, user };
        let tree = libgui_tree_new();
        let last = std::cell::Cell::new(LibguiTreeViewResponse::default());
        let step = || {
            frame(u, || last.set(libgui_tree_view(u, c("assembly").as_ptr(), tree, &src)));
        };
        step();
        assert_eq!(last.get().rows, 50);

        // Every assembly open: 10,050 rows, a screenful built, and each list
        // of 200 children read whole though the first ask had room for 64.
        for a in 0..50 {
            libgui_tree_expand(tree, a);
        }
        step();
        let r = last.get();
        assert_eq!(r.rows, 50 + 50 * 200, "a child list longer than the first buffer was cut short");
        assert!(r.built_end - r.built_first < 40, "{} rows built", r.built_end - r.built_first);
        let calls = (*(user as *mut CTree)).children_calls;
        step();
        step();
        assert_eq!((*(user as *mut CTree)).children_calls, calls, "a steady frame asked for children");

        // The keyboard: Tab in, Left on an open assembly closes it.
        key(u, KEY_TAB, step);
        let mut cur = u64::MAX;
        assert_eq!(libgui_tree_cursor(tree, &mut cur), 1);
        assert_eq!(cur, 0);
        libgui_push_key(u, 1, 1, 0); // ArrowLeft
        step();
        assert_eq!(last.get().has_collapsed, 1, "Left did not close the open assembly");
        assert_eq!(libgui_tree_is_expanded(tree, 0), 0);
        libgui_push_key(u, 1, 0, 0);
        step();

        // Reveal a part deep in the tree, through the parent callback.
        libgui_tree_collapse(tree, 37);
        libgui_tree_reveal(tree, 1000 + 37 * 200 + 150, Some(ct_parent), user);
        for _ in 0..90 {
            step();
        }
        assert_eq!(libgui_tree_is_expanded(tree, 37), 1, "reveal did not open the part's assembly");
        assert_eq!(libgui_tree_cursor(tree, &mut cur), 1);
        assert_eq!(cur, 1000 + 37 * 200 + 150);

        // Selection as keys, with the anchor kept by key.
        let mut out = [0u64; 8];
        let mut kind = -1;
        assert_eq!(libgui_tree_select(tree, 5, 0, out.as_mut_ptr(), 8, &mut kind), 1);
        assert_eq!((kind, out[0]), (0, 5));
        // A range is every row *shown* between the two: assemblies 5..=7 are
        // open, so their 600 parts are in it, and only `cap` are written.
        assert_eq!(libgui_tree_select(tree, 8, 2, out.as_mut_ptr(), 8, &mut kind), 4 + 3 * 200);
        assert_eq!((kind, &out[..3]), (2, &[5u64, 1000 + 5 * 200, 1000 + 5 * 200 + 1][..]), "the range does not run through the open assembly");

        // A callback that calls libgui is refused.
        (*(user as *mut CTree)).misbehave = u;
        libgui_tree_invalidate(tree);
        step();
        let err = std::ffi::CStr::from_ptr(libgui_last_error()).to_str().unwrap();
        assert!(err.contains("callback"), "a tree callback calling libgui was not refused: {err}");
        assert_eq!(libgui_ui_poisoned(u), 0);
        (*(user as *mut CTree)).misbehave = std::ptr::null_mut();

        libgui_tree_toggle(tree, 3);
        assert_eq!(libgui_tree_is_expanded(tree, 3), 0);
        // Nulls.
        let r = libgui_tree_view(u, c("x").as_ptr(), std::ptr::null_mut(), &src);
        assert_eq!(r.rows, 0);
        libgui_tree_expand(std::ptr::null_mut(), 1);
        libgui_tree_free(std::ptr::null_mut());
        libgui_tree_free(tree);
        libgui_ui_free(u);
    }
}

/// A notification from C: queued between frames, its action pressed through
/// the host's pointer, and an idle wait that lets the host sleep until the
/// next one is due.
#[test]
fn a_c_toast_reports_its_action_and_sleeps_until_due() {
    unsafe {
        let u = ui();
        let mut opts = std::mem::zeroed::<LibguiToastOptions>();
        libgui_toast_options_default(&mut opts);
        assert_eq!((opts.corner, opts.max_visible), (0, 4));
        let undo = libgui_toast(u, 0, c("Deleted 3 parts").as_ptr(), -1.0, c("Undo").as_ptr());
        assert_ne!(undo, 0);
        let timed = libgui_toast(u, 1, c("Saved").as_ptr(), 3.0, std::ptr::null());
        let error = libgui_toast(u, 3, c("Disk full").as_ptr(), -1.0, std::ptr::null());
        assert_eq!(libgui_toast_count(u), 3);
        let mut resp = LibguiToastResponse::default();
        let mut step = |u| {
            frame(u, || {
                let r = libgui_show_toasts(u, &opts);
                resp.action |= r.action;
                resp.closed |= r.closed;
            })
        };
        for _ in 0..60 {
            step(u);
        }
        let mut plat = std::mem::zeroed::<LibguiPlatformOutput>();
        libgui_frame_platform(u, &mut plat);
        assert!(plat.repaint_after > 1.0 && plat.repaint_after < 4.0, "an idle stack woke the host in {} s", plat.repaint_after);

        let id = libgui::Id::new(("libgui_toast", undo)).with("action").0;
        let mut b = LibguiRect::default();
        assert_eq!(libgui_rect_of(u, id, &mut b), 1, "no action button");
        libgui_push_pointer_moved(u, b.x + b.w / 2.0, b.y + b.h / 2.0);
        libgui_push_pointer_button(u, 0, 1);
        step(u);
        libgui_push_pointer_button(u, 0, 0);
        step(u);
        assert_eq!(resp.action, undo, "the action did not reach C");

        libgui_dismiss_toast(u, timed);
        assert_eq!(libgui_toast_count(u), 1, "only the error should be left");
        libgui_dismiss_toast(u, error);
        assert_eq!(libgui_toast_count(u), 0);

        // A host's own timer.
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0);
        libgui_request_repaint_in(u, 7.5);
        libgui_end_frame(u);
        libgui_frame_platform(u, &mut plat);
        assert!(plat.repaint_after > 0.0 && plat.repaint_after <= 7.5, "{}", plat.repaint_after);

        // Nulls.
        libgui_show_toasts(u, std::ptr::null());
        libgui_toast_options_default(std::ptr::null_mut());
        assert_eq!(libgui_toast(std::ptr::null_mut(), 0, c("x").as_ptr(), -1.0, std::ptr::null()), 0);
        libgui_dismiss_toast(std::ptr::null_mut(), 1);
        libgui_request_repaint_in(std::ptr::null_mut(), 1.0);
        assert_eq!(libgui_toast_count(std::ptr::null_mut()), 0);
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// What a C caller could not reach before: a custom widget that is a Tab
/// stop, focus itself, raw key and button state, and the rest of the gaps a
/// parity sweep found against the Rust API.
#[test]
fn a_c_custom_widget_is_a_focus_stop_and_reads_the_keyboard() {
    unsafe {
        let u = ui();
        // Tab means "next focus stop" only because a keymap says so — and on
        // a Mac by default it skips controls, so ask for Windows rules.
        assert_eq!(libgui_install_keymap(u, PLATFORM_WINDOWS), 0);
        let knob = 77u64;
        let mut seen_focus = false;
        let mut tab_pressed = false;
        for pass in 0..4 {
            if pass == 2 {
                libgui_push_key(u, KEY_TAB, 1, std::mem::zeroed());
            }
            frame(u, || {
                libgui_add_leaf(u, knob, leaf_layout(40.0, 40.0), 1, LibguiPaintFn { paint: None, drop_user: None, user: std::ptr::null_mut() });
                let r = libgui_interact_focusable(u, knob, 0);
                seen_focus |= r.focused != 0;
                tab_pressed |= libgui_key_pressed(u, KEY_TAB as u64) != 0;
            });
        }
        assert!(tab_pressed, "a pressed key was not visible to C");
        libgui_push_key(u, KEY_TAB, 0, 0);
        assert!(seen_focus && libgui_focused(u) == knob, "Tab did not reach a focusable C widget");
        libgui_set_focus(u, 0);
        assert_eq!(libgui_focused(u), 0);
        libgui_set_focus(u, knob);
        assert_eq!(libgui_focused(u), knob);

        // Pointer state, hover time and a drag sense.
        libgui_push_pointer_moved(u, 20.0, 20.0);
        libgui_push_pointer_button(u, 0, 1);
        let (mut down, mut pressed, mut active) = (0, 0, 0);
        frame(u, || {
            libgui_add_leaf(u, knob, leaf_layout(40.0, 40.0), 1, LibguiPaintFn { paint: None, drop_user: None, user: std::ptr::null_mut() });
            active = libgui_interact_drag(u, knob).active;
            down = libgui_pointer_button_down(u, 0);
            pressed = libgui_pointer_button_pressed(u, 0);
            assert_eq!(libgui_key_down(u, KEY_TAB as u64), 0);
        });
        assert_eq!((down, pressed, active), (1, 1, 1), "the press did not reach C");
        libgui_push_pointer_button(u, 0, 0);
        for _ in 0..30 {
            frame(u, || {
                libgui_add_leaf(u, knob, leaf_layout(40.0, 40.0), 1, LibguiPaintFn { paint: None, drop_user: None, user: std::ptr::null_mut() });
                libgui_interact(u, knob);
            });
        }
        let mut rest = 0.0;
        frame(u, || {
            libgui_add_leaf(u, knob, leaf_layout(40.0, 40.0), 1, LibguiPaintFn { paint: None, drop_user: None, user: std::ptr::null_mut() });
            rest = libgui_hover_time(u, knob);
        });
        assert!(rest > 0.3, "hover time {rest}");

        // The widgets that had no C name.
        let mut drag = 5.0f32;
        let mut pane = 200.0f32;
        let trace = [1.0f32, 3.0, 2.0];
        frame(u, || {
            let r = libgui_radio(u, c("Metric").as_ptr(), 1);
            assert_eq!(r.clicked, 0);
            libgui_drag_value_range(u, c("Depth").as_ptr(), &mut drag, 1.0, 0.0, 10.0);
            libgui_splitter(u, c("side").as_ptr(), &mut pane, 1, 0, 100.0, 300.0);
            libgui_plot(u, c("ms").as_ptr(), trace.as_ptr(), 3, 4.0, 40.0);
            libgui_plot(u, c("none").as_ptr(), std::ptr::null(), 0, 1.0, 20.0);
            libgui_insertion_line(u, knob, 0, 1);
            libgui_request_pointer_lock(u);
        });
        assert_eq!((drag, pane), (5.0, 200.0), "untouched widgets wrote their values");

        // A drag from outside the app.
        libgui_begin_external_drag(u, c("file").as_ptr(), 9, c("part.step").as_ptr());
        frame(u, || {
            assert!(!libgui_dragging(u).is_null(), "the outside drag is not visible");
            libgui_drag_ghost(u);
        });
        libgui_end_external_drag(u, 0);
        frame(u, || assert!(libgui_dragging(u).is_null()));
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// A virtual list from C builds only the window it reports.
#[test]
fn a_c_virtual_list_builds_only_what_shows() {
    unsafe {
        let u = ui();
        let (mut first, mut end) = (0u64, 0u64);
        let mut built = 0;
        for _ in 0..3 {
            built = 0;
            frame(u, || {
                libgui_open_virtual_list(u, c("rows").as_ptr(), 1_000_000, 24.0, &mut first, &mut end);
                for i in first..end {
                    libgui_open_virtual_row(u, i);
                    libgui_label(u, c("row").as_ptr());
                    libgui_close_virtual_row(u);
                    built += 1;
                }
                libgui_close_virtual_list(u);
            });
        }
        assert_eq!(first, 0);
        assert!(end > 5 && end < 40, "a 300 px window built {end} of a million rows");
        assert_eq!(built, end);
        frame(u, || {
            libgui_open_virtual_list(u, c("x").as_ptr(), 3, 24.0, std::ptr::null_mut(), std::ptr::null_mut());
            libgui_close_virtual_list(u);
        });
        libgui_ui_free(u);
    }
}

/// Dashes and rotation from C: the calls reach the draw list with the pattern
/// and the turn encoded as the render contract says, and the contract
/// version a hand-ported shader checks is the library's.
#[test]
fn c_dashes_and_rotation_reach_the_instances() {
    extern "C" fn paint(p: *mut LibguiPainter, r: LibguiRect, user: *mut c_void) {
        let white = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let pts = [r.x, r.y + 20.0, r.x + 30.0, r.y + 20.0, r.x + 30.0, r.y + 60.0];
        unsafe {
            libgui_painter_dashed_line(p, r.x, r.y + 4.0, r.x + 100.0, r.y + 4.0, 2.0, white, 6.0, 3.0, 1.5);
            let end = libgui_painter_dashed_polyline(p, pts.as_ptr(), 3, 1.0, white, 4.0, 4.0, 0.0);
            *(user as *mut f32) = end;
            let img = LibguiRect { x: r.x + 120.0, y: r.y, w: 40.0, h: 20.0 };
            libgui_painter_image_rotated(p, img, 3, 0.0, 0.0, 1.0, 1.0, 4.0, white, 2, std::f32::consts::FRAC_PI_2);
            libgui_painter_text_rotated(p, r.x + 200.0, r.y + 40.0, 13.0, white, c("Height").as_ptr(), -std::f32::consts::FRAC_PI_2);
            // Nulls.
            assert_eq!(libgui_painter_dashed_polyline(p, std::ptr::null(), 3, 1.0, white, 4.0, 4.0, 2.5), 2.5);
            libgui_painter_dashed_line(std::ptr::null_mut(), 0.0, 0.0, 1.0, 1.0, 1.0, white, 1.0, 1.0, 0.0);
            libgui_painter_image_rotated(std::ptr::null_mut(), img, 3, 0.0, 0.0, 1.0, 1.0, 0.0, white, 0, 1.0);
            libgui_painter_text_rotated(p, 0.0, 0.0, 13.0, white, std::ptr::null(), 1.0);
        }
    }
    assert_eq!(libgui_contract_version(), libgui::render_contract::CONTRACT_VERSION);
    unsafe {
        let u = ui();
        let mut end = 0.0f32;
        frame(u, || {
            let cb = LibguiPaintFn { paint: Some(paint), drop_user: None, user: &mut end as *mut f32 as *mut c_void };
            libgui_add_leaf(u, 9, leaf_layout(300.0, 100.0), 0, cb);
        });
        assert_eq!(end, 70.0, "the dashed polyline did not return the length it drew");
        let mut n = 0u64;
        let inst = libgui_frame_instances(u, &mut n) as *const libgui::Instance;
        let inst = std::slice::from_raw_parts(inst, n as usize);
        let kind = |i: &libgui::Instance| libgui::render_contract::PrimitiveKind::from_code(i.params[3]);
        use libgui::render_contract::PrimitiveKind::*;
        let dashed: Vec<_> = inst.iter().filter(|i| kind(i) == Some(Line) && i.params[1] > 0.0).collect();
        assert!(dashed.iter().any(|i| i.params[1] == 6.0 && i.params[2] == 3.0 && i.border_color[0] == 1.5), "the dashed line's pattern did not reach its instance");
        assert!(dashed.iter().any(|i| i.params[1] == 4.0 && i.border_color[0] == 30.0), "the polyline's second segment did not start where the first left off");
        let image = inst.iter().find(|i| kind(i) == Some(Image)).expect("no image instance");
        let rot = libgui::Rotation::from_code([image.border_color[0], image.border_color[1]]);
        assert!((rot.sin - 1.0).abs() < 1e-6 && rot.cos.abs() < 1e-6, "the image is not a quarter turn: {rot:?}");
        let turned_glyphs = inst.iter().filter(|i| kind(i) == Some(Glyph) && i.border_color[1] < -0.99).count();
        assert_eq!(turned_glyphs, "Height".len(), "the turned text's glyphs are not all turned");
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// Scopes and meters from C: a long ring buffer drawn at a screen width's
/// cost, the readout position, a meter's held peak and its latched light,
/// and the painter's trace calls.
#[test]
fn c_scopes_and_meters() {
    extern "C" fn paint(p: *mut LibguiPainter, r: LibguiRect, _: *mut c_void) {
        let s: Vec<f32> = (0..50_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let c = LibguiColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        unsafe {
            libgui_painter_trace(p, r, s.as_ptr(), s.len() as u64, 17, -1.0, 1.0, 1.0, c);
            libgui_painter_trace_fill(p, r, s.as_ptr(), s.len() as u64, 0, -1.0, 1.0, 0.0, c);
            libgui_painter_trace(p, r, std::ptr::null(), 10, 0, -1.0, 1.0, 1.0, c);
            libgui_painter_trace_fill(std::ptr::null_mut(), r, s.as_ptr(), 3, 0, 0.0, 1.0, 0.0, c);
        }
    }
    unsafe {
        let u = ui();
        let samples: Vec<f32> = (0..200_000).map(|i| (i as f32 * 0.002).sin() * 3.0).collect();
        let label = c("v");
        let trace = LibguiScopeTrace {
            samples: samples.as_ptr(),
            count: samples.len() as u64,
            start: 5,
            color: LibguiColor::default(),
            has_color: 0,
            fill: 1,
            _pad: [0; 2],
            width: 1.5,
            label: label.as_ptr(),
        };
        let mut so = std::mem::zeroed::<LibguiScopeOptions>();
        libgui_scope_options_default(&mut so);
        assert_eq!((so.auto_range, so.grid_x), (1, 10));
        let mut mo = std::mem::zeroed::<LibguiMeterOptions>();
        libgui_meter_options_audio_db(&mut mo);
        assert_eq!((mo.lo, mo.hi, mo.vertical, mo.clip_light), (-60.0, 0.0, 1, 1));
        let step = |u, level: f32| -> (LibguiScopeResponse, LibguiMeterResponse, u64) {
            let (mut scope, mut meter) = (LibguiScopeResponse::default(), LibguiMeterResponse::default());
            frame(u, || {
                scope = libgui_scope(u, c("wave").as_ptr(), &trace, 1, &so);
                meter = libgui_meter(u, c("L").as_ptr(), level, &mo);
                libgui_add_leaf(u, 31, leaf_layout(200.0, 40.0), 0, LibguiPaintFn { paint: Some(paint), drop_user: None, user: std::ptr::null_mut() });
            });
            let mut instances = 0;
            libgui_frame_instances(u, &mut instances);
            (scope, meter, instances)
        };
        step(u, -12.0);
        let (scope, _, instances) = step(u, -12.0);
        assert!(scope.lo <= -3.0 && scope.hi >= 3.0, "the fitted range {}..{} does not hold the data", scope.lo, scope.hi);
        assert!(instances < 2_000, "{instances} instances for two hundred thousand samples");
        assert!(scope.at < 0.0, "a readout with no pointer over the scope");
        let r = scope.response.rect;
        libgui_push_pointer_moved(u, r.x + r.w * 0.5, r.y + r.h * 0.5);
        let (scope, meter, _) = step(u, 3.0);
        assert!((scope.at - 0.5).abs() < 0.02, "the pointer at the middle read {}", scope.at);
        assert_eq!(meter.clipped, 1, "over the top did not latch the light");
        let (_, meter, _) = step(u, -40.0);
        assert_eq!(meter.peak, 3.0, "the peak was not held");
        assert_eq!(meter.clipped, 1, "the light went out on its own");
        // Defaults and nulls.
        let mut d = std::mem::zeroed::<LibguiMeterOptions>();
        libgui_meter_options_default(&mut d);
        assert_eq!((d.lo, d.hi, d.cells), (0.0, 1.0, 0));
        frame(u, || {
            libgui_meter_with_average(u, c("cpu").as_ptr(), 0.7, 0.4, &d);
            libgui_meter(u, c("n").as_ptr(), 0.2, std::ptr::null());
            libgui_scope(u, c("empty").as_ptr(), std::ptr::null(), 0, std::ptr::null());
        });
        libgui_meter_options_default(std::ptr::null_mut());
        libgui_meter_options_audio_db(std::ptr::null_mut());
        libgui_scope_options_default(std::ptr::null_mut());
        assert_eq!(libgui_scope(std::ptr::null_mut(), c("x").as_ptr(), &trace, 1, &so).at, -1.0);
        assert_eq!(libgui_ui_poisoned(u), 0);
        libgui_ui_free(u);
    }
}

/// A modal from C: it blocks the button behind, reports Escape, comes and
/// goes with the host's own flag, and a close without an open poisons
/// rather than corrupts.
#[test]
fn a_c_modal_blocks_and_reports() {
    unsafe {
        let u = ui();
        let mut mo = std::mem::zeroed::<LibguiModalOptions>();
        libgui_modal_options_default(&mut mo);
        assert_eq!((mo.dim, mo.shortcuts_behind), (1, 0));
        let step = |u, show: bool| -> (LibguiResponse, LibguiModalResponse) {
            let (mut behind, mut r) = (LibguiResponse::default(), LibguiModalResponse::default());
            frame(u, || {
                behind = libgui_button(u, c("Behind").as_ptr());
                if show {
                    libgui_open_modal(u, c("dlg").as_ptr(), c("Settings").as_ptr(), &mo);
                    libgui_label(u, c("Inside").as_ptr());
                    r = libgui_close_modal(u);
                }
            });
            (behind, r)
        };
        let (behind, _) = step(u, false);
        let (_, r) = step(u, true);
        assert_eq!(r.opened, 1);
        step(u, true);
        assert_eq!(libgui_any_modal_open(u), 1);
        libgui_push_pointer_moved(u, behind.rect.x + 4.0, behind.rect.y + 4.0);
        libgui_push_pointer_button(u, 0, 1);
        step(u, true);
        libgui_push_pointer_button(u, 0, 0);
        let (b, r) = step(u, true);
        assert_eq!(b.clicked, 0, "the button behind the dialog was clicked");
        assert_eq!(r.clicked_outside, 1, "the click outside was not reported");
        assert_eq!(libgui_install_keymap(u, PLATFORM_WINDOWS), 0);
        libgui_push_key(u, KEY_ESCAPE, 1, 0);
        let (_, r) = step(u, true);
        assert_eq!(r.cancelled, 1, "Escape was not reported");
        libgui_push_key(u, KEY_ESCAPE, 0, 0);
        step(u, false);
        step(u, false);
        assert_eq!(libgui_any_modal_open(u), 0);
        // Nulls, then misuse.
        libgui_modal_options_default(std::ptr::null_mut());
        frame(u, || {
            libgui_open_modal(u, c("n").as_ptr(), std::ptr::null(), std::ptr::null());
            libgui_close_modal(u);
        });
        assert_eq!(libgui_any_modal_open(std::ptr::null_mut()), 0);
        libgui_begin_frame(u, 400.0, 300.0, 1.0, 1.0 / 60.0);
        libgui_close_modal(u);
        assert_eq!(libgui_ui_poisoned(u), 1, "closing a modal that was never opened did not poison the handle");
        libgui_ui_free(u);
    }
}
