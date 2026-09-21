//! A data grid is mostly geometry, and geometry is testable without a window:
//! every assertion here is on a rect or a reported event, frame by frame.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 2.0, dt: 1.0 / 60.0 }
}

fn columns() -> TableState {
    TableState::new([
        Column::new("Name").width(150.0),
        Column::new("Kind").width(100.0),
        Column::new("Size").width(100.0).align(Align::End),
        Column::new("Modified").width(140.0),
    ])
}

/// Build one frame and hand back the table's response plus a way to ask where
/// any cell ended up. Cell ids are `("td", col)` inside the row container.
struct World {
    ui: Ui,
    state: TableState,
    rows: usize,
    opts: TableOptions,
}

impl World {
    fn new() -> Self {
        let ui = ui();
        let s = ui.theme.table;
        let opts = TableOptions {
            row_height: 20.0,
            header_height: 24.0,
            height: Size::Grow(1.0),
            selected: None,
            striped: s.row_fill_alt.a > 0.0,
            grid_lines: true,
        };
        Self { ui, state: columns(), rows: 1000, opts }
    }

    fn frame(&mut self) -> TableResponse {
        self.ui.begin_frame(info());
        let rows = self.rows;
        let opts = self.opts;
        let r = self.ui.table_with("files", &mut self.state, rows, opts, |ui, row, col| {
            ui.label(&format!("r{row}c{col}"));
        });
        let _ = self.ui.end_frame();
        r
    }

    fn warm(&mut self) -> TableResponse {
        for _ in 0..3 {
            self.frame();
        }
        self.frame()
    }

    /// Rect of the cell at (row, col), in window space. Mirrors the id chain
    /// the table builds, so it also documents it.
    fn cell(&self, row: usize, col: usize) -> Rect {
        let table = Id::new("root").with(("table", "files"));
        let list = table.with(("scroll", "rows"));
        let row_id = list.with(("vlist_row", row)).with(("trow", row));
        let pane = if col < self.state.frozen { row_id.with("f") } else { row_id.with("s") };
        self.ui.rect_of(pane.with(("td", col))).unwrap_or_else(|| panic!("no cell ({row}, {col})"))
    }

    fn header(&self, col: usize) -> Rect {
        let table = Id::new("root").with(("table", "files"));
        let pane = if col < self.state.frozen { table.with("hf") } else { table.with("hx") };
        self.ui.rect_of(pane.with(("th", col))).unwrap_or_else(|| panic!("no header {col}"))
    }

    /// Rect of the sideways scrollbar under the scrolling columns.
    fn hbar(&self) -> Rect {
        let table = Id::new("root").with(("table", "files"));
        self.ui.rect_of(table.with("hbar")).expect("no scrollbar")
    }

    fn press(&mut self, at: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    }

    fn release(&mut self) {
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    }

    fn wheel(&mut self, d: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: Vec2::new(200.0, 150.0) });
        self.ui.push(InputEvent::Wheel { delta: d, unit: WheelUnit::Pixel });
    }
}

#[test]
fn a_million_rows_build_only_what_fits() {
    let mut w = World::new();
    w.rows = 1_000_000;
    let r = w.warm();
    let built = r.rows_built.end - r.rows_built.start;
    assert!(built > 0 && built < 40, "built {built} rows for a 300px viewport");

    let mut small = World::new();
    small.rows = 100;
    let s = small.warm();
    assert_eq!(built, s.rows_built.end - s.rows_built.start, "row count depended on list length");
}

#[test]
fn clicking_a_sortable_header_cycles_and_reports() {
    let mut w = World::new();
    w.warm();
    assert_eq!(w.state.sort, None);

    // The "Kind" header sits after Name (150 wide), inside a 24px-tall header.
    let at = Vec2::new(200.0, 12.0);
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.sort_changed, Some((1, Sort::Ascending)));
    assert_eq!(w.state.sort, Some((1, Sort::Ascending)));

    // Clicking the same header again flips it.
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.sort_changed, Some((1, Sort::Descending)));

    // And a different one starts over ascending.
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 12.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.sort_changed, Some((0, Sort::Ascending)));
}

