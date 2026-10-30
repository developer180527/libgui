//! Markdown drawn with libgui: CommonMark, plus tables, strikethrough and
//! task lists, as GitHub writes them.
//!
//! ```no_run
//! # use libgui::*;
//! # fn f(ui: &mut Ui, readme: &str) {
//! // Parse when the text changes, not every frame.
//! let doc = libgui_markdown::Markdown::parse(readme);
//! // Each frame:
//! let r = doc.show(ui, "readme");
//! if let Some(url) = r.clicked_link {
//!     // Open it: libgui never opens anything itself.
//! #   let _ = url;
//! }
//! # }
//! ```
//!
//! It is built from libgui's own pieces, so it follows the theme, the scale
//! and the window like any other widget: paragraphs and headings are
//! [`Ui::rich_text`] in the faces of [`FontRole`] (set bold, italic and mono
//! faces with [`Ui::set_font_role`], or they fall back to the body face), code
//! blocks are drawn in the mono face with an optional highlighter, tables and
//! lists are containers.
//!
//! **What the app decides.** A link is reported, not followed. An image is
//! drawn only if [`MarkdownOptions::image`] turns its URL into a texture the
//! app has loaded; otherwise its alt text is shown. Raw HTML is shown as
//! text, not interpreted.
//!
//! **Not done:** footnotes, math, and horizontal scrolling of a long code
//! line (it is clipped).

use libgui::{Align, Color, FontRole, Frame, Id, Insets, Layout, RichText, Size, TextRun, TextSpan, TextureId, Ui, Vec2};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::cell::RefCell;
use std::rc::Rc;

/// A parsed Markdown document. Parse it when its text changes; draw it every
/// frame with [`Markdown::show`].
#[derive(Debug, Default)]
pub struct Markdown {
    blocks: Vec<Block>,
    links: Vec<String>,
    /// The paragraphs as rich text, built for one set of faces, sizes and
    /// colours and kept until those change.
    built: RefCell<Option<(u64, Paragraphs)>>,
}

/// Every paragraph of a document as rich text, in drawing order.
type Paragraphs = Rc<Vec<Rc<RichText>>>;

/// What [`Markdown::show`] reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarkdownResponse {
    /// A link clicked this frame, as written in the source.
    pub clicked_link: Option<String>,
    /// The link under the pointer, for a status bar or a tooltip.
    pub hovered_link: Option<String>,
}

/// How to draw what Markdown leaves to the app.
#[derive(Default)]
pub struct MarkdownOptions<'a> {
    /// Colours for a code block's lines: `(language, line, spans)`, the
    /// language as written after the fence ("rust"), empty if none.
    #[allow(clippy::type_complexity)]
    pub highlight: Option<&'a dyn Fn(&str, &str, &mut Vec<TextSpan>)>,
    /// The texture and size (logical px) of an image URL the app has loaded,
    /// or `None` to show the alt text.
    #[allow(clippy::type_complexity)]
    pub image: Option<&'a dyn Fn(&str) -> Option<(TextureId, Vec2)>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    /// Index into `links`, plus one; 0 for none.
    link: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum Inline {
    Text(String, Style),
    Image { url: String, alt: String },
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Paragraph(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Code { lang: String, text: String },
    List { start: Option<u64>, items: Vec<Item> },
    Quote(Vec<Block>),
    Rule,
    Table { align: Vec<Align>, head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Item {
    blocks: Vec<Block>,
    task: Option<bool>,
}

impl Markdown {
    /// Parse `source`. CommonMark, with GitHub's tables, strikethrough and
    /// task lists.
    pub fn parse(source: &str) -> Self {
        let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let mut b = Builder::default();
        for ev in Parser::new_ext(source, opts) {
            b.event(ev);
        }
        b.flush();
        let blocks = match b.stack.pop() {
            Some(Container::Root(v)) => v,
            _ => Vec::new(),
        };
        Self { blocks, links: b.links, built: RefCell::new(None) }
    }

    /// Every link in the document, in order, as written.
    pub fn links(&self) -> &[String] {
        &self.links
    }

    /// Draw the document, filling the width it is given.
    pub fn show(&self, ui: &mut Ui, key: &str) -> MarkdownResponse {
        self.show_with(ui, key, &MarkdownOptions::default())
    }

    pub fn show_with(&self, ui: &mut Ui, key: &str, opts: &MarkdownOptions) -> MarkdownResponse {
        let look = Look::of(ui);
        let rich = self.rich(&look);
        let mut cx = Cx { look, rich, next: 0, links: &self.links, opts, out: MarkdownResponse::default() };
        let gap = (cx.look.size * 0.75).round();
        ui.container_id(Id::from_name(key), Layout::column().height(Size::Fit).gap(gap), Frame::none(), |ui| {
            for b in &self.blocks {
                cx.block(ui, b);
            }
        });
        cx.out
    }

    /// The paragraphs' rich text for these faces and colours, built once.
    fn rich(&self, look: &Look) -> Paragraphs {
        let key = look.key();
        if let Some((k, v)) = &*self.built.borrow() {
            if *k == key {
                return v.clone();
            }
        }
        let mut out = Vec::new();
        for b in &self.blocks {
            collect(b, look, &mut out);
        }
        let v = Rc::new(out);
        *self.built.borrow_mut() = Some((key, v.clone()));
        v
    }
}

// ---- parsing ------------------------------------------------------------

#[derive(Debug)]
enum Container {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Item),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Para,
    Heading(u8),
    Cell,
}

#[derive(Debug, Default)]
struct TableB {
    align: Vec<Align>,
    head: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    row: Vec<Vec<Inline>>,
}

#[derive(Debug)]
struct Builder {
    stack: Vec<Container>,
    inline: Option<(Target, Vec<Inline>)>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Vec<u32>,
    links: Vec<String>,
    code: Option<(String, String)>,
    image: Option<(String, String)>,
    table: Option<TableB>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            stack: vec![Container::Root(Vec::new())],
            inline: None,
            bold: 0,
            italic: 0,
            strike: 0,
            link: Vec::new(),
            links: Vec::new(),
            code: None,
            image: None,
            table: None,
        }
    }
}

