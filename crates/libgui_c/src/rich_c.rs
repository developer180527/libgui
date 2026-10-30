//! Font roles, rich text and Markdown from C.

use crate::convert::str_or_empty;
use crate::handle::{set_error, with_ui, LibguiUi};
use crate::types::{LibguiColor, LibguiResponse};
use libgui::{Align, Color, FontRole, RichText, TextRun, Ui};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_char;
use std::rc::Rc;

fn role(r: u32) -> Option<FontRole> {
    FontRole::ALL.get(r as usize).copied()
}

fn color(c: LibguiColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

/// Use these fonts for a role: 0 body, 1 bold, 2 italic, 3 bold italic,
/// 4 mono. `fonts` is a fallback chain like
/// [`libgui_ui_new_with_fallbacks`](crate::libgui_ui_new_with_fallbacks)'s;
/// with `system_fonts` nonzero the system's installed fonts come after it
/// (when this library has its `system-fonts` feature). Returns 1, or 0 with
/// the reason in `libgui_last_error`.
///
/// # Safety
/// `ui` null or live; `fonts` and `lens` `count` readable entries.
#[no_mangle]
pub unsafe extern "C" fn libgui_set_font_role(
    ui: *mut LibguiUi,
    role_index: u32,
    fonts: *const *const u8,
    lens: *const u64,
    count: u64,
    system_fonts: u8,
) -> u8 {
    let what = "libgui_set_font_role";
    let Some(role) = role(role_index) else {
        set_error(&format!("{what}: no role {role_index} (0 body, 1 bold, 2 italic, 3 bold italic, 4 mono)"));
        return 0;
    };
    if fonts.is_null() || lens.is_null() || count == 0 {
        set_error(&format!("{what}: no fonts"));
        return 0;
    }
    let mut owned: Vec<&[u8]> = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let (p, n) = unsafe { (*fonts.add(i), *lens.add(i)) };
        if p.is_null() || n == 0 {
            set_error(&format!("{what}: a font is empty"));
            return 0;
        }
        owned.push(unsafe { std::slice::from_raw_parts(p, n as usize) });
    }
    let stack = match libgui::FontStack::from_fonts(&owned) {
        Ok(s) => s,
        Err(e) => {
            set_error(&format!("{what}: {e}"));
            return 0;
        }
    };
    #[cfg(feature = "system-fonts")]
    let stack = if system_fonts != 0 { stack.with_source(Box::new(libgui_sysfonts::SystemFonts::new())) } else { stack };
    #[cfg(not(feature = "system-fonts"))]
    let _ = system_fonts;
    with_ui(ui, 0, |u: &mut Ui| {
        let id = u.fonts.add_rasterizer(Box::new(stack));
        u.set_font_role(role, id);
        1
    })
}

/// One stretch of rich text in one style. Mirrors [`libgui::TextRun`], with
/// the face named by role and zeros meaning the theme's.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiTextRun {
    pub text: *const c_char,
    /// 0 body, 1 bold, 2 italic, 3 bold italic, 4 mono.
    pub role: u32,
    /// Logical px; 0 for the theme's font size.
    pub size: f32,
    /// Alpha 0 for the theme's text colour.
    pub color: LibguiColor,
    /// Alpha 0 for none.
    pub background: LibguiColor,
    pub underline: u8,
    pub strike: u8,
    pub _pad: [u8; 2],
    /// Reported when hovered or clicked; 0 for none.
    pub tag: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiRichResponse {
    pub response: LibguiResponse,
    pub hovered_tag: u32,
    pub clicked_tag: u32,
}

/// A wrapped paragraph of `count` runs, filling the width it is given.
/// `align`: 0 start, 1 centre, 2 end.
///
/// # Safety
/// `ui` null or live; `runs` `count` readable runs with valid strings.
#[no_mangle]
pub unsafe extern "C" fn libgui_rich_text(ui: *mut LibguiUi, key: *const c_char, runs: *const LibguiTextRun, count: u64, align: u32) -> LibguiRichResponse {
    let what = "libgui_rich_text";
    let key = unsafe { str_or_empty(key, what) };
    let runs: &[LibguiTextRun] = if runs.is_null() || count == 0 { &[] } else { unsafe { std::slice::from_raw_parts(runs, count as usize) } };
    let texts: Vec<&str> = runs.iter().map(|r| unsafe { str_or_empty(r.text, what) }).collect();
    with_ui(ui, LibguiRichResponse::default(), |u| {
        let mut rich = RichText::new();
        for (r, t) in runs.iter().zip(&texts) {
            let size = if r.size > 0.0 { r.size } else { u.theme.metrics.font_size };
            let c = if r.color.a > 0.0 { color(r.color) } else { u.theme.palette.text };
            let mut run = TextRun::new(*t, u.font_for(role(r.role).unwrap_or(FontRole::Body)), size, c);
            run.background = color(r.background);
            run.underline = r.underline != 0;
            run.strike = r.strike != 0;
            run.tag = r.tag;
            rich.push(run);
        }
        let align = match align {
            1 => Align::Center,
            2 => Align::End,
            _ => Align::Start,
        };
        let r = u.rich_text_with(key, Rc::new(rich), align);
        LibguiRichResponse { response: r.response.into(), hovered_tag: r.hovered_tag, clicked_tag: r.clicked_tag }
    })
}

/// A document's last source and its parse.
type Doc = (String, Rc<libgui_markdown::Markdown>);

thread_local! {
    /// Each document by the Ui and key it is shown with: parsed again only
    /// when the source changes.
    static DOCS: RefCell<HashMap<(usize, String), Doc>> = RefCell::new(HashMap::new());
}

/// Markdown, filling the width it is given: CommonMark with tables,
/// strikethrough and task lists. Parsed when `source` changes, not every
/// frame. A link clicked this frame is written to `link` (snprintf contract:
/// the return is its full length, 0 for no click); images show their alt
/// text.
///
/// # Safety
/// `ui` null or live; `source` a C string; `link` null or `link_cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn libgui_markdown(ui: *mut LibguiUi, key: *const c_char, source: *const c_char, link: *mut c_char, link_cap: u64) -> u64 {
    let what = "libgui_markdown";
    let key = unsafe { str_or_empty(key, what) };
    let source = unsafe { str_or_empty(source, what) };
    let clicked = with_ui(ui, None, |u| {
        let slot = (u as *mut Ui as usize, key.to_string());
        let doc = DOCS.with(|d| {
            let mut d = d.borrow_mut();
            match d.get(&slot) {
                Some((src, doc)) if src == source => doc.clone(),
                _ => {
                    let doc = Rc::new(libgui_markdown::Markdown::parse(source));
                    d.insert(slot, (source.to_string(), doc.clone()));
                    doc
                }
            }
        });
        doc.show(u, key).clicked_link
    });
    match clicked {
        Some(l) => crate::text::write_back(&l, link, link_cap),
        None => {
            crate::text::write_back("", link, link_cap);
            0
        }
    }
}

/// Drop the documents kept for a `Ui` that is being freed.
pub(crate) fn forget(ui: *mut Ui) {
    let _ = DOCS.try_with(|d| d.borrow_mut().retain(|k, _| k.0 != ui as usize));
}