#[test]
fn an_unsortable_column_never_reports_a_sort() {
    let mut w = World::new();
    w.state.columns[1] = Column::new("Kind").width(100.0).unsortable();
    w.warm();
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(200.0, 12.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.sort_changed, None);
    assert_eq!(w.state.sort, None);
}

#[test]
fn dragging_a_column_edge_resizes_it_and_stops_at_the_minimum() {
    let mut w = World::new();
    w.state.columns[0].min_width = 60.0;
    w.warm();
    let start = w.state.columns[0].width;

    // The grip straddles the edge between Name and Kind, at x = 150.
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(150.0, 12.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(190.0, 12.0) });
    let r = w.frame();
    assert_eq!(r.resized, Some(0));
    assert_eq!(w.state.columns[0].width, start + 40.0);

    // Far past the minimum, and it clamps rather than inverting.
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(-400.0, 12.0) });
    w.frame();
    assert_eq!(w.state.columns[0].width, 60.0, "a column shrank past its minimum");
    assert_eq!(w.ui.cursor, Cursor::ResizeHorizontal, "no resize cursor while dragging an edge");
}

#[test]
fn a_fixed_column_has_no_grip() {
    let mut w = World::new();
    w.state.columns[0] = Column::new("Name").width(150.0).fixed();
    w.warm();
    let before = w.state.columns[0].width;
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(150.0, 12.0) });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(200.0, 12.0) });
    let r = w.frame();
    assert_eq!(r.resized, None);
    assert_eq!(w.state.columns[0].width, before);
}

#[test]
fn clicking_a_row_reports_it() {
    let mut w = World::new();
    w.warm();
    // Rows start under the 24px header and its 1px rule.
    let at = Vec2::new(80.0, 24.0 + 1.0 + 10.0);
    w.ui.push(InputEvent::PointerMoved { pos: at });
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    w.frame();
    w.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = w.frame();
    assert_eq!(r.clicked_row, Some(0));
}

#[test]
fn grow_shares_the_leftover_width() {
    let mut w = World::new();
    w.state.columns = vec![
        Column::new("A").width(100.0).grow(1.0),
        Column::new("B").width(100.0).grow(3.0),
    ];
    w.warm();
    // 400 wide, 200 of fixed width: 200 spare, split 1:3.
    let a = w.ui.rect_of(Id::new("root").with(("table", "files")));
    assert!(a.is_some());
    let head = w.ui.rect_of(Id::new("root").with(("table", "files")).with("head")).expect("header");
    assert_eq!(head.w, 400.0);
}

#[test]
fn scrolling_sideways_moves_the_columns_and_the_header_together() {
    let mut w = World::new();
    w.warm();
    let c1 = w.cell(0, 1).x;
    let h1 = w.header(1).x;
    assert_eq!(c1, h1, "a header and its column started out of line");

    w.wheel(Vec2::new(-60.0, 0.0));
    w.frame();
    w.frame();
    assert_eq!(w.state.scroll_x, 60.0);
    assert_eq!(w.cell(0, 1).x, c1 - 60.0, "the column did not follow the scroll");
    assert_eq!(w.header(1).x, h1 - 60.0, "the header did not follow its column");
    assert_eq!(w.cell(0, 1).x, w.header(1).x, "header and column drifted apart");
}

#[test]
fn frozen_columns_stay_put_while_the_rest_scroll() {
    let mut w = World::new();
    w.state.frozen = 1;
    w.warm();
    let frozen = w.cell(0, 0).x;
    let moving = w.cell(0, 1).x;

    w.wheel(Vec2::new(-80.0, 0.0));
    w.frame();
    w.frame();
    assert_eq!(w.cell(0, 0).x, frozen, "a frozen column scrolled away");
    assert_eq!(w.header(0).x, frozen, "a frozen header scrolled away");
    assert_eq!(w.cell(0, 1).x, moving - 80.0, "the scrolling columns did not move");
}