impl Builder {
    fn push_block(&mut self, b: Block) {
        match self.stack.last_mut() {
            Some(Container::Root(v)) | Some(Container::Quote(v)) => v.push(b),
            Some(Container::Item(i)) => i.blocks.push(b),
            // Content straight inside a list belongs to its last item.
            Some(Container::List(_, items)) => match items.last_mut() {
                Some(i) => i.blocks.push(b),
                None => items.push(Item { blocks: vec![b], task: None }),
            },
            None => {}
        }
    }

    /// Close the paragraph, heading or cell being filled.
    fn flush(&mut self) {
        let Some((target, inl)) = self.inline.take() else { return };
        match target {
            Target::Para if !inl.is_empty() => self.push_block(Block::Paragraph(inl)),
            Target::Para => {}
            Target::Heading(l) => self.push_block(Block::Heading(l, inl)),
            Target::Cell => {
                if let Some(t) = &mut self.table {
                    t.row.push(inl);
                }
            }
        }
    }

    fn open(&mut self, t: Target) {
        self.flush();
        self.inline = Some((t, Vec::new()));
    }

    fn style(&self) -> Style {
        Style { bold: self.bold > 0, italic: self.italic > 0, strike: self.strike > 0, code: false, link: self.link.last().copied().unwrap_or(0) }
    }

    fn text(&mut self, t: &str, style: Style) {
        if self.inline.is_none() {
            // Text in a tight list item comes with no paragraph around it.
            self.inline = Some((Target::Para, Vec::new()));
        }
        let Some((_, inl)) = &mut self.inline else { return };
        match inl.last_mut() {
            Some(Inline::Text(s, st)) if *st == style => s.push_str(t),
            _ => inl.push(Inline::Text(t.to_string(), style)),
        }
    }

