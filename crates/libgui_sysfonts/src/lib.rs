//! The fonts installed on the system, as a fallback for libgui.
//!
//! libgui's core reads no files, so text in a script its fonts lack draws as
//! nothing: a Chinese filename is blank. This crate is the other half. It
//! finds the system's fonts and hands the core a face when it meets a
//! character none of its fonts has, the way a browser or a native toolkit
//! does:
//!
//! ```no_run
//! # fn f(inter: &[u8]) -> Result<(), libgui::FontError> {
//! let ui = libgui_sysfonts::ui(libgui::Theme::dark(), inter)?;
//! # let _ = ui; Ok(()) }
//! ```
//!
//! or, to keep your own chain and add the system's after it:
//!
//! ```no_run
//! # fn f(inter: &[u8], icons: &[u8]) -> Result<(), libgui::FontError> {
//! use libgui::{FontStack, Theme, Ui};
//! use libgui_sysfonts::SystemFonts;
//! let stack = FontStack::from_fonts(&[inter, icons])?.with_source(Box::new(SystemFonts::new().with_language("ja")));
//! let ui = Ui::with_rasterizer(Theme::dark(), Box::new(stack));
//! # let _ = ui; Ok(()) }
//! ```
//!
//! **Lazily.** Nothing is loaded up front. The first time a character turns
//! up that no face covers, its script picks the platform's own font for it
//! (PingFang or Hiragino on macOS, YaHei or Yu Gothic on Windows, Noto on
//! Linux), and only if none of those has it are the rest searched. The face
//! found joins the chain and covers the rest of that script; a character no
//! installed font has is looked for once. A CJK font is tens of megabytes,
//! and an app that never shows Chinese never opens one.
//!
//! **The first character of a script costs a moment**: finding and reading
//! its font takes 0.1–0.2 s for a CJK font (measured on macOS with PingFang),
//! paid once, on the first frame that shows the script. To pay it at start-up
//! instead, measure a sample before the first frame:
//! `ui.fonts.measure(font, 14.0, "文かな한")`.
//!
//! **Han characters** are shared by Chinese, Japanese and Korean, which draw
//! many of them differently. `with_language` says which to prefer; without
//! it, Simplified Chinese.
//!
//! **Not covered:** colour emoji (Apple Color Emoji and Segoe UI Emoji are
//! bitmap or layered colour fonts, which libgui's single-channel atlas does
//! not draw), and complex shaping for a system face: faces found here are
//! read with fontdue, so Arabic and Indic scripts from system fonts draw
//! their letters unjoined. Give libgui a shaped face for those scripts
//! yourself (`ShapeRasterizer`) and this crate is not asked.

use libgui::{FontRasterizer, FontSource, FontdueRasterizer};

/// The system's fonts, as a [`FontSource`] for a [`libgui::FontStack`].
pub struct SystemFonts {
    db: fontdb::Database,
    language: Language,
}

/// Which Han glyphs to prefer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    SimplifiedChinese,
    TraditionalChinese,
    Japanese,
    Korean,
}

/// What a character needs, for picking a font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Han,
    Kana,
    Hangul,
    Arabic,
    Hebrew,
    Devanagari,
    Bengali,
    Tamil,
    Thai,
    Other,
}

impl Default for SystemFonts {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemFonts {
    /// Every font installed on the system: their names and where they are,
    /// read from the font directories; no font file is opened yet.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self::with_database(db)
    }

    /// Over a font database you filled yourself: an app's own font folder,
    /// or, in a test, nothing.
    pub fn with_database(db: fontdb::Database) -> Self {
        Self { db, language: Language::SimplifiedChinese }
    }

    /// The language Han characters are in, as a BCP-47 tag: `"ja"`, `"ko"`,
    /// `"zh-Hant"`, `"zh-TW"`, `"zh-HK"`; anything else is Simplified Chinese.
    /// Kana and Hangul pick their own language whatever this says.
    pub fn with_language(mut self, tag: &str) -> Self {
        let t = tag.to_ascii_lowercase();
        self.language = if t.starts_with("ja") {
            Language::Japanese
        } else if t.starts_with("ko") {
            Language::Korean
        } else if t.contains("hant") || t.ends_with("-tw") || t.ends_with("-hk") || t.ends_with("-mo") {
            Language::TraditionalChinese
        } else {
            Language::SimplifiedChinese
        };
        self
    }

    /// The database, to add a folder of fonts the app ships or downloads.
    pub fn database_mut(&mut self) -> &mut fontdb::Database {
        &mut self.db
    }

    /// How many faces are installed.
    pub fn len(&self) -> usize {
        self.db.len()
    }

    pub fn is_empty(&self) -> bool {
        self.db.is_empty()
    }

