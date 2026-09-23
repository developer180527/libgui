//! Tables: a properties grid, a parts list, a feature tree's columns.
//!
//! The cell body is a callback for the same reason a dock panel is — libgui
//! calls it while holding `&mut Ui` — so it uses the same
//! [`Borrowed`](crate::handle::Borrowed) trick: the host's own handle, pointed
//! at the live borrow for the duration of the call.
//!
//! Columns and the table's own retained state (sort, column widths, sideways
//! scroll) live in a `LibguiTable` the caller owns, because that state must
//! survive between frames and saves with a layout.

use crate::convert::str_or_empty;
use crate::handle::{inside_callback, set_error, with_ui, Borrowed, LibguiUi};
use libgui::{Align, Column, Sort, TableState};
use std::os::raw::{c_char, c_void};

/// A table's columns and what the user has done to them. The caller owns it
/// and keeps it between frames.
pub struct LibguiTable {
    state: TableState,
}

/// What the table reported this frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LibguiTableResponse {
    /// The user asked to sort by a column: 1 when `sort_column` is meaningful.
    pub sort_changed: u8,
    /// 0 ascending, 1 descending.
    pub sort_descending: u8,
    /// A row was clicked: 1 when `clicked_row` is meaningful.
    pub row_clicked: u8,
    /// A column was resized: 1 when `resized_column` is meaningful.
    pub column_resized: u8,
    pub _pad: [u8; 4],
    pub sort_column: u64,
    pub clicked_row: u64,
    pub resized_column: u64,
    /// Which rows were actually built — a table virtualises, so this is a
    /// window into the total, not all of it.
    pub first_row: u64,
    pub row_count: u64,
}

/// Fill one cell. `row` and `col` say which.
pub type LibguiCellFn = Option<unsafe extern "C" fn(ui: *mut LibguiUi, row: u64, col: u64, user: *mut c_void)>;

/// Create a table. Free it with [`libgui_table_free`].
#[no_mangle]
pub extern "C" fn libgui_table_new() -> *mut LibguiTable {
    Box::into_raw(Box::new(LibguiTable { state: TableState::new([]) }))
}

/// # Safety
/// `table` must have come from [`libgui_table_new`] and not be used again.
#[no_mangle]
pub unsafe extern "C" fn libgui_table_free(table: *mut LibguiTable) {
    if !table.is_null() {
        drop(unsafe { Box::from_raw(table) });
    }
}

/// Append a column. `align` is 0 left, 1 centre, 2 right.
///
/// # Safety
/// `table` must be null or live; `title` null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn libgui_table_add_column(
    table: *mut LibguiTable,
    title: *const c_char,
    width: f32,
    grow: f32,
    resizable: u8,
    sortable: u8,
    align: u32,
) {
    let title = unsafe { str_or_empty(title, "libgui_table_add_column") };
    let Some(t) = (unsafe { table.as_mut() }) else {
        set_error("libgui_table_add_column: null table");
        return;
    };
    let mut c = Column::new(title);
    c.width = width;
    c.grow = grow;
    c.resizable = resizable != 0;
    c.sortable = sortable != 0;
    c.align = match align {
        1 => Align::Center,
        2 => Align::End,
        _ => Align::Start,
    };
    t.state.columns.push(c);
}

/// Remove every column, for a table whose shape changed.
///
/// # Safety
/// `table` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_table_clear_columns(table: *mut LibguiTable) {
    if let Some(t) = unsafe { table.as_mut() } {
        t.state.columns.clear();
    }
}

/// How many leading columns stay put when the table is scrolled sideways.
///
/// # Safety
/// `table` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn libgui_table_set_frozen(table: *mut LibguiTable, frozen: u64) {
    if let Some(t) = unsafe { table.as_mut() } {
        t.state.frozen = frozen as usize;
    }
}

/// Show the table. `cell` is called for each visible cell — the table
/// virtualises, so a million rows cost a screenful.
///
/// The `LibguiUi*` the callback receives is the handle you already own; the
/// same rules apply as for a dock panel (see `libgui_dock_show`).
///
/// # Safety
/// `ui`, `table` must be null or live; `out` writable or null.
#[no_mangle]
pub unsafe extern "C" fn libgui_table_show(
    ui: *mut LibguiUi,
    table: *mut LibguiTable,
    key: *const c_char,
    rows: u64,
    cell: LibguiCellFn,
    user: *mut c_void,
    out: *mut LibguiTableResponse,
) {
    if inside_callback(ui, "libgui_table_show") {
        return;
    }
    let key = unsafe { str_or_empty(key, "libgui_table_show") };
    let Some(t) = (unsafe { table.as_mut() }) else {
        set_error("libgui_table_show: null table");
        return;
    };
    let state = &mut t.state;
    let r = with_ui(ui, LibguiTableResponse::default(), move |u| {
        let resp = u.table(key, state, rows as usize, |cui, row, col| {
            let Some(f) = cell else { return };
            let _guard = Borrowed::new(ui, cui);
            unsafe { f(ui, row as u64, col as u64, user) };
        });
        LibguiTableResponse {
            sort_changed: resp.sort_changed.is_some() as u8,
            sort_descending: matches!(resp.sort_changed, Some((_, Sort::Descending))) as u8,
            row_clicked: resp.clicked_row.is_some() as u8,
            column_resized: resp.resized.is_some() as u8,
            _pad: [0; 4],
            sort_column: resp.sort_changed.map_or(0, |(c, _)| c as u64),
            clicked_row: resp.clicked_row.unwrap_or(0) as u64,
            resized_column: resp.resized.unwrap_or(0) as u64,
            first_row: resp.rows_built.start as u64,
            row_count: (resp.rows_built.end - resp.rows_built.start) as u64,
        }
    });
    if let Some(slot) = unsafe { out.as_mut() } {
        *slot = r;
    }
}
