//! C ABI for libgui, for hosts that are not written in Rust.
//!
//! # Why this is its own crate
//!
//! `crate-type` is fixed in the manifest and cannot be switched on by a
//! feature, so declaring `staticlib` on `libgui` would build a static library
//! for every Rust-only user of it, forever. That alone forces a separate
//! crate. The better reason is that the two make **different promises**:
//! libgui's Rust API is pre-1.0 and changes freely, while a C ABI is a promise
//! about bytes — struct layouts, symbol names, calling convention — that a C++
//! host links against. Different promises need different version numbers, and
//! a crate has only one.
//!
//! # The rules at this boundary
//!
//! - **Nothing panics across it.** Every entry point catches, and a `Ui` that
//!   panicked is *poisoned*: further calls do nothing and
//!   [`libgui_ui_poisoned`] says so. Half a frame is not worth continuing
//!   into, and aborting the host over a UI bug is worse than either.
//! - **No allocation crosses it.** Strings go in as `const char*` the caller
//!   owns; nothing is returned that the caller must free.
//! - **Every struct that crosses is `#[repr(C)]` and size-checked** from both
//!   sides. Mirrors of Rust structs are where ABI bugs hide, so
//!   `libgui_sizeof_*` exists for the C side to `static_assert` against.
//! - **Versioned.** [`libgui_abi_version`] must match `LIBGUI_ABI_VERSION` in
//!   the header the caller compiled against.
//!
//! The widget surface is generated from one table (see `table.rs`), which also
//! emits the header, so the two cannot drift.

use std::os::raw::c_char;

mod containers;
mod convert;
mod handle;
mod dock;
mod dock_build;
mod frame;
pub mod header;
mod input;
mod keymap;
mod nav;
mod table_c;
mod text;
mod theme_dnd;
pub mod table;
mod types;

pub use containers::*;
pub use handle::*;
pub use dock::*;
pub use frame::*;
pub use input::*;
pub use keymap::*;
pub use nav::*;
pub use table_c::*;
pub use text::*;
pub use theme_dnd::*;
// The generated widget entry points. Exported for Rust callers and tests; a C
// caller reaches them through the header, which comes from the same table.
pub use table::*;
pub use types::*;

/// Bump on any change that moves a byte or renames a symbol.
pub const LIBGUI_ABI_VERSION: u32 = 1;

/// The ABI version this library was built with. A host compares it against the
/// `LIBGUI_ABI_VERSION` in the header it compiled against, once, at start-up:
/// a mismatch means the two disagree about layout, and every call after it is
/// undefined.
#[no_mangle]
pub extern "C" fn libgui_abi_version() -> u32 {
    LIBGUI_ABI_VERSION
}

/// The last error, as a static NUL-terminated string, or null if there was
/// none. Owned by the library and valid until the next call on any `Ui`.
#[no_mangle]
pub extern "C" fn libgui_last_error() -> *const c_char {
    handle::last_error()
}
