//! The three strips around the page: the menu bar, the toolbar and the status
//! bar.
//!
//! Every control here raises a [`Cmd`] and nothing else — a menu item, a
//! toolbar button and a chord all end up in the same queue, so there is one
//! implementation of "sort the lines" rather than three.
//!
//! The window's own title bar and its close/minimise buttons belong to the OS,
//! and are not drawn here.

use libgui::*;

use crate::{Cmd, Pad, SIZES};

/// A vertical rule between groups of toolbar controls. `ui.separator` is the
/// horizontal one, for stacking things in a column.
fn rule(ui: &mut Ui, key: &str) {
    let c = ui.theme.palette.border;
    let id = ui.make_id(("rule", key));
    ui.add_leaf(id, Layout::leaf(Size::Fixed(1.0), Size::Fixed(18.0)), Vec2::ZERO, false, move |p, r| {
        p.rect(p.snap_rect(r), c, 0.0)
    });
}

pub fn menu_bar(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(30.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(2.0)
        .align(Align::Start, Align::Center);
    let frame = Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() };
    ui.container_id(Id::new("menubar"), row, frame, |ui| {
        file_menu(ui, pad);
        edit_menu(ui, pad);
        format_menu(ui, pad);
        view_menu(ui, pad);
        help_menu(ui, pad);
        ui.flex();
        let note = if pad.edited { "edited · nothing is saved" } else { "nothing is saved" };
        ui.text_with(note, t.metrics.font_size_small, t.palette.text_faint);
    });
}

/// A menu item that raises a command, labelled with the chord the app's keymap
/// gives it — which is the keymap's job, not libgui's.
fn item(ui: &mut Ui, pad: &mut Pad, label: &str, cmd: Cmd, enabled: bool) {
    let hint = pad.keys().label(cmd);
    let hint = if hint.is_empty() { None } else { Some(hint) };
    if ui.menu_item_ex(label, hint.as_deref(), enabled).clicked {
        pad.raise(cmd);
    }
}

fn file_menu(ui: &mut Ui, pad: &mut Pad) {
    ui.menu_button("File", |ui| {
        item(ui, pad, "New", Cmd::New, true);
        item(ui, pad, "Sample text", Cmd::Sample, true);
        ui.menu_separator();
        // Honest rather than decorative: this demo has no files, and says so
        // where someone would look for them.
        let _ = ui.menu_item_ex("Open…", Some("no files in this demo"), false);
        let _ = ui.menu_item_ex("Save", Some("no files in this demo"), false);
    });
}

fn edit_menu(ui: &mut Ui, pad: &mut Pad) {
    let editing = pad.editing;
    let (can_undo, can_redo) = (pad.can_undo(), pad.can_redo());
    ui.menu_button("Edit", |ui| {
        // While the caret is in the page, these are the *field's* and the
        // chord never reaches Pad — so the menu says whose they are instead of
        // pretending it could run them.
        if editing {
            let _ = ui.menu_item_ex("Undo typing", Some("the page has the caret"), false);
            let _ = ui.menu_item_ex("Redo typing", Some("the page has the caret"), false);
        } else {
            item(ui, pad, "Undo command", Cmd::Undo, can_undo);
            item(ui, pad, "Redo command", Cmd::Redo, can_redo);
        }
        ui.menu_separator();
        item(ui, pad, "Duplicate line", Cmd::DuplicateLine, true);
        item(ui, pad, "Delete line", Cmd::DeleteLine, true);
        item(ui, pad, "Insert date & time", Cmd::InsertDate, true);
    });
}

fn format_menu(ui: &mut Ui, pad: &mut Pad) {
    let sel = pad.has_selection();
    ui.menu_button("Format", |ui| {
        let what = if sel { "selection" } else { "line" };
        item(ui, pad, &format!("UPPER CASE {what}"), Cmd::Upper, true);
        item(ui, pad, &format!("lower case {what}"), Cmd::Lower, true);
        ui.menu_separator();
        item(ui, pad, "Sort lines", Cmd::SortLines, true);
        item(ui, pad, "Trim trailing spaces", Cmd::TrimTrailing, true);
    });
}

fn view_menu(ui: &mut Ui, pad: &mut Pad) {
    let (bar, nums, paper) = (pad.sidebar, pad.line_numbers, pad.paper);
    ui.menu_button("View", |ui| {
        item(ui, pad, if bar { "Hide sidebar" } else { "Show sidebar" }, Cmd::ToggleSidebar, true);
        item(ui, pad, if nums { "Hide line numbers" } else { "Show line numbers" }, Cmd::ToggleNumbers, true);
        item(ui, pad, if paper { "Dark chrome" } else { "Light chrome" }, Cmd::TogglePaper, true);
        ui.menu_separator();
        item(ui, pad, "Bigger text", Cmd::ZoomIn, true);
        item(ui, pad, "Smaller text", Cmd::ZoomOut, true);
        item(ui, pad, "Reset text size", Cmd::ZoomReset, true);
    });
}

