use std::hash::{Hash, Hasher};

/// Stable widget identity. Derived from the parent's id plus a label or key,
/// so the same widget gets the same id every frame. This is what links the
/// immediate-mode API to retained per-widget state (animations, drag, focus).
///
/// Deliberately keeps a strong hash while the per-frame maps keyed *by* `Id`
/// use libgui's internal FxHash. A collision here is not a bucket probe: two
/// unrelated widgets would silently share animation, focus and drag state.
/// FxHash measured 1776 collisions over the 960k ids in `libgui_bench --bin
/// collide`, where a 64-bit hash should produce none.
///
/// Ids are **stable across Rust releases, targets and processes**: they use
/// libgui's own [`StableHasher`] (not `std`'s `DefaultHasher`, whose algorithm
/// may change), so they can be persisted (saved layouts) and cross FFI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Id(pub u64);

impl Id {
    pub fn new(src: impl Hash) -> Id {
        Id(0).with(src)
    }

    pub fn with(self, src: impl Hash) -> Id {
        let mut h = StableHasher::new();
        self.0.hash(&mut h);
        src.hash(&mut h);
        Id(h.finish())
    }
}

/// SipHash-1-3 with a zero key: the algorithm `std`'s `DefaultHasher` uses
/// today, frozen here so ids never change under a toolchain upgrade.
/// Integers are always fed little-endian and `usize` as 64 bits, so ids are
/// also identical on 32-bit and big-endian targets.
#[derive(Clone, Debug)]
pub struct StableHasher {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
    tail: u64,
    ntail: usize,
    length: usize,
}

impl StableHasher {
    pub fn new() -> Self {
        let (k0, k1) = (0u64, 0u64);
        Self {
            v0: k0 ^ 0x736f_6d65_7073_6575,
            v1: k1 ^ 0x646f_7261_6e64_6f6d,
            v2: k0 ^ 0x6c79_6765_6e65_7261,
            v3: k1 ^ 0x7465_6462_7974_6573,
            tail: 0,
            ntail: 0,
            length: 0,
        }
    }

    #[inline]
    fn round(&mut self) {
        self.v0 = self.v0.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(13) ^ self.v0;
        self.v0 = self.v0.rotate_left(32);
        self.v2 = self.v2.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(16) ^ self.v2;
        self.v0 = self.v0.wrapping_add(self.v3);
        self.v3 = self.v3.rotate_left(21) ^ self.v0;
        self.v2 = self.v2.wrapping_add(self.v1);
        self.v1 = self.v1.rotate_left(17) ^ self.v2;
        self.v2 = self.v2.rotate_left(32);
    }

    #[inline]
    fn block(&mut self, m: u64) {
        self.v3 ^= m;
        self.round();
        self.v0 ^= m;
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.length += bytes.len();
        let mut i = 0;
        // Fill the pending partial block first.
        if self.ntail != 0 {
            while self.ntail < 8 && i < bytes.len() {
                self.tail |= (bytes[i] as u64) << (8 * self.ntail);
                self.ntail += 1;
                i += 1;
            }
            if self.ntail < 8 {
                return;
            }
            let m = self.tail;
            self.block(m);
            self.tail = 0;
            self.ntail = 0;
        }
        while i + 8 <= bytes.len() {
            let m = u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
            self.block(m);
            i += 8;
        }
        while i < bytes.len() {
            self.tail |= (bytes[i] as u64) << (8 * self.ntail);
            self.ntail += 1;
            i += 1;
        }
    }

    fn write_u16(&mut self, n: u16) {
        self.write(&n.to_le_bytes());
    }
    fn write_u32(&mut self, n: u32) {
        self.write(&n.to_le_bytes());
    }
    fn write_u64(&mut self, n: u64) {
        self.write(&n.to_le_bytes());
    }
    fn write_u128(&mut self, n: u128) {
        self.write(&n.to_le_bytes());
    }
    fn write_usize(&mut self, n: usize) {
        self.write(&(n as u64).to_le_bytes());
    }
    fn write_i16(&mut self, n: i16) {
        self.write_u16(n as u16);
    }
    fn write_i32(&mut self, n: i32) {
        self.write_u32(n as u32);
    }
    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }
    fn write_i128(&mut self, n: i128) {
        self.write_u128(n as u128);
    }
    fn write_isize(&mut self, n: isize) {
        self.write_usize(n as usize);
    }

    fn finish(&self) -> u64 {
        let mut s = self.clone();
        let b = ((s.length as u64 & 0xff) << 56) | s.tail;
        s.block(b);
        s.v2 ^= 0xff;
        for _ in 0..3 {
            s.round();
        }
        s.v0 ^ s.v1 ^ s.v2 ^ s.v3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned values. If this fails, every widget id (and anything persisted
    /// by id) changed: that is a breaking change, not a test to update casually.
    #[test]
    fn ids_are_pinned() {
        let root = Id::new("root");
        assert_eq!(root.0, 0xfb7e_ce09_3bf4_2275);
        assert_eq!(root.with("same").0, 0x38a7_1eaa_3733_6fbd);
        assert_eq!(root.with("same").with(1u32).0, 0x8bd9_b472_4241_6adf);
        assert_eq!(root.with(("button", "OK")).0, 0x6ca8_bdde_cce3_cee8);
        assert_eq!(Id::new(("dock_tab", 7u64, 3u64)).0, 0x16b2_eb81_49de_894b);
    }

    /// SipHash-1-3 reference behaviour: chunking must not matter, and every
    /// input length (partial-block tails) must agree with one-shot hashing.
    #[test]
    fn streaming_matches_one_shot() {
        let data: Vec<u8> = (0..64u8).collect();
        for len in 0..data.len() {
            let mut one = StableHasher::new();
            one.write(&data[..len]);
            let mut split = StableHasher::new();
            for chunk in data[..len].chunks(3) {
                split.write(chunk);
            }
            assert_eq!(one.finish(), split.finish(), "len {len}");
        }
    }

    /// Today the std hasher is the same algorithm; this guards the port. If a
    /// future Rust changes `DefaultHasher`, delete this test: `ids_are_pinned`
    /// is the real guarantee.
    #[test]
    fn matches_std_sip13_on_this_toolchain() {
        use std::collections::hash_map::DefaultHasher;
        for s in ["", "a", "button", "a much longer label that spans blocks"] {
            let mut a = DefaultHasher::new();
            let mut b = StableHasher::new();
            (s, 42u32, 7u64).hash(&mut a);
            (s, 42u32, 7u64).hash(&mut b);
            assert_eq!(a.finish(), b.finish(), "{s:?}");
        }
    }
}