    /// Does face `id` have an outline for `ch`? A face with only colour
    /// bitmaps (an emoji font) has none libgui can draw, so it does not.
    fn has(&self, id: fontdb::ID, ch: char) -> bool {
        self.db
            .with_face_data(id, |data, index| {
                let Ok(face) = ttf_parser::Face::parse(data, index) else { return false };
                let t = face.tables();
                (t.glyf.is_some() || t.cff.is_some() || t.cff2.is_some()) && face.glyph_index(ch).is_some_and(|g| g.0 != 0)
            })
            .unwrap_or(false)
    }

    fn load(&self, id: fontdb::ID) -> Option<Box<dyn FontRasterizer>> {
        self.db
            .with_face_data(id, |data, index| FontdueRasterizer::from_collection(data, index).ok())
            .flatten()
            .map(|f| Box::new(f) as Box<dyn FontRasterizer>)
    }

    /// The upright, regular face of `family`, if installed.
    fn family(&self, family: &str) -> Option<fontdb::ID> {
        self.db.query(&fontdb::Query { families: &[fontdb::Family::Name(family)], ..Default::default() })
    }

    /// The platform's own fonts for `script`, best first.
    fn preferred(&self, script: Script) -> &'static [&'static str] {
        let han = match self.language {
            Language::SimplifiedChinese => Script::Han,
            Language::TraditionalChinese => return families(Want::HanTraditional),
            Language::Japanese => Script::Kana,
            Language::Korean => Script::Hangul,
        };
        let script = if script == Script::Han { han } else { script };
        match script {
            Script::Han => families(Want::HanSimplified),
            Script::Kana => families(Want::Japanese),
            Script::Hangul => families(Want::Korean),
            Script::Arabic => families(Want::Arabic),
            Script::Hebrew => families(Want::Hebrew),
            Script::Devanagari => families(Want::Devanagari),
            Script::Bengali => families(Want::Bengali),
            Script::Tamil => families(Want::Tamil),
            Script::Thai => families(Want::Thai),
            Script::Other => &[],
        }
    }
}

impl FontSource for SystemFonts {
    fn face_for(&self, ch: char) -> Option<Box<dyn FontRasterizer>> {
        // The platform's choice for the script first.
        for name in self.preferred(script_of(ch)) {
            if let Some(id) = self.family(name) {
                if self.has(id, ch) {
                    if let Some(face) = self.load(id) {
                        return Some(face);
                    }
                }
            }
        }
        // Then anything installed: regular weight and upright first, so a
        // symbol does not come from a hairline italic.
        let mut faces: Vec<&fontdb::FaceInfo> = self.db.faces().collect();
        faces.sort_by_key(|f| (f.style != fontdb::Style::Normal, f.weight.0.abs_diff(400), f.monospaced));
        faces.into_iter().filter(|f| self.has(f.id, ch)).find_map(|f| self.load(f.id))
    }
}

/// A `Ui` drawing with `font` (your UI font, Inter for instance), falling
/// back to the system's fonts for whatever it lacks.
pub fn ui(theme: libgui::Theme, font: &[u8]) -> Result<libgui::Ui, libgui::FontError> {
    let stack = libgui::FontStack::from_fonts(&[font])?.with_source(Box::new(SystemFonts::new()));
    Ok(libgui::Ui::with_rasterizer(theme, Box::new(stack)))
}

fn script_of(ch: char) -> Script {
    match ch as u32 {
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Kana,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7AF | 0xD7B0..=0xD7FF => Script::Hangul,
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3190..=0x31EF | 0x3200..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF65 | 0x20000..=0x3FFFF => {
            Script::Han
        }
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0900..=0x097F | 0xA8E0..=0xA8FF => Script::Devanagari,
        0x0980..=0x09FF => Script::Bengali,
        0x0B80..=0x0BFF => Script::Tamil,
        0x0E00..=0x0E7F => Script::Thai,
        _ => Script::Other,
    }
}

#[derive(Clone, Copy)]
enum Want {
    HanSimplified,
    HanTraditional,
    Japanese,
    Korean,
    Arabic,
    Hebrew,
    Devanagari,
    Bengali,
    Tamil,
    Thai,
}

