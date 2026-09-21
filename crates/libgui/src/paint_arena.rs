//! Paint closures for one frame, stored inline in a buffer that outlives none
//! of them.
//!
//! Layout runs after the frame is built, so a widget cannot draw itself as it
//! is declared: it hands over a closure that runs once its rect is known. The
//! obvious home for that closure is a `Box`, and a `Box` per painted widget is
//! an allocation per widget per frame — measured at one for every widget that
//! captures anything, which for a 500-widget panel is 500 trips through a
//! global lock that every other thread in the process shares.
//!
//! So the captures are written straight into one buffer that is reused frame
//! to frame, with a pair of function pointers per entry to call and to drop
//! them. In the steady state it allocates nothing at all, and the widget API
//! is untouched: a custom widget still passes an ordinary closure.
//!
//! # Safety
//!
//! The buffer is bytes, so the invariants are kept by hand:
//!
//! - `Entry::call` and `Entry::drop` are monomorphised for the exact type
//!   written at `Entry::offset`, and only [`PaintArena::write`] creates both
//!   together, so they cannot disagree.
//! - `call` *consumes* the closure (`FnOnce`), so an entry may run once. It is
//!   marked done before anything else can reach it, and [`PaintArena::clear`]
//!   and `Drop` only drop entries that never ran — no double free, and no leak
//!   for a widget whose paint was never reached (clipped away, or a container
//!   that was built and then not painted).
//! - Closures needing an alignment the buffer cannot give are boxed first and
//!   the box is stored inline instead. A `Box` is one word, so that path costs
//!   an allocation only for a closure capturing something over-aligned, which
//!   no built-in widget does.
//! - The buffer may reallocate while filling. Entries hold *offsets*, never
//!   pointers, and moving a `Sized` Rust value bitwise is what `Vec` growth
//!   already does.
//! - Nothing re-enters: closures run during paint, and a `Painter` cannot
//!   reach the `Ui` that owns the arena, so no closure can push while another
//!   is running.

use crate::{Painter, Rect};

/// Alignment the buffer guarantees. `u64` backing gives 8, which covers every
/// capture a widget has: floats, ids, colours, `String`, `Rc`.
const ALIGN: usize = align_of::<u64>();

type CallFn = unsafe fn(*mut u8, &mut Painter, Rect);
type DropFn = unsafe fn(*mut u8);

struct Entry {
    offset: u32,
    call: CallFn,
    drop: DropFn,
    /// The closure has been consumed; its storage is now uninitialised.
    done: bool,
}

#[derive(Default)]
pub(crate) struct PaintArena {
    /// `u64` so the base is 8-aligned; closures live at byte offsets in it.
    buf: Vec<u64>,
    /// Bytes used.
    len: usize,
    entries: Vec<Entry>,
}

/// SAFETY: `p` points at a live, initialised `F` that nothing else will read.
unsafe fn call_inline<F: FnOnce(&mut Painter, Rect)>(p: *mut u8, painter: &mut Painter, r: Rect) {
    let f = unsafe { std::ptr::read(p as *mut F) };
    f(painter, r);
}

/// SAFETY: as `call_inline`.
unsafe fn drop_inline<F>(p: *mut u8) {
    unsafe { std::ptr::drop_in_place(p as *mut F) }
}

/// SAFETY: `p` points at a live, initialised `Box<F>`.
unsafe fn call_boxed<F: FnOnce(&mut Painter, Rect)>(p: *mut u8, painter: &mut Painter, r: Rect) {
    let f = unsafe { std::ptr::read(p as *mut Box<F>) };
    f(painter, r);
}

/// SAFETY: as `call_boxed`.
unsafe fn drop_boxed<F>(p: *mut u8) {
    unsafe { std::ptr::drop_in_place(p as *mut Box<F>) }
}

