//! Where a line of text is allowed to break.
//!
//! Not the Unicode line breaking algorithm (UAX #14), and it does not pretend
//! to be: that needs a break-class table for every code point, which is a
//! dependency and a megabyte. This is the part of it that matters for a user
//! interface, stated plainly so you can tell what it will and will not do.
//!
//! - After a run of whitespace, which is the usual case and the one that
//!   carries the trailing space off the end of the line.
//! - After a hyphen, slash or an em/en dash, the way a URL or a hyphenated
//!   word breaks.
//! - Between two **ideographs** — CJK, kana, Hangul — which break almost
//!   anywhere, and without which Japanese or Chinese text in a panel is one
//!   line that never fits. Not before a closing bracket or after an opening
//!   one, and not before the punctuation that must not start a line, which is
//!   the part of *kinsoku shori* that a reader actually notices.
//! - Nowhere inside a word, until nothing else fits, at which point the word is
//!   cut rather than left to overflow.
//!
//! What it does not do: hyphenation dictionaries, Thai and Khmer (which need
//! word segmentation and will break only at spaces here), or the full kinsoku
//! rule set. Those belong to a real implementation behind
//! [`FontRasterizer`](crate::FontRasterizer), the same seam complex shaping
//! goes through.

/// A place a line may end, as a byte offset into the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Opportunity {
    /// Break here: the next line starts at this offset.
    pub at: usize,
    /// Offset the line being ended should be measured and drawn up to, which
    /// excludes the whitespace the break consumed.
    pub trim: usize,
}

/// Ideographs and kana break between characters; Latin does not.
fn ideographic(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'     // Hangul Jamo
        | '\u{2E80}'..='\u{303E}'   // CJK radicals, symbols and punctuation
        | '\u{3041}'..='\u{33FF}'   // Hiragana, Katakana, compatibility
        | '\u{3400}'..='\u{4DBF}'   // CJK extension A
        | '\u{4E00}'..='\u{9FFF}'   // CJK unified
        | '\u{A000}'..='\u{A4CF}'   // Yi
        | '\u{AC00}'..='\u{D7A3}'   // Hangul syllables
        | '\u{F900}'..='\u{FAFF}'   // CJK compatibility
        | '\u{FF00}'..='\u{FF9F}'   // Full-width forms
    )
}

/// Must not start a line: closing brackets, and the punctuation that clings to
/// the word before it.
fn no_line_start(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '»' | '”' | '’')
        || matches!(c, '\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}' | '\u{FF1A}' | '\u{FF1B}'
            | '\u{FF01}' | '\u{FF1F}' | '\u{3009}' | '\u{300B}' | '\u{300D}' | '\u{300F}' | '\u{3011}'
            | '\u{FF09}' | '\u{FF3D}' | '\u{FF5D}' | '\u{30FC}' | '\u{3005}')
}

/// Must not end a line: opening brackets.
fn no_line_end(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '«' | '“' | '‘')
        || matches!(c, '\u{3008}' | '\u{300A}' | '\u{300C}' | '\u{300E}' | '\u{3010}'
            | '\u{FF08}' | '\u{FF3B}' | '\u{FF5B}')
}

/// After this character a line may end, whatever follows.
fn breaks_after(c: char) -> bool {
    matches!(c, '-' | '/' | '\u{2013}' | '\u{2014}')
}

/// Every place `text` may be broken, in order. Does not include the end of the
/// text: a caller ends the last line itself.
pub(crate) fn opportunities(text: &str, out: &mut Vec<Opportunity>) {
    out.clear();
    let bytes = text.as_bytes();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '\n' {
            // A hard break is an opportunity that must be taken; the caller
            // sees it as a break whose trimmed end is before the newline.
            out.push(Opportunity { at: i + c.len_utf8(), trim: i });
            continue;
        }
        if c.is_whitespace() {
            // Run to the end of the whitespace: the break goes after all of
            // it, and the line ends before all of it.
            let start = i;
            let mut end = i + c.len_utf8();
            while let Some(&(j, n)) = it.peek() {
                if n == '\n' || !n.is_whitespace() {
                    break;
                }
                end = j + n.len_utf8();
                it.next();
            }
            if end < bytes.len() {
                out.push(Opportunity { at: end, trim: start });
            }
            continue;
        }
        let Some(&(j, next)) = it.peek() else { continue };
        if no_line_start(next) || no_line_end(c) {
            continue;
        }
        if breaks_after(c) || (ideographic(c) && ideographic(next)) || ideographic(next) || ideographic(c) {
            out.push(Opportunity { at: j, trim: j });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str) -> Vec<&str> {
        let mut o = Vec::new();
        opportunities(text, &mut o);
        o.iter().map(|b| &text[..b.trim]).collect()
    }

    #[test]
    fn words_break_after_their_spaces() {
        assert_eq!(breaks("one two three"), ["one", "one two"]);
    }

    #[test]
    fn a_run_of_spaces_is_one_opportunity_and_the_line_keeps_none_of_it() {
        let text = "one   two";
        let mut o = Vec::new();
        opportunities(text, &mut o);
        assert_eq!(o.len(), 1);
        assert_eq!(&text[..o[0].trim], "one");
        assert_eq!(&text[o[0].at..], "two");
    }

    #[test]
    fn hyphens_and_slashes_break_after_themselves() {
        assert_eq!(breaks("well-known"), ["well-"]);
        assert_eq!(breaks("a/b"), ["a/"]);
    }

    #[test]
    fn ideographs_break_between_characters() {
        let text = "日本語のテキスト";
        let mut o = Vec::new();
        opportunities(text, &mut o);
        assert!(o.len() >= 6, "CJK barely broke: {o:?}");
    }

    #[test]
    fn a_line_does_not_start_with_closing_punctuation_or_end_with_opening() {
        // The break between 「 and 日 would put the opening bracket alone at
        // the end of a line.
        let mut o = Vec::new();
        opportunities("「日本」", &mut o);
        let text = "「日本」";
        for b in &o {
            let after = text[b.at..].chars().next();
            let before = text[..b.trim].chars().last();
            assert!(!after.is_some_and(no_line_start), "would start a line with {after:?}");
            assert!(!before.is_some_and(no_line_end), "would end a line with {before:?}");
        }
    }

    #[test]
    fn a_newline_is_a_break_that_keeps_nothing_of_itself() {
        let text = "one\ntwo";
        let mut o = Vec::new();
        opportunities(text, &mut o);
        assert_eq!(o.len(), 1);
        assert_eq!(&text[..o[0].trim], "one");
        assert_eq!(&text[o[0].at..], "two");
    }

    #[test]
    fn text_with_nowhere_to_break_offers_nothing() {
        let mut o = Vec::new();
        opportunities("unbreakable", &mut o);
        assert!(o.is_empty(), "{o:?}");
    }
}