    fn event(&mut self, ev: Event) {
        if let Some((_, code)) = &mut self.code {
            match ev {
                Event::Text(t) => code.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, mut text) = self.code.take().unwrap_or_default();
                    while text.ends_with('\n') {
                        text.pop();
                    }
                    self.push_block(Block::Code { lang, text });
                }
                _ => {}
            }
            return;
        }
        if let Some((_, alt)) = &mut self.image {
            match ev {
                Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                Event::End(TagEnd::Image) => {
                    let (url, alt) = self.image.take().unwrap_or_default();
                    if self.inline.is_none() {
                        self.inline = Some((Target::Para, Vec::new()));
                    }
                    if let Some((_, inl)) = &mut self.inline {
                        inl.push(Inline::Image { url, alt });
                    }
                }
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph | Tag::HtmlBlock => self.open(Target::Para),
                Tag::Heading { level, .. } => self.open(Target::Heading(level_num(level))),
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.stack.push(Container::Quote(Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    self.flush();
                    self.stack.push(Container::List(start, Vec::new()));
                }
                Tag::Item => {
                    self.flush();
                    self.stack.push(Container::Item(Item::default()));
                }
                Tag::Table(al) => {
                    self.flush();
                    let align = al
                        .iter()
                        .map(|a| match a {
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::End,
                            _ => Align::Start,
                        })
                        .collect();
                    self.table = Some(TableB { align, ..Default::default() });
                }
                Tag::TableHead | Tag::TableRow => {
                    if let Some(t) = &mut self.table {
                        t.row.clear();
                    }
                }
                Tag::TableCell => self.open(Target::Cell),
                Tag::Emphasis => self.italic += 1,
                Tag::Strong => self.bold += 1,
                Tag::Strikethrough => self.strike += 1,
                Tag::Link { dest_url, .. } => {
                    self.links.push(dest_url.to_string());
                    self.link.push(self.links.len() as u32);
                }
                Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::HtmlBlock | TagEnd::TableCell => self.flush(),
                TagEnd::BlockQuote(_) => {
                    self.flush();
                    if let Some(Container::Quote(v)) = self.stack.pop() {
                        self.push_block(Block::Quote(v));
                    }
                }
                TagEnd::List(_) => {
                    self.flush();
                    if let Some(Container::List(start, items)) = self.stack.pop() {
                        self.push_block(Block::List { start, items });
                    }
                }
                TagEnd::Item => {
                    self.flush();
                    if let Some(Container::Item(item)) = self.stack.pop() {
                        if let Some(Container::List(_, items)) = self.stack.last_mut() {
                            items.push(item);
                        }
                    }
                }
                TagEnd::TableHead => {
                    if let Some(t) = &mut self.table {
                        t.head = std::mem::take(&mut t.row);
                    }
                }
                TagEnd::TableRow => {
                    if let Some(t) = &mut self.table {
                        let row = std::mem::take(&mut t.row);
                        t.rows.push(row);
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = self.table.take() {
                        self.push_block(Block::Table { align: t.align, head: t.head, rows: t.rows });
                    }
                }
                TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
                TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
                TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
                TagEnd::Link => {
                    self.link.pop();
                }
                _ => {}
            },
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                let s = self.style();
                self.text(&t, s);
            }
            Event::Code(t) => {
                let s = Style { code: true, ..self.style() };
                self.text(&t, s);
            }
            Event::SoftBreak => {
                let s = self.style();
                self.text(" ", s);
            }
            Event::HardBreak => {
                let s = self.style();
                self.text("\n", s);
            }
            Event::Rule => {
                self.flush();
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some(Container::Item(i)) = self.stack.last_mut() {
                    i.task = Some(done);
                }
            }
            _ => {}
        }
    }
}

fn level_num(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ---- drawing ------------------------------------------------------------

/// The faces, sizes and colours a document is drawn with, from the `Ui`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    fonts: [libgui::FontId; 5],
    size: f32,
    text: Color,
    muted: Color,
    link: Color,
    inset: Color,
    border: Color,
    accent: Color,
    radius: f32,
}

impl Look {
    fn of(ui: &Ui) -> Self {
        let p = &ui.theme.palette;
        Self {
            fonts: FontRole::ALL.map(|r| ui.font_for(r)),
            size: ui.theme.metrics.font_size,
            text: p.text,
            muted: p.text_muted,
            link: p.accent,
            inset: p.bg_inset,
            border: p.border,
            accent: p.accent,
            radius: ui.theme.metrics.radius,
        }
    }

    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for f in self.fonts {
            f.0.hash(&mut h);
        }
        for v in [self.size, self.text.r, self.text.g, self.text.b, self.text.a, self.link.r, self.link.g, self.link.b, self.inset.r, self.inset.g, self.inset.b, self.muted.r] {
            v.to_bits().hash(&mut h);
        }
        h.finish()
    }

    fn font(&self, r: FontRole) -> libgui::FontId {
        self.fonts[r as usize]
    }

    fn heading_size(&self, level: u8) -> f32 {
        let k = [2.0, 1.6, 1.3, 1.12, 1.0, 0.9][(level.clamp(1, 6) - 1) as usize];
        (self.size * k).round()
    }

    fn mono_size(&self) -> f32 {
        (self.size * 0.95).round()
    }
}