fn help_menu(ui: &mut Ui, pad: &mut Pad) {
    ui.menu_button("Help", |ui| {
        item(ui, pad, "About Pad", Cmd::About, true);
    });
}

pub fn toolbar(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(40.0))
        .padding(Insets::xy(8.0, 0.0))
        .gap(6.0)
        .align(Align::Start, Align::Center);
    let frame = Frame { fill: t.palette.bg_panel, border: t.palette.border, border_width: 1.0, clip: true, ..Frame::none() };
    ui.container_id(Id::new("toolbar"), row, frame, |ui| {
        // Size, as a word processor's size box: the value is the app's, the
        // page reads it when it restyles the field.
        ui.text_with("Size", t.metrics.font_size, t.palette.text_muted);
        let labels: Vec<String> = SIZES.iter().map(|s| format!("{s:.0}")).collect();
        let options: Vec<&str> = labels.iter().map(String::as_str).collect();
        // A combo grows to the width it is given, so it is given one.
        let box_ = Layout::row().width(Size::Fixed(72.0)).height(Size::Fit).align(Align::Start, Align::Center);
        let before = pad.size;
        ui.container_id(Id::new("sizebox"), box_, Frame::none(), |ui| {
            ui.combo("size", &mut pad.size, &options);
        });
        if pad.size != before {
            pad.status = format!("{} pt", pad.font_size());
        }
        rule(ui, "a");

        let undo = ui.button("Undo command");
        ui.tooltip(&undo, "Pad's own history. The caret in the page has its own, and Cmd/Ctrl+Z goes there first.");
        if undo.clicked {
            pad.raise(Cmd::Undo);
        }
        if ui.button("Redo").clicked {
            pad.raise(Cmd::Redo);
        }
        rule(ui, "b");

        for (label, cmd) in [
            ("Sort", Cmd::SortLines),
            ("Trim", Cmd::TrimTrailing),
            ("UPPER", Cmd::Upper),
            ("lower", Cmd::Lower),
            ("Duplicate", Cmd::DuplicateLine),
        ] {
            if ui.button(label).clicked {
                pad.raise(cmd);
            }
        }
        rule(ui, "c");

        // A copy, so the state stays the command's to change: the checkbox
        // writes the copy, the queued command writes the document's view.
        let mut nums = pad.line_numbers;
        if ui.checkbox("Line numbers", &mut nums).clicked {
            pad.raise(Cmd::ToggleNumbers);
        }
        ui.flex();
        let mut bar = pad.sidebar;
        if ui.checkbox("Sidebar", &mut bar).clicked {
            pad.raise(Cmd::ToggleSidebar);
        }
        let mut paper = pad.paper;
        if ui.checkbox("Light", &mut paper).clicked {
            pad.raise(Cmd::TogglePaper);
        }
    });
}

pub fn status_bar(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let text = &pad.text;
    let words = text.split_whitespace().count();
    let chars = text.chars().count();
    let lines = text.split('\n').count();
    let (a, b) = pad.selection;

    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(26.0))
        .padding(Insets::xy(10.0, 0.0))
        .gap(14.0)
        .align(Align::Start, Align::Center);
    let frame = Frame { fill: t.palette.bg_panel, clip: true, ..Frame::none() };
    ui.container_id(Id::new("statusbar"), row, frame, |ui| {
        let small = t.metrics.font_size_small;
        // Ln/Col comes from the field: nothing outside it knows where the
        // caret is.
        ui.text_with(&format!("Ln {}, Col {}", pad.caret.0 + 1, pad.caret.1 + 1), small, t.palette.text);
        if b > a {
            ui.text_with(&format!("{} selected", b - a), small, t.palette.accent);
        }
        ui.text_with(&format!("{words} words · {chars} characters · {lines} lines"), small, t.palette.text_muted);
        ui.flex();

        // The whole point of the demo, in one label: which undo the chord
        // would reach if it were pressed right now.
        let chord = pad.keys().label(Cmd::Undo);
        let (who, colour) = if pad.editing {
            ("the page's typing", t.palette.accent)
        } else if pad.can_undo() {
            ("this document's last command", t.palette.text)
        } else {
            ("nothing — no command to take back", t.palette.text_faint)
        };
        ui.text_with(&format!("{chord} → {who}"), small, colour);
        ui.flex();
        ui.text_with(&pad.status, small, t.palette.text_muted);
    });
}
