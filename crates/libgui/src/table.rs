//! Data grids: virtualised rows, resizable columns, a header that stays put,
//! and columns that can be frozen against horizontal scrolling.
//!
//! libgui does not own your rows, exactly as it does not own your tree. You
//! keep the data and the sort order; the table asks for the cells it can
//! actually see, by row and column index, and reports what the user did.
//!
//! ```ignore
//! let mut cols = TableState::new([
//!     Column::new("Name").width(220.0).grow(1.0),
//!     Column::new("Kind").width(90.0),
//!     Column::new("Size").width(80.0).align(Align::End),
//! ]);
//! cols.frozen = 1;                       // Name stays put when scrolled
//!
//! let t = ui.table("files", &mut cols, files.len(), |ui, row, col| match col {
//!     0 => ui.label(&files[row].name),
//!     1 => ui.label(&files[row].kind),
//!     _ => ui.label(&files[row].size),
//! });
//! if let Some((col, order)) = t.sort_changed {
//!     files.sort_by(|a, b| key(a, col).cmp(&key(b, col)));
//!     if order == Sort::Descending { files.reverse(); }
//! }
//! if let Some(i) = t.clicked_row { selected = Some(i); }
//! ```
//!
//! # What it costs
//!
//! Rows are virtualised, so a million of them cost what fifty do. Columns are
//! not: every column of every visible row is built, and clipped if it is off
//! to the side. That is the right trade for the tens of columns a table
//! actually has, and the wrong one for hundreds — measure with
//! [`crate::testing`] before reaching for a table that wide.

use crate::layout::{Align, Insets, Layout, Size};
use crate::math::{Color, Rect, Vec2};
use crate::ui::{Frame, ListOptions, Ui};
use crate::{Chevron, Cursor, Id, Painter};
use std::ops::Range;

/// Which way a column is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Ascending,
    Descending,
}

impl Sort {
    pub fn flipped(self) -> Self {
        match self {
            Sort::Ascending => Sort::Descending,
            Sort::Descending => Sort::Ascending,
        }
    }
}

/// One column's identity and shape. Widths are edited by the user, so this is
/// state your app owns and can save with the rest of its layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: String,
    /// Current width in logical px, before any share of the leftover.
    pub width: f32,
    pub min_width: f32,
    /// Share of the width left over when the columns do not fill the table.
    /// Zero keeps the column exactly `width` wide.
    pub grow: f32,
    pub resizable: bool,
    pub sortable: bool,
    /// Alignment of the cell contents, and of the header title.
    pub align: Align,
}

impl Column {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 120.0,
            min_width: 32.0,
            grow: 0.0,
            resizable: true,
            sortable: true,
            align: Align::Start,
        }
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    pub fn grow(mut self, w: f32) -> Self {
        self.grow = w;
        self
    }

    pub fn fixed(mut self) -> Self {
        self.resizable = false;
        self
    }

    pub fn unsortable(mut self) -> Self {
        self.sortable = false;
        self
    }

    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
}

/// The columns, and everything the user can change about them.
#[derive(Clone, Debug, PartialEq)]
pub struct TableState {
    pub columns: Vec<Column>,
    /// The first `frozen` columns do not move when the table is scrolled
    /// sideways. Clamped to the number of columns.
    pub frozen: usize,
    /// Which column the app is sorting by, and which way. The table only
    /// reports what was asked for; the sorting is yours.
    pub sort: Option<(usize, Sort)>,
    /// How far the scrolling columns are scrolled, logical px. Retained here
    /// rather than inside the library so it saves and restores with a layout.
    pub scroll_x: f32,
    /// The offset handed to layout last frame: a moving table draws its text
    /// sub-pixel, a still one snaps it to the grid.
    applied_x: f32,
}

impl TableState {
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        Self {
            columns: columns.into_iter().collect(),
            frozen: 0,
            sort: None,
            scroll_x: 0.0,
            applied_x: 0.0,
        }
    }

    /// Pin the first `n` columns against horizontal scrolling.
    pub fn frozen(mut self, n: usize) -> Self {
        self.frozen = n;
        self
    }

    pub fn sorted_by(mut self, col: usize, order: Sort) -> Self {
        self.sort = Some((col, order));
        self
    }
}