/// Each platform's own fonts for a script, its own first and the others'
/// after (a Linux box with Microsoft's fonts installed uses them).
fn families(want: Want) -> &'static [&'static str] {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return match want {
        Want::HanSimplified => &["PingFang SC", "Hiragino Sans GB", "STHeiti", "Heiti SC", "Noto Sans CJK SC", "Microsoft YaHei"],
        Want::HanTraditional => &["PingFang TC", "PingFang HK", "Heiti TC", "Noto Sans CJK TC", "Microsoft JhengHei"],
        Want::Japanese => &["Hiragino Sans", "Hiragino Kaku Gothic ProN", "Noto Sans CJK JP", "Yu Gothic"],
        Want::Korean => &["Apple SD Gothic Neo", "Noto Sans CJK KR", "Malgun Gothic"],
        Want::Arabic => &["Geeza Pro", "SF Arabic", "Noto Sans Arabic"],
        Want::Hebrew => &["Arial Hebrew", "SF Hebrew", "Lucida Grande", "Noto Sans Hebrew"],
        Want::Devanagari => &["Kohinoor Devanagari", "Devanagari Sangam MN", "Noto Sans Devanagari"],
        Want::Bengali => &["Kohinoor Bangla", "Bangla Sangam MN", "Noto Sans Bengali"],
        Want::Tamil => &["Tamil Sangam MN", "Noto Sans Tamil"],
        Want::Thai => &["Thonburi", "Noto Sans Thai"],
    };
    #[cfg(target_os = "windows")]
    return match want {
        Want::HanSimplified => &["Microsoft YaHei", "Microsoft YaHei UI", "SimSun", "Noto Sans CJK SC"],
        Want::HanTraditional => &["Microsoft JhengHei", "Microsoft JhengHei UI", "MingLiU", "Noto Sans CJK TC"],
        Want::Japanese => &["Yu Gothic UI", "Yu Gothic", "Meiryo", "MS Gothic", "Noto Sans CJK JP"],
        Want::Korean => &["Malgun Gothic", "Gulim", "Noto Sans CJK KR"],
        Want::Arabic => &["Segoe UI", "Tahoma", "Noto Sans Arabic"],
        Want::Hebrew => &["Segoe UI", "Arial", "Noto Sans Hebrew"],
        Want::Devanagari => &["Nirmala UI", "Mangal", "Noto Sans Devanagari"],
        Want::Bengali => &["Nirmala UI", "Vrinda", "Noto Sans Bengali"],
        Want::Tamil => &["Nirmala UI", "Latha", "Noto Sans Tamil"],
        Want::Thai => &["Leelawadee UI", "Tahoma", "Noto Sans Thai"],
    };
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "windows")))]
    return match want {
        Want::HanSimplified => &["Noto Sans CJK SC", "Noto Sans SC", "Source Han Sans SC", "WenQuanYi Micro Hei", "Droid Sans Fallback"],
        Want::HanTraditional => &["Noto Sans CJK TC", "Noto Sans TC", "Source Han Sans TC", "Droid Sans Fallback"],
        Want::Japanese => &["Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP", "IPAGothic", "Droid Sans Fallback"],
        Want::Korean => &["Noto Sans CJK KR", "Noto Sans KR", "Source Han Sans KR", "NanumGothic", "Droid Sans Fallback"],
        Want::Arabic => &["Noto Sans Arabic", "Noto Naskh Arabic", "DejaVu Sans"],
        Want::Hebrew => &["Noto Sans Hebrew", "DejaVu Sans"],
        Want::Devanagari => &["Noto Sans Devanagari", "Lohit Devanagari"],
        Want::Bengali => &["Noto Sans Bengali", "Lohit Bengali"],
        Want::Tamil => &["Noto Sans Tamil", "Lohit Tamil"],
        Want::Thai => &["Noto Sans Thai", "Loma"],
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts() {
        assert_eq!(script_of('文'), Script::Han);
        assert_eq!(script_of('か'), Script::Kana);
        assert_eq!(script_of('한'), Script::Hangul);
        assert_eq!(script_of('ع'), Script::Arabic);
        assert_eq!(script_of('א'), Script::Hebrew);
        assert_eq!(script_of('क'), Script::Devanagari);
        assert_eq!(script_of('a'), Script::Other);
    }

    #[test]
    fn han_follows_the_language() {
        let none = SystemFonts::with_database(fontdb::Database::new());
        assert_eq!(none.preferred(Script::Han), families(Want::HanSimplified));
        let ja = SystemFonts::with_database(fontdb::Database::new()).with_language("ja-JP");
        assert_eq!(ja.preferred(Script::Han), families(Want::Japanese));
        let tw = SystemFonts::with_database(fontdb::Database::new()).with_language("zh-TW");
        assert_eq!(tw.preferred(Script::Han), families(Want::HanTraditional));
        // Kana is Japanese whatever the language says.
        let zh = SystemFonts::with_database(fontdb::Database::new()).with_language("zh-Hans");
        assert_eq!(zh.preferred(Script::Kana), families(Want::Japanese));
    }

    #[test]
    fn an_empty_database_has_nothing() {
        let none = SystemFonts::with_database(fontdb::Database::new());
        assert!(none.face_for('文').is_none());
    }
}
