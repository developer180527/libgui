//! A fourth libgui demo: **Pad**, a plain-text editor.
//!
//! The first demo shows features one at a time, the second density, the third
//! a timeline. This one is a document: one big [`Ui::text_area`] on a page,
//! with the chrome a text editor wears around it — a menu bar, a toolbar, a
//! find-and-replace panel and a status bar.
//!
//! It is plain text and **nothing is saved**: there is no file open, no file
//! written, and quitting loses the document. That is deliberate — the point
//! here is the editing, not an I/O layer libgui has no opinion about.
//!
//! # The part worth reading: two undo stacks
//!
//! Pad keeps its **own** document history — a snapshot before every command it
//! runs (Sort, Trim, Replace all, …). The text area keeps the **field's**
//! history of typing. Both answer to Cmd/Ctrl+Z, and they never collide,
//! because the field gets first refusal:
//!
//! - caret in the page → the chord is the field's, [`Ui::consume_shortcut`]
//!   refuses it here, and Pad's document stack is not touched;
//! - focus anywhere else → the field never sees it, `consume_shortcut` hands
//!   it over, and Pad undoes its last command.
//!
//! The status bar says which one the chord would hit right now, live. Run the
//! demo, type a word, press Cmd+Z; then click the sidebar and press it again.
//!
//! A command writes the document from outside the field, so the field notices
//! its buffer was replaced and drops its typing history — which is right: that
//! history described text that no longer exists. Pad's own stack is what takes
//! a command back.

use libgui::*;
use libgui_keymap::{Chord, Keymap, Platform};

pub mod theme;

mod chrome;
mod page;
mod side;

pub use theme::theme;

/// Everything Pad can do to the document, and everything a menu, a toolbar
/// button or a chord can ask for. One enum, so a command has exactly one
/// implementation no matter which of the three fired it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    New,
    Sample,
    /// The *document's* undo — Pad's own, not the field's.
    Undo,
    Redo,
    InsertDate,
    DuplicateLine,
    DeleteLine,
    SortLines,
    TrimTrailing,
    Upper,
    Lower,
    ReplaceAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleSidebar,
    ToggleNumbers,
    TogglePaper,
    About,
}

/// Text sizes the toolbar offers, in points.
pub const SIZES: [f32; 6] = [11.0, 13.0, 15.0, 18.0, 22.0, 28.0];

const SAMPLE: &str = "\
Pad — a plain-text editor built with libgui.

Nothing here is saved. Close the window and the document is gone; that is the
deal, and it keeps the demo about editing rather than about files.

Try this:
  1. Type a few words, then press Cmd/Ctrl+Z. The field takes back the typing.
  2. Click this sidebar, then press it again. Now Pad undoes its last command,
     because the field never saw the chord.
  3. Run Format > Sort lines, then undo it from outside the page.

The status bar names which undo the chord would reach, live.

Long lines are not wrapped yet, so this one runs off the right edge of the page to show that the caret drags the view sideways with it rather than folding.
";

/// The document, the view, and Pad's own undo.
pub struct Pad {
    pub text: String,
    pub find: String,
    pub replace: String,
    /// Index into [`SIZES`]: the size the page is set in.
    pub size: usize,
    pub line_numbers: bool,
    pub sidebar: bool,
    /// Sidebar page: 0 = Find, 1 = Stats.
    pub side_tab: usize,
    /// Light chrome instead of dark. The page is white either way, the way a
    /// word processor keeps the paper white in its dark mode.
    pub paper: bool,

    /// Where the caret was on the last frame, from [`TextResponse::caret`]:
    /// a line number and a character column. Commands act on it, which is why
    /// the field has to report it.
    pub caret: (usize, usize),
    /// The selected range, as byte offsets into `text`.
    pub selection: (usize, usize),
    /// The page has the keyboard — so the undo chord *may* be the field's.
    pub editing: bool,
    /// The page has typing of its own to take back. When it does not, the
    /// chord is released and reaches Pad even with the caret in the page.
    pub field_can_undo: bool,
    pub edited: bool,
    pub status: String,

    /// The document's history: whole snapshots, one per command.
    undo: Vec<String>,
    redo: Vec<String>,
    /// Commands raised this frame, run at the start of the next one.
    queue: Vec<Cmd>,
    keys: Keymap<Cmd>,
}