/// How a table is laid out. [`crate::TableStyle`] supplies the defaults.
#[derive(Clone, Copy, Debug)]
pub struct TableOptions {
    pub row_height: f32,
    pub header_height: f32,
    /// Fills the space it is given by default.
    pub height: Size,
    /// The row to draw as selected, if any.
    pub selected: Option<usize>,
    /// Shade every other row.
    pub striped: bool,
    /// Draw a rule between columns.
    pub grid_lines: bool,
}

/// What the user did to a table this frame.
#[derive(Clone, Debug, Default)]
pub struct TableResponse {
    /// A sortable header was clicked: sort by this column, this way, and put
    /// the answer back in [`TableState::sort`] — the table has already done so.
    pub sort_changed: Option<(usize, Sort)>,
    /// A row was clicked.
    pub clicked_row: Option<usize>,
    /// A column was resized by dragging its edge.
    pub resized: Option<usize>,
    /// The rows that were actually built this frame.
    pub rows_built: Range<usize>,
}

impl Ui {
    /// A data grid. See the [module docs](crate::table) for the shape of it.
    ///
    /// `cell` is called for every column of every *visible* row, with the row
    /// and column index. Rows are virtualised: what is off screen is not built.
    pub fn table(
        &mut self,
        key: &str,
        state: &mut TableState,
        rows: usize,
        cell: impl FnMut(&mut Ui, usize, usize),
    ) -> TableResponse {
        let s = self.theme.table;
        let opts = TableOptions {
            row_height: s.row_height,
            header_height: s.header_height,
            height: Size::Grow(1.0),
            selected: None,
            striped: s.row_fill_alt.a > 0.0,
            grid_lines: true,
        };
        self.table_with(key, state, rows, opts, cell)
    }

