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