impl Default for Pad {
    fn default() -> Self {
        Self::new(Platform::current())
    }
}

/// Steps of document history kept. Snapshots of a plain-text file are cheap,
/// but not free.
const MAX_UNDO: usize = 64;

impl Pad {
    /// `platform` decides what the menus spell and which chords fire — the
    /// app's keymap, not libgui's. A test pins it so the picture is the same
    /// everywhere.
    pub fn new(platform: Platform) -> Self {
        let mut keys = Keymap::new(platform);
        keys.bind(Chord::primary(Key::N), Cmd::New)
            .bind(Chord::primary(Key::Z), Cmd::Undo)
            .bind(Chord::primary(Key::Z).shift(), Cmd::Redo)
            .bind(Chord::primary(Key::D), Cmd::DuplicateLine)
            .bind(Chord::primary(Key::K), Cmd::DeleteLine)
            .bind(Chord::primary(Key::Equal), Cmd::ZoomIn)
            .bind(Chord::primary(Key::Minus), Cmd::ZoomOut)
            .bind(Chord::primary(Key::Num0), Cmd::ZoomReset)
            .bind(Chord::primary(Key::Backslash), Cmd::ToggleSidebar);
        Self {
            text: SAMPLE.to_string(),
            find: String::new(),
            replace: String::new(),
            size: 1,
            line_numbers: false,
            sidebar: true,
            side_tab: 0,
            paper: false,
            caret: (0, 0),
            selection: (0, 0),
            editing: false,
            field_can_undo: false,
            edited: false,
            status: "Ready. Nothing is saved.".into(),
            undo: Vec::new(),
            redo: Vec::new(),
            queue: Vec::new(),
            keys,
        }
    }

    pub fn keys(&self) -> &Keymap<Cmd> {
        &self.keys
    }

    /// Ask for a command. It runs at the top of the next frame, so a menu
    /// click and a chord take the same path and neither edits the document
    /// halfway through building a frame that already drew it.
    pub fn raise(&mut self, cmd: Cmd) {
        self.queue.push(cmd);
    }