    /// [`Ui::table`] with explicit options.
    pub fn table_with(
        &mut self,
        key: &str,
        state: &mut TableState,
        rows: usize,
        opts: TableOptions,
        mut cell: impl FnMut(&mut Ui, usize, usize),
    ) -> TableResponse {
        let id = self.make_id(("table", key));
        let s = self.theme.table;
        let mut out = TableResponse::default();
        state.frozen = state.frozen.min(state.columns.len());

        // Widths come from last frame's rect, like every other geometry read.
        // Only the leftover share depends on it, so a table whose columns are
        // all fixed is exact on its very first frame.
        let outer = self.rect_of(id).unwrap_or_default();
        let fixed: f32 = state.columns.iter().map(|c| c.width).sum();
        let weight: f32 = state.columns.iter().map(|c| c.grow).sum();
        let spare = if weight > 0.0 { (outer.w - fixed).max(0.0) } else { 0.0 };
        let width_of = |c: &Column| c.width + if weight > 0.0 { spare * c.grow / weight } else { 0.0 };
        // The width the frozen pane is actually laid out at, share of the
        // leftover included. Summing the raw widths instead would put the
        // viewport, the scroll range and the scrollbar's origin somewhere the
        // pane is not — visible the moment a frozen column has `grow`.
        let frozen_w: f32 = state.columns[..state.frozen].iter().map(width_of).sum();
        let scroll_w = state.columns[state.frozen..].iter().map(width_of).sum::<f32>();
        let viewport_w = (outer.w - frozen_w).max(0.0);
        let max_x = (scroll_w - viewport_w).max(0.0);

        // Sideways scrolling is the table's own: one offset that the header,
        // the frozen pane and every row have to agree on to the pixel.
        let over = self.scroll_target == Some(id.with(("scroll", "rows")));
        if over {
            let cfg = self.scroll;
            let px = self.input.scroll_px;
            let steps = Vec2::new(
                cfg.steps_to_px(self.input.scroll_lines.x, self.input.scroll_pages.x),
                cfg.steps_to_px(self.input.scroll_lines.y, self.input.scroll_pages.y),
            );
            // A table scrolls sideways from a sideways gesture, or from a
            // plain one while Shift is held, which is the convention
            // everywhere a wide grid exists.
            let d = if self.input.modifiers.shift { px.y + steps.y } else { px.x + steps.x };
            state.scroll_x -= d;
        }
        state.scroll_x = state.scroll_x.clamp(0.0, max_x);
        let moving = state.scroll_x != state.applied_x;
        let scale = self.input.scale.max(0.01);
        if !moving {
            state.scroll_x = (state.scroll_x * scale).round() / scale;
        }
        state.applied_x = state.scroll_x;
        self.animating |= moving;
        let shift = Vec2::new(state.scroll_x, 0.0);

        let layout = Layout::column().width(Size::Grow(1.0)).height(opts.height);
        self.container_id(id, layout, Frame { clip: true, ..Frame::none() }, |ui| {
            // ---- header -----------------------------------------------------
            let head = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(opts.header_height));
            ui.container_id(id.with("head"), head, Frame { fill: s.header_fill, clip: true, ..Frame::none() }, |ui| {
                let (sorted, resized) = header_pane(ui, id.with("hf"), state, 0..state.frozen, width_of, &opts);
                out.sort_changed = out.sort_changed.or(sorted);
                out.resized = out.resized.or(resized);
                let rest = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
                let clip = Frame { clip: true, ..Frame::none() };
                ui.offset_container(id.with("hs"), shift, !moving, rest, clip, |ui| {
                    let cols = state.frozen..state.columns.len();
                    let (sorted, resized) = header_pane(ui, id.with("hx"), state, cols, width_of, &opts);
                    out.sort_changed = out.sort_changed.or(sorted);
                    out.resized = out.resized.or(resized);
                });
            });
            rule(ui, id.with("headrule"), s.grid);

            // ---- rows -------------------------------------------------------
            let list = ListOptions { height: Size::Grow(1.0), ..ListOptions::new(opts.row_height) };
            out.rows_built = ui.virtual_list_with("rows", rows, list, |ui, i| {
                let row_id = ui.make_id(("trow", i));
                let r = ui.interact(row_id);
                if r.clicked {
                    out.clicked_row = Some(i);
                }
                let fill = row_fill(&s, &opts, i, r.hovered);
                let grid = opts.grid_lines.then_some(s.grid);
                // The row's own background and hit area span the whole width,
                // under both panes, so a click lands anywhere along it.
                let full = Layout::leaf(Size::Grow(1.0), Size::Fixed(opts.row_height));
                let hit = crate::LeafOptions { interactive: true, ..Default::default() };
                ui.add_leaf_ex(row_id, full, Vec2::ZERO, hit, move |p, rect| {
                    if fill.a > 0.0 {
                        // Hard edges: a row band is a boundary, not a shape,
                        // and a half-pixel one is a smear at any odd DPI.
                        let band = p.snap_rect(rect);
                        p.rect(band, fill, 0.0);
                    }
                });
                // …and the cells sit on top of it, positioned over the row.
                let over_row = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(opts.row_height));
                ui.container_at(row_id.with("cells"), ui.rect_of(row_id).unwrap_or_default(), Frame::none(), |ui| {
                    ui.container_id(row_id.with("f"), over_row, Frame::none(), |ui| {
                        cells(ui, state, 0..state.frozen, width_of, &opts, grid, i, &mut cell);
                        let rest = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
                        let clip = Frame { clip: true, ..Frame::none() };
                        ui.offset_container(row_id.with("s"), shift, !moving, rest, clip, |ui| {
                            let c = state.frozen..state.columns.len();
                            cells(ui, state, c, width_of, &opts, grid, i, &mut cell);
                        });
                    });
                });
            });

            // ---- sideways scrollbar -----------------------------------------
            if max_x > 0.5 {
                let want = hbar(ui, id.with("hbar"), frozen_w, viewport_w, scroll_w, state.scroll_x, s.grid);
                state.scroll_x = want.clamp(0.0, max_x);
            }
        });
        out
    }
}

/// A 1px horizontal rule across the table.
fn rule(ui: &mut Ui, id: Id, color: Color) {
    let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(1.0));
    ui.add_leaf(id, layout, Vec2::ZERO, false, move |p, r| {
        let line = p.hairline(r.x, r.y, 1.0, r.h);
        p.rect(Rect::new(line.x, line.y, r.w, line.w), color, 0.0);
    });
}