/// `inlines` as rich text: base size `size`, all bold if `bold`.
fn to_rich(inlines: &[Inline], look: &Look, size: f32, bold: bool, color: Color) -> RichText {
    let mut rich = RichText::new();
    for i in inlines {
        match i {
            Inline::Text(t, st) => {
                let role = FontRole::of(bold || st.bold, st.italic, st.code);
                let size = if st.code { (size * 0.95).round() } else { size };
                let mut run = TextRun::new(t.as_str(), look.font(role), size, if st.link > 0 { look.link } else { color });
                if st.code {
                    run.background = look.inset;
                }
                run.strike = st.strike;
                run.underline = st.link > 0;
                run.tag = st.link;
                rich.push(run);
            }
            Inline::Image { alt, .. } => {
                let mut run = TextRun::new(if alt.is_empty() { "[image]" } else { alt.as_str() }, look.font(FontRole::Italic), size, look.muted);
                run.tag = 0;
                rich.push(run);
            }
        }
    }
    rich
}

/// Every paragraph's rich text, in the order `Cx::block` draws them.
fn collect(b: &Block, look: &Look, out: &mut Vec<Rc<RichText>>) {
    match b {
        Block::Paragraph(i) => out.push(Rc::new(to_rich(i, look, look.size, false, look.text))),
        Block::Heading(l, i) => out.push(Rc::new(to_rich(i, look, look.heading_size(*l), true, look.text))),
        Block::List { items, .. } => {
            for it in items {
                for b in &it.blocks {
                    collect(b, look, out);
                }
            }
        }
        Block::Quote(bs) => {
            let quoted = Look { text: look.muted, ..*look };
            for b in bs {
                collect(b, &quoted, out);
            }
        }
        Block::Table { head, rows, .. } => {
            for c in head {
                out.push(Rc::new(to_rich(c, look, look.size, true, look.text)));
            }
            for r in rows {
                for c in r {
                    out.push(Rc::new(to_rich(c, look, look.size, false, look.text)));
                }
            }
        }
        Block::Code { .. } | Block::Rule => {}
    }
}

struct Cx<'a> {
    look: Look,
    rich: Paragraphs,
    next: usize,
    links: &'a [String],
    opts: &'a MarkdownOptions<'a>,
    out: MarkdownResponse,
}

