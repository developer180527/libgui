//! The sidebar: find and replace, and what the document is made of.
//!
//! It is also the demo's second half: click in here and the page loses the
//! keyboard, which is what hands Cmd/Ctrl+Z back to the app.

use libgui::*;

use crate::{lines, Cmd, Pad};

const WIDTH: f32 = 280.0;

pub fn panel(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let col = Layout::column()
        .width(Size::Fixed(WIDTH))
        .height(Size::Grow(1.0))
        .padding(Insets::all(10.0))
        .gap(8.0);
    let frame =
        Frame { fill: t.palette.bg_panel, border: t.palette.border, border_width: 1.0, clip: true, ..Frame::none() };
    ui.container_id(Id::new("sidebar"), col, frame, |ui| {
        ui.segmented("side", &mut pad.side_tab, &["Find", "Document"]);
        if pad.side_tab == 0 {
            find(ui, pad);
        } else {
            stats(ui, pad);
        }
    });
}

fn find(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    ui.section("Find");
    ui.text_input("find", &mut pad.find, "Search the document");
    ui.section("Replace with");
    ui.text_input("replace", &mut pad.replace, "Replacement");

    let hits = pad.matches();
    let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fit).gap(8.0).align(Align::Start, Align::Center);
    ui.container_id(Id::new("findrow"), row, Frame::none(), |ui| {
        let label = match (pad.find.is_empty(), hits.len()) {
            (true, _) => "Matching is case-insensitive".to_string(),
            (false, 0) => "No matches".to_string(),
            (false, 1) => "1 match".to_string(),
            (false, n) => format!("{n} matches"),
        };
        ui.text_with(&label, t.metrics.font_size, t.palette.text_muted);
        ui.flex();
        if ui.button_primary("Replace all").clicked {
            pad.raise(Cmd::ReplaceAll);
        }
    });

    // Where the matches are. Read-only: this demo cannot move the caret from
    // outside the field, and a row that looks clickable and does nothing is
    // worse than a row that does not.
    ui.section("Lines");
    let rows = match_lines(&pad.text, &hits);
    if rows.is_empty() {
        ui.label_muted("—");
        return;
    }
    let size = t.metrics.font_size_small;
    ui.scroll_area("hits", |ui| {
        for (n, line) in &rows {
            let row =
                Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(20.0)).gap(8.0).align(Align::Start, Align::Center);
            let id = ui.make_id(("hit", *n));
            ui.container_id(id, row, Frame::none(), |ui| {
                ui.text_with(&format!("{n}"), size, t.palette.text_faint);
                let cut: String = line.chars().take(34).collect();
                ui.text_with(cut.trim(), size, t.palette.text);
            });
        }
    });
}

/// The lines the matches fall on, numbered from one, each line named once.
fn match_lines(text: &str, hits: &[usize]) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for &at in hits {
        // `at` is a byte offset, so the line it lands on is the number of
        // newlines before it.
        let line = text[..at].matches('\n').count();
        if out.last().map(|(n, _)| *n) == Some(line + 1) {
            continue;
        }
        let src = text.split('\n').nth(line).unwrap_or_default().to_string();
        out.push((line + 1, src));
    }
    out
}

fn stats(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let text = &pad.text;
    let lines = lines(text);
    let words = text.split_whitespace().count();
    let chars = text.chars().count();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let blank = lines.iter().filter(|l| l.trim().is_empty()).count();
    // 200 words a minute, the number every reading-time estimate uses.
    let minutes = (words as f32 / 200.0).max(0.0);

    for (k, v) in [
        ("Lines", lines.len().to_string()),
        ("Blank lines", blank.to_string()),
        ("Words", words.to_string()),
        ("Characters", chars.to_string()),
        ("Longest line", format!("{longest} characters")),
        ("Reading time", format!("{:.1} min", minutes)),
    ] {
        let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(20.0)).align(Align::Start, Align::Center);
        let id = ui.make_id(("stat", k));
        ui.container_id(id, row, Frame::none(), |ui| {
            ui.text_with(k, t.metrics.font_size, t.palette.text_muted);
            ui.flex();
            ui.text_with(&v, t.metrics.font_size, t.palette.text);
        });
    }

    ui.space(4.0);
    ui.section("Line lengths");
    let lengths: Vec<f32> = lines.iter().map(|l| l.chars().count() as f32).collect();
    let max = lengths.iter().copied().fold(1.0f32, f32::max);
    ui.plot("lengths", &lengths, max, 64.0);
    ui.space(4.0);
    ui.paragraph_with(
        "Click here and the page loses the keyboard. Cmd/Ctrl+Z now belongs to this app, \
         and takes back the last command instead of the last thing typed.",
        t.metrics.font_size_small,
        t.palette.text_faint,
        Align::Start,
    );
}