fn row_fill(s: &crate::TableStyle, opts: &TableOptions, i: usize, hovered: bool) -> Color {
    if opts.selected == Some(i) {
        s.row_fill_selected
    } else if hovered {
        s.row_fill_hover
    } else if opts.striped && i % 2 == 1 {
        s.row_fill_alt
    } else {
        Color::TRANSPARENT
    }
}

/// One pane of the header: the frozen columns, or the scrolling ones.
fn header_pane(
    ui: &mut Ui,
    id: Id,
    state: &mut TableState,
    cols: Range<usize>,
    width_of: impl Fn(&Column) -> f32,
    opts: &TableOptions,
) -> (Option<(usize, Sort)>, Option<usize>) {
    let s = ui.theme.table;
    let mut sorted = None;
    let mut resized = None;
    // `Fit`, not `Grow`: the pane is as wide as its columns, so the ones past
    // the table's edge have somewhere to be and the offset can bring them in.
    let layout = Layout::row().width(Size::Fit).height(Size::Fixed(opts.header_height));
    ui.container_id(id, layout, Frame::none(), |ui| {
        for c in cols {
            // Read the fields rather than cloning the column: the title is a
            // `String`, and cloning it would be an allocation per column per
            // frame — the thing the rest of the library just stopped doing.
            // The resize is read *before* this column's width is, so the
            // header cell and the body cells below it are laid out at the
            // same width. Reading it afterwards left the header a drag-delta
            // behind its own column for every frame of the drag.
            let resizable = state.columns[c].resizable;
            let grip = resizable.then(|| {
                let grip_id = ui.make_id(("grip", c));
                let g = ui.interact_drag(grip_id);
                if g.hovered || g.active {
                    ui.cursor = Cursor::ResizeHorizontal;
                }
                if g.active && g.drag_delta.x != 0.0 {
                    let col = &mut state.columns[c];
                    col.width = (col.width + g.drag_delta.x).max(col.min_width);
                    resized = Some(c);
                }
                (grip_id, ui.animate_bool(grip_id, 0, g.hovered || g.active))
            });
            let col = &state.columns[c];
            let (cw, align, sortable) = (width_of(col), col.align, col.sortable);
            let title = ui.frame_text(&state.columns[c].title);
            let cell_id = ui.make_id(("th", c));
            let r = ui.interact(cell_id);
            if sortable && r.clicked {
                let next = match state.sort {
                    Some((prev, order)) if prev == c => (c, order.flipped()),
                    _ => (c, Sort::Ascending),
                };
                state.sort = Some(next);
                sorted = Some(next);
            }
            if r.hovered && sortable {
                ui.cursor = Cursor::Pointer;
            }
            let hot = ui.animate_bool(cell_id, 0, r.hovered);
            let arrow = state.sort.and_then(|(sc, o)| (sc == c).then_some(o));
            let size = ui.theme.metrics.font_size;
            let pad = s.cell_padding_x;
            let leaf = Layout::leaf(Size::Fixed(cw), Size::Grow(1.0));
            ui.add_leaf(cell_id, leaf, Vec2::ZERO, true, move |p, rect| {
                let fg = s.header_text.lerp(s.header_text_active, hot.max(arrow.is_some() as u8 as f32));
                let inner = rect.shrink(pad, 0.0, pad + if arrow.is_some() { 12.0 } else { 0.0 }, 0.0);
                match align {
                    Align::End => p.text_right(inner, size, fg, title),
                    Align::Center => p.text_centered(inner, size, fg, title),
                    _ => p.text_left(inner, size, fg, title),
                }
                if let Some(o) = arrow {
                    let a = Rect::new(rect.right() - 14.0, rect.y, 12.0, rect.h);
                    let dir = if o == Sort::Ascending { Chevron::Up } else { Chevron::Down };
                    p.chevron(a, size * 0.8, dir, fg);
                }
            });
            // The drag zone straddles the column edge: zero-width in the flow,
            // so it shifts nothing, and hit-tested above the header cell. Only
            // the painting is left to do here; the drag was read above.
            if let Some((grip_id, live)) = grip {
                let grip = s.resize_grip;
                let line = s.grid;
                let hotc = s.resize_hover;
                let opts = crate::LeafOptions { interactive: true, hit_pad: grip, hit_top: true };
                let leaf = Layout::leaf(Size::Fixed(0.0), Size::Grow(1.0));
                ui.add_leaf_ex(grip_id, leaf, Vec2::ZERO, opts, move |p, rect| {
                    let c = line.lerp(hotc, live);
                    let r = p.hairline(rect.x, rect.y, 1.0 + live, rect.h);
                    p.rect(r, c, 0.0);
                });
            }
        }
    });
    (sorted, resized)
}