impl PaintArena {
    /// Store `f` for this frame. The returned handle is what a [`Node`] keeps.
    ///
    /// [`Node`]: crate::layout::Node
    pub fn push<F: FnOnce(&mut Painter, Rect) + 'static>(&mut self, f: F) -> u32 {
        if align_of::<F>() <= ALIGN {
            self.write(f, call_inline::<F>, drop_inline::<F>)
        } else {
            self.write(Box::new(f), call_boxed::<F>, drop_boxed::<F>)
        }
    }

    fn write<T>(&mut self, v: T, call: CallFn, drop: DropFn) -> u32 {
        debug_assert!(align_of::<T>() <= ALIGN);
        let size = size_of::<T>();
        let offset = (self.len + align_of::<T>() - 1) & !(align_of::<T>() - 1);
        let end = offset + size;
        if end > self.buf.len() * ALIGN {
            self.buf.resize(end.div_ceil(ALIGN).next_power_of_two(), 0);
        }
        // SAFETY: the resize above guarantees `offset..end` is inside the
        // buffer, and `offset` is aligned for `T` because the buffer's base is
        // `ALIGN`-aligned and `align_of::<T>() <= ALIGN`. Nothing has been
        // written there this frame: `len` only moves forward until `clear`.
        unsafe {
            let base = self.buf.as_mut_ptr() as *mut u8;
            std::ptr::write(base.add(offset) as *mut T, v);
        }
        self.len = end;
        let i = self.entries.len() as u32;
        self.entries.push(Entry { offset: offset as u32, call, drop, done: false });
        i
    }

    /// Run the closure `i`, once. A second call is a no-op.
    pub fn run(&mut self, i: u32, painter: &mut Painter, rect: Rect) {
        let Some(e) = self.entries.get_mut(i as usize) else { return };
        if e.done {
            return;
        }
        // Marked before the call, so a panic inside the closure cannot leave
        // an entry that `clear` would drop a second time.
        e.done = true;
        let (offset, call) = (e.offset, e.call);
        // SAFETY: `offset` was written with a `T` that `call` is monomorphised
        // for, and `done` guarantees nothing else reads it.
        unsafe {
            let base = self.buf.as_mut_ptr() as *mut u8;
            call(base.add(offset as usize), painter, rect);
        }
    }

    /// Drop everything that never ran and start the next frame. Keeps the
    /// buffer, so a steady frame allocates nothing.
    pub fn clear(&mut self) {
        let base = self.buf.as_mut_ptr() as *mut u8;
        for e in &self.entries {
            if !e.done {
                // SAFETY: never run, so still an initialised value of the type
                // `drop` was monomorphised for.
                unsafe { (e.drop)(base.add(e.offset as usize)) };
            }
        }
        self.entries.clear();
        self.len = 0;
    }

    /// Make room for `widgets` closures of a typical size, so a first frame
    /// costs no more than a steady one.
    pub fn reserve(&mut self, widgets: usize) {
        // 48 bytes is about what a built-in widget's paint closure captures:
        // a style, a couple of floats and a colour.
        self.buf.resize(self.buf.len().max(widgets * 48 / ALIGN), 0);
        self.entries.reserve(widgets.saturating_sub(self.entries.capacity()));
    }

    /// Bytes the buffer holds, for tests.
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.buf.len() * ALIGN
    }
}

impl Drop for PaintArena {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::cell::Cell;

    /// Counts its own drops, so a leak and a double free are both visible.
    struct Bomb(Rc<Cell<i32>>);

    impl Drop for Bomb {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn arena_with(n: usize, drops: &Rc<Cell<i32>>) -> PaintArena {
        let mut a = PaintArena::default();
        for _ in 0..n {
            let b = Bomb(drops.clone());
            a.push(move |_: &mut Painter, _: Rect| {
                let _ = &b;
            });
        }
        a
    }

    #[test]
    fn a_closure_that_never_runs_is_dropped_exactly_once() {
        let drops = Rc::new(Cell::new(0));
        let mut a = arena_with(3, &drops);
        a.clear();
        assert_eq!(drops.get(), 3);
        a.clear();
        assert_eq!(drops.get(), 3, "cleared twice, dropped twice");
        drop(a);
        assert_eq!(drops.get(), 3);
    }

    #[test]
    fn dropping_the_arena_mid_frame_drops_what_is_left() {
        let drops = Rc::new(Cell::new(0));
        drop(arena_with(4, &drops));
        assert_eq!(drops.get(), 4);
    }

    #[test]
    fn alignment_and_offsets_survive_a_reallocation() {
        // Values wider than the initial buffer, so it grows several times, and
        // every closure still sees exactly what it captured.
        let mut a = PaintArena::default();
        let seen = Rc::new(Cell::new(0u64));
        let mut ids = Vec::new();
        for i in 0..200u64 {
            let big = [i; 9];
            let seen = seen.clone();
            ids.push(a.push(move |_: &mut Painter, _: Rect| {
                assert!(big.iter().all(|&v| v == big[0]));
                seen.set(seen.get() + big[0]);
            }));
        }
        assert!(a.capacity() >= 200 * 9 * 8);
        // Run them out of order: entries are independent.
        with_painter(|p| {
            for i in ids.iter().rev() {
                a.run(*i, p, Rect::default());
            }
        });
        assert_eq!(seen.get(), (0..200u64).sum::<u64>());
    }