    pub fn font_size(&self) -> f32 {
        SIZES[self.size.min(SIZES.len() - 1)]
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn has_selection(&self) -> bool {
        self.selection.1 > self.selection.0
    }

    /// Where the find field matches, as byte offsets into the document. Empty
    /// when the field is, since an empty needle matches everywhere and that is
    /// not what anyone means by it.
    pub fn matches(&self) -> Vec<usize> {
        if self.find.is_empty() {
            return Vec::new();
        }
        let hay = self.text.to_lowercase();
        let needle = self.find.to_lowercase();
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(i) = hay[at..].find(&needle) {
            out.push(at + i);
            at += i + needle.len();
        }
        out
    }

    // ---- the document's history -------------------------------------------

    /// Remember the text before a command changes it. Every command that
    /// writes the document calls this first, and nothing else does.
    fn snapshot(&mut self) {
        self.redo.clear();
        self.undo.push(self.text.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.edited = true;
    }

    // ---- commands ----------------------------------------------------------

    /// Run everything raised since the last frame.
    fn run_queue(&mut self, ui: &mut Ui) {
        let queued: Vec<Cmd> = self.queue.drain(..).collect();
        if queued.is_empty() {
            return;
        }
        for cmd in queued {
            self.run(cmd);
        }
        // The document may have changed after the frame that drew it, so ask
        // for one more rather than waiting for the next input.
        ui.request_repaint();
    }

    fn run(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::New => {
                self.snapshot();
                self.text.clear();
                self.status = "New document".into();
            }
            Cmd::Sample => {
                self.snapshot();
                self.text = SAMPLE.to_string();
                self.status = "Sample text".into();
            }
            Cmd::Undo => match self.undo.pop() {
                Some(prev) => {
                    self.redo.push(std::mem::replace(&mut self.text, prev));
                    self.status = "Undid a command — the document's history, not the field's".into();
                }
                None => self.status = "Nothing to undo in the document".into(),
            },
            Cmd::Redo => match self.redo.pop() {
                Some(next) => {
                    self.undo.push(std::mem::replace(&mut self.text, next));
                    self.status = "Redid a command".into();
                }
                None => self.status = "Nothing to redo".into(),
            },
            Cmd::InsertDate => {
                self.snapshot();
                let stamp = utc_now();
                let at = line_byte(&self.text, self.caret);
                self.text.insert_str(at, &stamp);
                self.status = format!("Inserted {stamp}");
            }
            Cmd::DuplicateLine => {
                self.snapshot();
                let mut lines = lines(&self.text);
                let i = self.caret.0.min(lines.len() - 1);
                lines.insert(i, lines[i].clone());
                self.text = lines.join("\n");
                self.status = format!("Duplicated line {}", i + 1);
            }
            Cmd::DeleteLine => {
                self.snapshot();
                let mut lines = lines(&self.text);
                let i = self.caret.0.min(lines.len() - 1);
                lines.remove(i);
                if lines.is_empty() {
                    lines.push(String::new());
                }
                self.text = lines.join("\n");
                self.status = format!("Deleted line {}", i + 1);
            }
            Cmd::SortLines => {
                self.snapshot();
                let mut lines = lines(&self.text);
                lines.sort_by_key(|l| l.to_lowercase());
                let n = lines.len();
                self.text = lines.join("\n");
                self.status = format!("Sorted {n} lines");
            }
            Cmd::TrimTrailing => {
                self.snapshot();
                let mut trimmed = 0usize;
                let lines: Vec<String> = lines(&self.text)
                    .iter()
                    .map(|l| {
                        let t = l.trim_end();
                        trimmed += l.len() - t.len();
                        t.to_string()
                    })
                    .collect();
                self.text = lines.join("\n");
                self.status = format!("Trimmed {trimmed} trailing characters");
            }
            Cmd::Upper | Cmd::Lower => {
                self.snapshot();
                let up = cmd == Cmd::Upper;
                // The selection, or the caret's line when there is none — the
                // rule every editor uses for a line-or-selection command.
                // Byte offsets both ways: the field reports the selection in
                // bytes, and a line's span is measured the same way, so a
                // command never converts between counts and offsets.
                let (a, b) = if self.has_selection() { self.selection } else { line_span(&self.text, self.caret.0) };
                let slice = &self.text[a..b];
                let cased = if up { slice.to_uppercase() } else { slice.to_lowercase() };
                let n = slice.chars().count();
                self.text.replace_range(a..b, &cased);
                self.status = format!("{} {n} characters", if up { "Upper-cased" } else { "Lower-cased" });
            }
            Cmd::ReplaceAll => {
                if self.find.is_empty() {
                    self.status = "Nothing to find".into();
                    return;
                }
                let n = self.matches().len();
                if n == 0 {
                    self.status = format!("No match for “{}”", self.find);
                    return;
                }
                self.snapshot();
                self.text = replace_all_ignoring_case(&self.text, &self.find, &self.replace);
                self.status = format!("Replaced {n} match{}", if n == 1 { "" } else { "es" });
            }
            Cmd::ZoomIn => {
                self.size = (self.size + 1).min(SIZES.len() - 1);
                self.status = format!("{} pt", self.font_size());
            }
            Cmd::ZoomOut => {
                self.size = self.size.saturating_sub(1);
                self.status = format!("{} pt", self.font_size());
            }
            Cmd::ZoomReset => {
                self.size = 1;
                self.status = format!("{} pt", self.font_size());
            }
            Cmd::ToggleSidebar => self.sidebar = !self.sidebar,
            Cmd::ToggleNumbers => self.line_numbers = !self.line_numbers,
            Cmd::TogglePaper => self.paper = !self.paper,
            Cmd::About => {
                self.status = "Pad — a libgui demo. Plain text, no files, two undo stacks.".into();
            }
        }
    }

    // ---- the frame ---------------------------------------------------------

    pub fn ui(&mut self, ui: &mut Ui) {
        self.run_queue(ui);
        ui.theme = theme(self.paper);

        let t = ui.theme.clone();
        let root = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
        ui.container_id(
            Id::new("pad"),
            root,
            Frame { fill: t.palette.bg_app, clip: true, ..Frame::none() },
            |ui| {
                chrome::menu_bar(ui, self);
                chrome::toolbar(ui, self);
                let body = Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0));
                ui.container_id(Id::new("body"), body, Frame::none(), |ui| {
                    page::page(ui, self);
                    if self.sidebar {
                        side::panel(ui, self);
                    }
                });
                chrome::status_bar(ui, self);
            },
        );