impl Cx<'_> {
    fn take_rich(&mut self) -> Rc<RichText> {
        let r = self.rich.get(self.next).cloned().unwrap_or_default();
        self.next += 1;
        r
    }

    fn rich_text(&mut self, ui: &mut Ui, align: Align) {
        let n = self.next;
        let rich = self.take_rich();
        let r = ui.rich_text_with(("md", n), rich, align);
        let link = |t: u32| (t > 0).then(|| self.links.get(t as usize - 1).cloned()).flatten();
        if let Some(l) = link(r.clicked_tag) {
            self.out.clicked_link = Some(l);
        }
        if let Some(l) = link(r.hovered_tag) {
            self.out.hovered_link = Some(l);
        }
    }

    fn block(&mut self, ui: &mut Ui, b: &Block) {
        let look = self.look;
        match b {
            Block::Paragraph(inl) => {
                if let Some((tex, size)) = self.only_image(inl) {
                    self.next += 1;
                    let n = self.next;
                    ui.add_leaf(Id::new(("md-img", n)), Layout::leaf(Size::Fixed(size.x), Size::Fixed(size.y)), size, false, move |p, r| {
                        p.image(r, tex, 0.0);
                    });
                } else {
                    self.rich_text(ui, Align::Start);
                }
            }
            Block::Heading(level, _) => {
                self.rich_text(ui, Align::Start);
                if *level <= 2 {
                    ui.separator();
                }
            }
            Block::Rule => ui.separator(),
            Block::Code { lang, text } => self.code(ui, lang, text),
            Block::Quote(blocks) => {
                let bar = look.border;
                let inner = Look { text: look.muted, ..look };
                ui.container(Layout::row().height(Size::Fit).gap(10.0).align(Align::Start, Align::Start), Frame::none(), |ui| {
                    let n = self.next;
                    ui.add_leaf(Id::new(("md-bar", n)), Layout::leaf(Size::Fixed(3.0), Size::Grow(1.0)), Vec2::new(3.0, 0.0), false, move |p, r| {
                        p.rect(r, bar, 1.5);
                    });
                    ui.container(Layout::column().height(Size::Fit).gap((look.size * 0.5).round()), Frame::none(), |ui| {
                        let outer = std::mem::replace(&mut self.look, inner);
                        for b in blocks {
                            self.block(ui, b);
                        }
                        self.look = outer;
                    });
                });
            }
            Block::List { start, items } => {
                let lh = ui.fonts.line_height(look.font(FontRole::Body), look.size);
                ui.container(Layout::column().height(Size::Fit).gap((look.size * 0.3).round()), Frame::none(), |ui| {
                    for (k, item) in items.iter().enumerate() {
                        ui.container(Layout::row().height(Size::Fit).gap(4.0).align(Align::Start, Align::Start), Frame::none(), |ui| {
                            let marker = match (item.task, start) {
                                (Some(done), _) => Marker::Task(done),
                                (None, Some(s)) => Marker::Number(s + k as u64),
                                (None, None) => Marker::Bullet,
                            };
                            self.marker(ui, marker, lh);
                            ui.container(Layout::column().height(Size::Fit).gap((look.size * 0.4).round()), Frame::none(), |ui| {
                                for b in &item.blocks {
                                    self.block(ui, b);
                                }
                            });
                        });
                    }
                });
            }
            Block::Table { align, head, rows } => {
                let frame = Frame { fill: Color::TRANSPARENT, border: look.border, border_width: 1.0, radius: look.radius, shadow: false, clip: true };
                let pad = Insets::xy((look.size * 0.6).round(), (look.size * 0.35).round());
                ui.container(Layout::column().height(Size::Fit), frame, |ui| {
                    let header = Frame { fill: look.inset, ..Frame::none() };
                    for (r, cells) in std::iter::once(head).chain(rows.iter()).enumerate() {
                        if r > 1 {
                            ui.separator();
                        }
                        ui.container(Layout::row().height(Size::Fit).align(Align::Start, Align::Start), if r == 0 { header } else { Frame::none() }, |ui| {
                            for (c, _) in cells.iter().enumerate() {
                                let a = align.get(c).copied().unwrap_or(Align::Start);
                                ui.container(Layout::column().height(Size::Fit).padding(pad), Frame::none(), |ui| self.rich_text(ui, a));
                            }
                        });
                    }
                });
            }
        }
    }

    /// A paragraph that is one image the app can draw.
    fn only_image(&self, inl: &[Inline]) -> Option<(TextureId, Vec2)> {
        let f = self.opts.image?;
        let mut img = None;
        for i in inl {
            match i {
                Inline::Image { url, .. } if img.is_none() => img = Some(url),
                Inline::Text(t, _) if t.trim().is_empty() => {}
                _ => return None,
            }
        }
        f(img?)
    }

    fn marker(&mut self, ui: &mut Ui, m: Marker, lh: f32) {
        let look = self.look;
        let w = (look.size * 1.6).round();
        let n = self.next;
        let font = look.font(FontRole::Body);
        let text = match m {
            Marker::Bullet => "•".to_string(),
            Marker::Number(k) => format!("{k}."),
            Marker::Task(_) => String::new(),
        };
        let shown = ui.frame_text(&text);
        let w = w.max(ui.fonts.measure(font, look.size, &text).x + 6.0);
        let (color, accent, border) = (look.muted, look.accent, look.border);
        ui.add_leaf(Id::new(("md-mark", n, ui.open_depth())), Layout::leaf(Size::Fixed(w), Size::Fixed(lh)), Vec2::new(w, lh), false, move |p, r| {
            let size = look.size;
            match m {
                Marker::Task(done) => {
                    let b = (size * 0.9).round();
                    let bx = libgui::Rect::new(r.x + w - b - 6.0, r.y + ((r.h - b) * 0.5).round(), b, b);
                    if done {
                        p.rect(bx, accent, 3.0);
                        let s = b / 14.0;
                        p.line(Vec2::new(bx.x + 3.0 * s, bx.y + 7.5 * s), Vec2::new(bx.x + 6.0 * s, bx.y + 10.5 * s), 1.6, Color::WHITE);
                        p.line(Vec2::new(bx.x + 6.0 * s, bx.y + 10.5 * s), Vec2::new(bx.x + 11.0 * s, bx.y + 4.0 * s), 1.6, Color::WHITE);
                    } else {
                        p.rect_bordered(bx, Color::TRANSPARENT, 3.0, 1.0, border);
                    }
                }
                _ => {
                    let old = p.font;
                    p.font = font;
                    p.text_right(libgui::Rect::new(r.x, r.y, w - 6.0, r.h), size, color, shown);
                    p.font = old;
                }
            }
        });
    }

    fn code(&mut self, ui: &mut Ui, lang: &str, text: &str) {
        let look = self.look;
        let font = look.font(FontRole::Mono);
        let size = look.mono_size();
        let lh = ui.fonts.line_height(font, size);
        let lines: Vec<&str> = text.split('\n').collect();
        let mut spans: Vec<Vec<TextSpan>> = Vec::with_capacity(lines.len());
        for l in &lines {
            let mut s = Vec::new();
            if let Some(h) = self.opts.highlight {
                h(lang, l, &mut s);
            }
            spans.push(s);
        }
        let shown: Vec<libgui::FrameText> = lines.iter().map(|l| ui.frame_text(l)).collect();
        let pad = (look.size * 0.75).round();
        let h = lh * lines.len() as f32 + pad * 2.0;
        let (fill, color, radius) = (look.inset, look.text, look.radius);
        let n = self.next;
        ui.add_leaf(Id::new(("md-code", n, lines.len(), ui.open_depth())), Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(0.0, h), false, move |p, r| {
            p.rect(r, fill, radius);
            p.draw.push_clip(r);
            let old = p.font;
            p.font = font;
            for (k, line) in shown.iter().enumerate() {
                p.text_spans(Vec2::new(r.x + pad, r.y + pad + lh * k as f32), size, color, *line, &spans[k]);
            }
            p.font = old;
            p.draw.pop_clip();
        });
    }
}

