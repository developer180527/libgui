//! A fast hasher for libgui's hot per-frame maps.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// FxHash: a multiply-xor hash, used for the per-frame maps keyed by [`Id`]
/// and for the glyph/measurement caches.
///
/// These are hit several times per widget per frame with tiny keys (a `u64`
/// id, a `(font, char, px)` tuple) where SipHash's quality costs more than it
/// buys. Note the *keys* are already well-mixed: an [`Id`] is itself a hash, so
/// a weaker mixing function still spreads them evenly across buckets.
///
/// This is only used where a collision costs a bucket probe. Deriving an `Id`
/// keeps a strong hash, because a collision there would silently make two
/// widgets share retained state.
#[derive(Default)]
pub(crate) struct FxHasher {
    hash: u64,
}

const FX_K: u64 = 0x517c_c1b7_2722_0a95;

impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = (self.hash.rotate_left(5) ^ w).wrapping_mul(FX_K);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub(crate) type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub(crate) type FxSet<K> = HashSet<K, BuildHasherDefault<FxHasher>>;
