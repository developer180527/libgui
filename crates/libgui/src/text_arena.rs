//! Strings a widget hands to its paint closure, for the length of one frame.
//!
//! A widget is declared before its rect is known, so the text it will draw has
//! to outlive the call that declared it — which meant a `String` per text
//! widget per frame, and after the paint closures moved into their own arena
//! it was the only allocation libgui still made per widget.
//!
//! A [`FrameText`] is eight bytes naming a range in one buffer that is reused
//! frame to frame, so the copy is a `memcpy` into space that already exists.
//!
//! Handles are only valid for the frame that made them, exactly like the paint
//! closure that carries one: both are cleared at the start of the next frame.
//! A stale handle cannot read another frame's text or go out of bounds — it
//! resolves to `""` — but it will not read what it used to, so do not keep one.
//!
//! Custom widgets do not have to use this at all: [`Painter::text`] and friends
//! take anything that implements [`PaintText`], which `&str` and `String` do,
//! so an ordinary owned `String` in a closure keeps working.
//!
//! [`Painter::text`]: crate::Painter::text

/// Text stored in the frame's arena. Copy, eight bytes, valid for this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FrameText {
    start: u32,
    len: u32,
}

impl FrameText {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Anything [`Painter::text`](crate::Painter::text) can draw: a borrowed or
/// owned string, or a [`FrameText`] handle that costs nothing to carry.
pub trait PaintText {
    /// Resolve against the frame's text arena. Called once, at paint time.
    fn get<'a>(&'a self, arena: &'a [u8]) -> &'a str;
}

impl PaintText for FrameText {
    fn get<'a>(&'a self, arena: &'a [u8]) -> &'a str {
        let (start, end) = (self.start as usize, self.start as usize + self.len as usize);
        // A handle from a previous frame points past the end of a cleared
        // arena, or at bytes that are no longer a character boundary. Neither
        // is unsafe, and neither is worth a panic in a paint closure.
        arena.get(start..end).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("")
    }
}

impl PaintText for str {
    fn get<'a>(&'a self, _: &'a [u8]) -> &'a str {
        self
    }
}

impl PaintText for String {
    fn get<'a>(&'a self, _: &'a [u8]) -> &'a str {
        self
    }
}

/// So `&str`, `&String` and `&FrameText` all work without the caller thinking
/// about it.
impl<T: PaintText + ?Sized> PaintText for &T {
    fn get<'a>(&'a self, arena: &'a [u8]) -> &'a str {
        (**self).get(arena)
    }
}

/// One frame's text, in one buffer.
#[derive(Default)]
pub(crate) struct TextArena {
    buf: Vec<u8>,
}

impl TextArena {
    pub fn push(&mut self, s: &str) -> FrameText {
        let start = self.buf.len() as u32;
        self.buf.extend_from_slice(s.as_bytes());
        FrameText { start, len: s.len() as u32 }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn reserve(&mut self, bytes: usize) {
        self.buf.reserve(bytes.saturating_sub(self.buf.capacity()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_resolve_to_what_was_put_in() {
        let mut a = TextArena::default();
        let one = a.push("Inspector");
        let two = a.push("");
        let three = a.push("Übergrößen — ✓");
        assert_eq!(one.get(a.bytes()), "Inspector");
        assert_eq!(two.get(a.bytes()), "");
        assert_eq!(three.get(a.bytes()), "Übergrößen — ✓");
    }

    #[test]
    fn a_handle_from_a_cleared_frame_resolves_to_nothing() {
        let mut a = TextArena::default();
        let stale = a.push("gone");
        a.clear();
        assert_eq!(stale.get(a.bytes()), "", "a stale handle read the next frame's bytes");
        // And one that now lands mid-character does not panic either.
        a.push("é");
        assert_eq!(FrameText { start: 1, len: 1 }.get(a.bytes()), "");
    }

    #[test]
    fn borrowed_and_owned_strings_still_work() {
        let arena: &[u8] = b"unused";
        assert_eq!(PaintText::get(&"literal", arena), "literal");
        assert_eq!(PaintText::get(&String::from("owned"), arena), "owned");
    }
}