/// The cells of one row, for one pane.
#[allow(clippy::too_many_arguments)]
fn cells(
    ui: &mut Ui,
    state: &TableState,
    cols: Range<usize>,
    width_of: impl Fn(&Column) -> f32,
    opts: &TableOptions,
    grid: Option<Color>,
    row: usize,
    cell: &mut impl FnMut(&mut Ui, usize, usize),
) {
    let pad = ui.theme.table.cell_padding_x;
    for c in cols {
        let cw = width_of(&state.columns[c]);
        let align = state.columns[c].align;
        let id = ui.make_id(("td", c));
        let layout = Layout::row()
            .width(Size::Fixed(cw))
            .height(Size::Fixed(opts.row_height))
            .padding(Insets::xy(pad, 0.0))
            .align(align, Align::Center);
        ui.container_id(id, layout, Frame { clip: true, ..Frame::none() }, |ui| cell(ui, row, c));
        // Zero-width in the flow and drawn on the edge it sits at, so the
        // rules cannot push the columns out of line with their headers.
        if let Some(color) = grid {
            let line = ui.make_id(("tdline", c));
            let leaf = Layout::leaf(Size::Fixed(0.0), Size::Grow(1.0));
            ui.add_leaf(line, leaf, Vec2::ZERO, false, move |p, r| {
                let line = p.hairline(r.x, r.y, 1.0, r.h);
                p.rect(line, color, 0.0);
            });
        }
    }
}

/// The table's own sideways scrollbar, under the scrolling columns only.
/// Returns the offset it was dragged to.
///
/// It is 4 px tall, which is easy to see and hard to hit, so its hit area is
/// padded: the first thing anyone does with a scrollbar is try to drag it.
fn hbar(ui: &mut Ui, id: Id, x: f32, viewport: f32, content: f32, offset: f32, color: Color) -> f32 {
    let h = 4.0;
    let r = ui.interact_drag(id);
    let mut offset = offset;
    let track_w = (r.rect.w - x).max(1.0);
    let thumb_w = (track_w * viewport / content.max(1.0)).max(24.0).min(track_w);
    let travel = (track_w - thumb_w).max(1.0);
    let range = (content - viewport).max(0.0);
    if r.pressed {
        // Clicking the track jumps the thumb to the pointer.
        let at = r.mouse_pos.x - r.rect.x - x;
        let t = offset / range.max(1.0);
        let thumb_x = travel * t.clamp(0.0, 1.0);
        if at < thumb_x || at > thumb_x + thumb_w {
            offset = ((at - thumb_w * 0.5) / travel).clamp(0.0, 1.0) * range;
        }
    }
    if r.active && r.drag_delta.x != 0.0 {
        offset += r.drag_delta.x * range / travel;
    }
    if r.hovered || r.active {
        ui.cursor = crate::Cursor::ResizeHorizontal;
    }
    let hot = ui.animate_bool(id, 0, r.hovered || r.active);
    let color = color.lerp(crate::Color::WHITE.with_alpha(color.a), hot * 0.35);
    let offset_now = offset;
    let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(h));
    let opts = crate::LeafOptions { interactive: true, hit_pad: 5.0, hit_top: true };
    ui.add_leaf_ex(id, layout, Vec2::ZERO, opts, move |p: &mut Painter, r: Rect| {
        let offset = offset_now;
        let track = Rect::new(r.x + x, r.y, (r.w - x).max(0.0), h);
        if track.w <= 0.0 || content <= 0.0 {
            return;
        }
        let thumb_w = (track.w * viewport / content).max(24.0).min(track.w);
        let travel = (track.w - thumb_w).max(0.0);
        let t = if content > viewport { offset / (content - viewport) } else { 0.0 };
        let thumb = Rect::new(track.x + travel * t.clamp(0.0, 1.0), track.y, thumb_w, h);
        p.rect(thumb, color, h * 0.5);
    });
    offset
}