#[derive(Clone, Copy)]
enum Marker {
    Bullet,
    Number(u64),
    Task(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(i: &[Inline]) -> String {
        i.iter()
            .map(|x| match x {
                Inline::Text(t, _) => t.clone(),
                Inline::Image { alt, .. } => alt.clone(),
            })
            .collect()
    }

    #[test]
    fn blocks_are_parsed() {
        let md = Markdown::parse("# Title\n\nSome **bold** and *it* and `code` and [a link](https://x.io).\n\n- one\n- [x] two\n\n> quoted\n\n```rust\nfn main() {}\n```\n\n---\n\n| a | b |\n|:--|--:|\n| 1 | 2 |\n");
        let kinds: Vec<&str> = md
            .blocks
            .iter()
            .map(|b| match b {
                Block::Heading(..) => "h",
                Block::Paragraph(_) => "p",
                Block::List { .. } => "list",
                Block::Quote(_) => "quote",
                Block::Code { .. } => "code",
                Block::Rule => "rule",
                Block::Table { .. } => "table",
            })
            .collect();
        assert_eq!(kinds, vec!["h", "p", "list", "quote", "code", "rule", "table"]);
        let Block::Paragraph(p) = &md.blocks[1] else { unreachable!() };
        assert_eq!(text(p), "Some bold and it and code and a link.");
        let styles: Vec<Style> = p.iter().filter_map(|i| if let Inline::Text(_, s) = i { Some(*s) } else { None }).collect();
        assert!(styles.iter().any(|s| s.bold) && styles.iter().any(|s| s.italic) && styles.iter().any(|s| s.code));
        assert!(styles.iter().any(|s| s.link == 1));
        assert_eq!(md.links(), &["https://x.io".to_string()]);
        let Block::List { start, items } = &md.blocks[2] else { unreachable!() };
        assert_eq!(*start, None);
        assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), vec![None, Some(true)]);
        let Block::Code { lang, text } = &md.blocks[4] else { unreachable!() };
        assert_eq!((lang.as_str(), text.as_str()), ("rust", "fn main() {}"));
        let Block::Table { align, head, rows } = &md.blocks[6] else { unreachable!() };
        assert_eq!(align, &vec![Align::Start, Align::End]);
        assert_eq!((head.len(), rows.len(), rows[0].len()), (2, 1, 2));
    }

    #[test]
    fn nested_lists_and_ordered_starts() {
        let md = Markdown::parse("3. three\n4. four\n   - inner\n");
        let Block::List { start, items } = &md.blocks[0] else { panic!("{:?}", md.blocks) };
        assert_eq!(*start, Some(3));
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1].blocks.last(), Some(Block::List { start: None, .. })), "{:?}", items[1].blocks);
    }

    #[test]
    fn a_hard_break_is_a_newline() {
        let md = Markdown::parse("one  \ntwo\nthree");
        let Block::Paragraph(p) = &md.blocks[0] else { panic!() };
        assert_eq!(text(p), "one\ntwo three");
    }
}
