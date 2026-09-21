//! `Color` as text: `#rrggbb`, `#rrggbbaa`, `#rgb` or `transparent`.
//!
//! Its own module, and not part of `theme_file`, because more than theme files
//! need it: anything deriving serde with a `Color` in it — a `Palette` under the
//! bare `serde` feature, an app's own settings — needs these impls without
//! pulling in TOML. The conversions themselves depend on nothing.

use crate::Color;

impl Color {
    /// `#rrggbb` when opaque, else `#rrggbbaa`.
    pub fn to_hex(self) -> String {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if b(self.a) == 255 {
            format!("#{:02x}{:02x}{:02x}", b(self.r), b(self.g), b(self.b))
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", b(self.r), b(self.g), b(self.b), b(self.a))
        }
    }

    /// Parse `#rgb`, `#rrggbb`, `#rrggbbaa` or `transparent`.
    pub fn parse_hex(s: &str) -> Option<Color> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("transparent") {
            return Some(Color::TRANSPARENT);
        }
        let h = s.strip_prefix('#')?;
        let n = |i: usize, len: usize| u8::from_str_radix(h.get(i..i + len)?, 16).ok();
        let (r, g, b, a) = match h.len() {
            3 => (n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17, 255),
            6 => (n(0, 2)?, n(2, 2)?, n(4, 2)?, 255),
            8 => (n(0, 2)?, n(2, 2)?, n(4, 2)?, n(6, 2)?),
            _ => return None,
        };
        Some(Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid colour `{s}`")))
    }
}