    #[test]
    fn an_over_aligned_capture_takes_the_boxed_path() {
        #[repr(align(64))]
        struct Wide([u8; 64]);
        let drops = Rc::new(Cell::new(0));
        let mut a = PaintArena::default();
        let b = Bomb(drops.clone());
        let w = Wide([7; 64]);
        a.push(move |_: &mut Painter, _: Rect| {
            let _ = &b;
            assert_eq!(w.0[0], 7);
        });
        a.clear();
        assert_eq!(drops.get(), 1, "the boxed path leaked or double-dropped");
    }

    /// Boxing keeps the alignment promise on the way *out* as well as in: the
    /// closure is read back through a `*mut Box<F>` and called, and `F` here
    /// may not sit at an address that is not a multiple of 64.
    #[test]
    fn an_over_aligned_capture_is_aligned_when_it_runs() {
        #[repr(align(64))]
        struct Wide([u64; 8]);
        let mut a = PaintArena::default();
        let ran = Rc::new(Cell::new(0));
        let mut ids = Vec::new();
        for i in 0..8u64 {
            let w = Wide([i; 8]);
            let ran = ran.clone();
            // A second, ordinary closure between them, so the two storage
            // paths interleave in one buffer.
            ids.push(a.push(move |_: &mut Painter, _: Rect| {
                assert_eq!(&w as *const Wide as usize % 64, 0, "the boxed capture was not 64-aligned");
                assert!(w.0.iter().all(|&v| v == w.0[0]), "the capture was torn");
                ran.set(ran.get() + 1);
            }));
            let small = i;
            ids.push(a.push(move |_: &mut Painter, _: Rect| {
                let _ = small;
            }));
        }
        // Boxed, not inlined: sixteen entries, eight of them 64 bytes wide,
        // cannot fit in a buffer this size if they were stored in place.
        assert!(a.capacity() < 8 * 64, "the over-aligned captures were stored inline: {}", a.capacity());
        with_painter(|p| {
            for id in &ids {
                a.run(*id, p, Rect::default());
            }
        });
        assert_eq!(ran.get(), 8);
    }

    /// The drop path goes through `drop_boxed`, which drops an `F` that may do
    /// anything a `Drop` may do — including allocate, which is the case the
    /// buffer's own bookkeeping could be caught out by.
    #[test]
    fn a_drop_that_allocates_runs_exactly_once() {
        #[repr(align(64))]
        struct Loud {
            log: Rc<std::cell::RefCell<Vec<String>>>,
            name: usize,
            _pad: [u64; 8],
        }
        impl Drop for Loud {
            fn drop(&mut self) {
                // Allocates twice: the string, and the vector's growth.
                self.log.borrow_mut().push(format!("dropped {}", self.name));
            }
        }
        let log: Rc<std::cell::RefCell<Vec<String>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut a = PaintArena::default();
        let mut ids = Vec::new();
        for name in 0..6 {
            let l = Loud { log: log.clone(), name, _pad: [0; 8] };
            ids.push(a.push(move |_: &mut Painter, _: Rect| {
                let _ = &l;
            }));
        }
        // Run half of them; the rest are dropped by `clear`.
        with_painter(|p| {
            for id in ids.iter().take(3) {
                a.run(*id, p, Rect::default());
            }
        });
        assert_eq!(log.borrow().len(), 3, "running a closure did not drop its capture");
        a.clear();
        assert_eq!(log.borrow().len(), 6, "clear dropped the wrong number: {:?}", log.borrow());
        drop(a);
        assert_eq!(log.borrow().len(), 6, "something was dropped twice");
        let mut names: Vec<String> = log.borrow().clone();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 6, "a capture was dropped twice: {names:?}");
    }

    /// A real `Painter` over an empty draw list and no fonts: the closures
    /// under test do not draw, but a fake reference would be undefined
    /// behaviour whether or not anything read it.
    fn with_painter(f: impl FnOnce(&mut Painter)) {
        let mut draw = crate::DrawList::default();
        let mut fonts = crate::Fonts::new();
        let theme = crate::Theme::dark();
        let mut p =
            Painter { draw: &mut draw, fonts: &mut fonts, theme: &theme, font: crate::FontId(0), strs: &[], scale: 1.0 };
        f(&mut p);
    }
}