#[test]
fn the_offset_stops_at_the_end_of_the_columns() {
    let mut w = World::new();
    w.warm();
    // 490 px of columns in a 400 px table: 90 px of travel.
    for _ in 0..8 {
        w.wheel(Vec2::new(-500.0, 0.0));
        w.frame();
    }
    assert_eq!(w.state.scroll_x, 90.0, "scrolled past the last column");

    for _ in 0..8 {
        w.wheel(Vec2::new(500.0, 0.0));
        w.frame();
    }
    assert_eq!(w.state.scroll_x, 0.0, "scrolled before the first column");
}

#[test]
fn a_table_that_fits_does_not_scroll_at_all() {
    let mut w = World::new();
    w.state.columns = vec![Column::new("A").width(100.0), Column::new("B").width(100.0)];
    w.warm();
    w.wheel(Vec2::new(-200.0, 0.0));
    w.frame();
    assert_eq!(w.state.scroll_x, 0.0);
}

#[test]
fn shift_and_a_plain_wheel_scroll_sideways_too() {
    let mut w = World::new();
    w.warm();
    w.ui.push(InputEvent::ModifiersChanged(Modifiers { shift: true, ..Modifiers::NONE }));
    w.wheel(Vec2::new(0.0, -40.0));
    w.frame();
    w.frame();
    assert_eq!(w.state.scroll_x, 40.0, "shift+wheel did not scroll sideways");
}

#[test]
fn a_resized_column_moves_the_ones_after_it() {
    let mut w = World::new();
    w.warm();
    let before = w.cell(0, 2).x;
    w.state.columns[0].width += 30.0;
    w.frame();
    w.frame();
    assert_eq!(w.cell(0, 2).x, before + 30.0, "widening a column did not push its neighbours");
}

#[test]
fn cells_line_up_with_their_header_in_every_column() {
    let mut w = World::new();
    w.state.frozen = 2;
    w.warm();
    for c in 0..w.state.columns.len() {
        assert_eq!(w.cell(0, c).x, w.header(c).x, "column {c} is not under its header");
        assert_eq!(w.cell(0, c).w, w.header(c).w, "column {c} is not as wide as its header");
    }
}

/// The sideways scrollbar is the first thing anyone tries to drag, so it has
/// to be draggable — and clicking its track has to jump to the pointer.
#[test]
fn the_sideways_scrollbar_drags() {
    let mut w = World::new();
    // Narrow the window until the columns overflow, so there is a bar at all.
    w.state = TableState::new([
        Column::new("Name").width(260.0),
        Column::new("Kind").width(220.0),
        Column::new("Size").width(220.0),
        Column::new("Modified").width(240.0),
    ]);
    w.warm();
    let bar = w.hbar();
    assert!(bar.w > 0.0, "no scrollbar rect");
    assert_eq!(w.state.scroll_x, 0.0);

    // Drag the thumb to the right.
    let at = Vec2::new(bar.x + 40.0, bar.center().y);
    w.press(at);
    w.frame();
    w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(at.x + 60.0, at.y) });
    w.frame();
    let dragged = w.state.scroll_x;
    assert!(dragged > 0.0, "dragging the scrollbar did not scroll: {dragged}");
    w.release();
    w.frame();

    // Clicking further along the track jumps rather than doing nothing.
    let far = Vec2::new(bar.right() - 30.0, bar.center().y);
    w.press(far);
    w.frame();
    w.release();
    w.frame();
    assert!(w.state.scroll_x > dragged, "clicking the track did not jump: {} vs {dragged}", w.state.scroll_x);

    // And it stops at the end rather than running past it.
    for _ in 0..4 {
        w.press(Vec2::new(bar.right() - 2.0, bar.center().y));
        w.frame();
        w.release();
        w.frame();
    }
    let max = w.state.scroll_x;
    w.press(Vec2::new(bar.right() - 2.0, bar.center().y));
    w.frame();
    w.release();
    w.frame();
    assert_eq!(w.state.scroll_x, max, "the scrollbar ran past the end");
}