        // After the widgets, so a chord one of them claimed this frame is
        // already gone. The undo chord needs no ordering to be safe —
        // `consume_shortcut` refuses a field's own keys while a field has
        // focus, wherever it is called from — but a chord the app and a widget
        // both want goes to whoever asks first, and the widget should win.
        for cmd in COMMANDS {
            if self.keys.triggered(ui, cmd) {
                self.raise(cmd);
            }
        }
    }
}

/// Every command a chord can fire. Kept here so `ui` can ask about each one
/// without a bound chord being forgotten.
const COMMANDS: [Cmd; 9] = [
    Cmd::New,
    Cmd::Undo,
    Cmd::Redo,
    Cmd::DuplicateLine,
    Cmd::DeleteLine,
    Cmd::ZoomIn,
    Cmd::ZoomOut,
    Cmd::ZoomReset,
    Cmd::ToggleSidebar,
];

// ---- text helpers ----------------------------------------------------------

pub(crate) fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

/// Byte offset of the caret's `(line, column)` — the column counted in chars,
/// as the field reports it.
fn line_byte(text: &str, caret: (usize, usize)) -> usize {
    // The line's own span, not the rest of the document: a column past the end
    // of its line clamps there rather than running on into the next one.
    let (start, end) = line_span(text, caret.0);
    let line = &text[start..end];
    line.char_indices().nth(caret.1).map_or(end, |(b, _)| start + b)
}

/// The byte range one line covers, newline excluded.
fn line_span(text: &str, line: usize) -> (usize, usize) {
    let mut start = 0;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return (text.len(), text.len()),
        }
    }
    (start, text[start..].find('\n').map_or(text.len(), |i| start + i))
}

/// Case-insensitive replace, so Find and Replace agree about what a match is.
fn replace_all_ignoring_case(text: &str, find: &str, with: &str) -> String {
    let (hay, needle) = (text.to_lowercase(), find.to_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    // `to_lowercase` can change a string's length, so the lowered copy is only
    // safe to search when it maps byte-for-byte. It does for the ASCII this
    // demo's find field is used with; anything else falls back to an exact
    // match, rather than slicing the original at an index that means something
    // different in it.
    if hay.len() != text.len() {
        return text.replace(find, with);
    }
    while let Some(i) = hay[at..].find(&needle) {
        out.push_str(&text[at..at + i]);
        out.push_str(with);
        at += i + needle.len();
    }
    out.push_str(&text[at..]);
    out
}

fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    utc_stamp(secs)
}

/// `YYYY-MM-DD HH:MM UTC` from a Unix time — no date crate for one line of a
/// demo. (Civil-from-days, after Howard Hinnant.)
fn utc_stamp(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3_600, (rem % 3_600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field reports a character column; a `String` is sliced by bytes.
    /// This is the one place Pad converts, and the line it converts within is
    /// the only thing it walks.
    #[test]
    fn a_caret_becomes_a_byte_offset() {
        let text = "one\ntwö\nthree";
        assert_eq!(line_byte(text, (0, 0)), 0);
        assert_eq!(line_span(text, 1), (4, 8), "the multi-byte line was measured in chars");
        // Column 3 of "twö" is past 'ö', which is two bytes.
        assert_eq!(line_byte(text, (1, 3)), 8);
        // A column past the end of its line clamps rather than running on.
        assert_eq!(line_byte(text, (1, 99)), 8);
    }

    #[test]
    fn replace_all_is_case_insensitive_and_keeps_the_rest() {
        assert_eq!(replace_all_ignoring_case("Pad pad PAD.", "pad", "x"), "x x x.");
        assert_eq!(replace_all_ignoring_case("nothing", "zz", "x"), "nothing");
    }

    /// Dates are hand-rolled here, so they are checked at instants everyone
    /// knows rather than at "whatever today is".
    #[test]
    fn the_date_is_the_date() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_stamp(1_000_000_000), "2001-09-09 01:46 UTC");
        // A leap day, which is where a hand-rolled calendar goes wrong.
        assert_eq!(utc_stamp(1_709_164_800), "2024-02-29 00:00 UTC");
        assert!(utc_now().ends_with(" UTC"));
    }
}
