//! TOML themes with hot reload.
//!
//! ```toml
//! name = "Unity-ish"
//! extends = "dark"          # dark | midnight | light
//! density = "compact"       # compact | regular | touch
//!
//! [palette]                 # changes here flow into every derived style
//! accent = "#3a79d8"
//!
//! [metrics]
//! radius = 2
//!
//! [button]                  # then override individual widget styles
//! radius = 2
//! fill = { normal = "surface", hover = "mix(surface, accent, 0.15)", active = "surface_active" }
//! shadow = { color = "transparent", offset_y = 0, blur = 0 }
//! ```
//!
//! Colour values: `"#rgb"`, `"#rrggbb"`, `"#rrggbbaa"`, `"transparent"`, a palette
//! name (`"accent"`), a palette name with alpha (`"accent@0.3"`), or
//! `"mix(a, b, t)"` of any of these. Only the keys you write are changed; unknown
//! keys are errors so typos don't silently do nothing.

use crate::{Color, Density, Metrics, Palette, Theme};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeError(pub String);

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ThemeError {}

fn err(msg: impl Into<String>) -> ThemeError {
    ThemeError(msg.into())
}

// ---- Color <-> string ---------------------------------------------------------

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

impl serde::Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid colour `{s}`")))
    }
}

/// Resolve a colour expression against a palette.
fn resolve(expr: &str, palette: &toml::Table) -> Result<Color, String> {
    let e = expr.trim();
    if let Some(c) = Color::parse_hex(e) {
        return Ok(c);
    }
    // `<any colour>@alpha`
    if let Some((left, a)) = e.rsplit_once('@') {
        if let Ok(alpha) = a.trim().parse::<f32>() {
            return Ok(resolve(left, palette)?.with_alpha(alpha));
        }
        return Err(format!("`{e}`: bad alpha"));
    }
    if let Some(args) = e.strip_prefix("mix(").and_then(|r| r.strip_suffix(')')) {
        // Split on top-level commas (nested mix() allowed).
        let (mut depth, mut parts, mut start) = (0, Vec::new(), 0);
        for (i, ch) in args.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&args[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(&args[start..]);
        if parts.len() != 3 {
            return Err(format!("`{e}`: mix() takes (colour, colour, amount)"));
        }
        let t: f32 = parts[2].trim().parse().map_err(|_| format!("`{e}`: bad mix amount"))?;
        return Ok(resolve(parts[0], palette)?.lerp(resolve(parts[1], palette)?, t));
    }
    palette
        .get(e)
        .and_then(|v| v.as_str())
        .and_then(Color::parse_hex)
        .ok_or_else(|| format!("unknown colour `{e}` (use #hex, transparent, a palette name, name@alpha, or mix(a, b, t))"))
}

/// Deep-merge `patch` into `base`, resolving colour expressions in strings.
fn merge(base: &mut toml::Value, patch: &toml::Value, palette: &toml::Table, path: &str) -> Result<(), String> {
    match (base, patch) {
        (toml::Value::Table(b), toml::Value::Table(p)) => {
            for (k, v) in p {
                let sub = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                match b.get_mut(k) {
                    Some(slot) => merge(slot, v, palette, &sub)?,
                    None => return Err(format!("unknown key `{sub}`")),
                }
            }
            Ok(())
        }
        (slot @ toml::Value::String(_), toml::Value::String(s)) => {
            *slot = toml::Value::String(resolve(s, palette).map_err(|e| format!("{path}: {e}"))?.to_hex());
            Ok(())
        }
        (slot @ toml::Value::Float(_), toml::Value::Integer(i)) => {
            *slot = toml::Value::Float(*i as f64);
            Ok(())
        }
        (slot, v) if std::mem::discriminant(slot) == std::mem::discriminant(v) => {
            *slot = v.clone();
            Ok(())
        }
        (slot, v) => Err(format!("{path}: expected {}, found {}", slot.type_str(), v.type_str())),
    }
}

fn to_value<T: serde::Serialize>(v: &T) -> toml::Value {
    toml::Value::try_from(v).expect("theme types serialize")
}

fn from_value<T: serde::de::DeserializeOwned>(v: toml::Value, what: &str) -> Result<T, ThemeError> {
    v.try_into().map_err(|e: toml::de::Error| err(format!("{what}: {}", e.message())))
}

impl Theme {
    /// Parse a theme file. See the module docs for the format.
    pub fn from_toml(src: &str) -> Result<Theme, ThemeError> {
        Self::from_toml_with(src, None)
    }

    /// Like [`Theme::from_toml`], forcing a density (the file's explicit
    /// `[metrics]` and style values still win).
    pub fn from_toml_with(src: &str, density: Option<Density>) -> Result<Theme, ThemeError> {
        let file: toml::Table = src.parse().map_err(|e: toml::de::Error| err(e.to_string()))?;
        let extends = file.get("extends").and_then(|v| v.as_str()).unwrap_or("dark");
        let mut theme = Theme::preset(extends).ok_or_else(|| err(format!("extends: unknown preset `{extends}` (dark, midnight, light)")))?;

        let density = match (density, file.get("density")) {
            (Some(d), _) => d,
            (None, Some(v)) => from_value(v.clone(), "density")?,
            (None, None) => theme.density,
        };
        theme.density = density;
        let mut metrics = to_value(&Metrics::for_density(density));

        // 1. Palette (may reference the base palette's names).
        let mut palette = to_value(&theme.palette);
        if let Some(p) = file.get("palette") {
            let base = palette.as_table().unwrap().clone();
            merge(&mut palette, p, &base, "palette").map_err(err)?;
        }
        let palette_table = palette.as_table().unwrap().clone();
        theme.palette = from_value::<Palette>(palette, "palette")?;

        // 2. Metrics, then derive every style from palette + metrics.
        if let Some(m) = file.get("metrics") {
            merge(&mut metrics, m, &palette_table, "metrics").map_err(err)?;
        }
        theme.metrics = from_value(metrics, "metrics")?;
        theme.rederive();

        // 3. Per-widget style overrides.
        let mut full = to_value(&theme);
        for (key, value) in &file {
            match key.as_str() {
                "name" | "extends" | "density" | "palette" | "metrics" => {}
                _ => {
                    let slot = full.as_table_mut().unwrap().get_mut(key).ok_or_else(|| {
                        err(format!(
                            "unknown section [{key}] (styles: button, button_primary, toggle, slider, selectable, \
                             text_input, segmented, scrollbar, tab, splitter, panel, plot, viewport, drop_preview)"
                        ))
                    })?;
                    merge(slot, value, &palette_table, key).map_err(err)?;
                }
            }
        }
        let mut theme: Theme = from_value(full, "theme")?;
        if let Some(name) = file.get("name").and_then(|v| v.as_str()) {
            theme.name = name.to_string();
        }
        Ok(theme)
    }

    /// Every resolved value, as a complete TOML file (a reference to copy from).
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("theme serializes")
    }
}

/// Polls a theme file and reloads it when it changes. No platform file-watching
/// dependency: checks the file's modification time at most every `interval`.
pub struct ThemeWatcher {
    path: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    last_check: Option<Instant>,
    missing_reported: bool,
    pub interval: Duration,
    pub density: Option<Density>,
}

impl ThemeWatcher {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            stamp: None,
            last_check: None,
            missing_reported: false,
            interval: Duration::from_millis(200),
            density: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Force the next `poll` to reload (e.g. after changing `density`).
    pub fn invalidate(&mut self) {
        self.stamp = None;
        self.last_check = None;
        self.missing_reported = false;
    }

    /// `Some` when the file changed (or on first call): the new theme or why it failed.
    pub fn poll(&mut self) -> Option<Result<Theme, ThemeError>> {
        let now = Instant::now();
        if self.last_check.is_some_and(|t| now - t < self.interval) {
            return None;
        }
        self.last_check = Some(now);
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) => {
                // Report a missing/unreadable file once, not every poll.
                self.stamp = None;
                if std::mem::replace(&mut self.missing_reported, true) {
                    return None;
                }
                return Some(Err(err(format!("{}: {e}", self.path.display()))));
            }
        };
        self.missing_reported = false;
        let stamp = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
        if self.stamp == Some(stamp) {
            return None;
        }
        self.stamp = Some(stamp);
        Some(
            std::fs::read_to_string(&self.path)
                .map_err(|e| err(format!("{}: {e}", self.path.display())))
                .and_then(|src| Theme::from_toml_with(&src, self.density))
                .map_err(|e| err(format!("{}: {e}", self.path.display()))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_change_flows_into_derived_styles() {
        let t = Theme::from_toml("[palette]\naccent = \"#ff0000\"").unwrap();
        assert_eq!(t.palette.accent, Color::hex(0xff0000));
        assert_eq!(t.button_primary.fill.normal, Color::hex(0xff0000));
        assert_eq!(t.slider.fill, Color::hex(0xff0000));
        assert_eq!(t.tab.accent, Color::hex(0xff0000));
    }

    #[test]
    fn style_overrides_and_colour_expressions() {
        let src = r##"
            name = "Test"
            extends = "light"
            density = "compact"
            [button]
            radius = 0
            fill = { normal = "accent@0.5", hover = "mix(#000000, #ffffff, 0.5)" }
            shadow = { color = "transparent" }
        "##;
        let t = Theme::from_toml(src).unwrap();
        assert_eq!(t.name, "Test");
        assert_eq!(t.density, Density::Compact);
        assert_eq!(t.metrics.control_height, 24.0);
        assert_eq!(t.button.radius, 0.0);
        assert_eq!(t.button.fill.normal.to_hex(), Palette::light().accent.with_alpha(0.5).to_hex());
        assert!((t.button.fill.hover.r - 0.5).abs() < 0.01);
        assert_eq!(t.button.shadow.color, Color::TRANSPARENT);
        // Untouched fields keep their derived values.
        assert_eq!(t.button.fill.active, Palette::light().surface_active);
        assert_eq!(t.button.height, 24.0);
    }

    #[test]
    fn errors_are_specific() {
        let e = Theme::from_toml("[buton]\nradius = 1").unwrap_err().0;
        assert!(e.contains("unknown section [buton]"), "{e}");
        let e = Theme::from_toml("[button]\nradus = 1").unwrap_err().0;
        assert!(e.contains("unknown key `button.radus`"), "{e}");
        let e = Theme::from_toml("[palette]\naccent = \"acent\"").unwrap_err().0;
        assert!(e.contains("unknown colour `acent`"), "{e}");
        let e = Theme::from_toml("[button]\nradius = \"big\"").unwrap_err().0;
        assert!(e.contains("button.radius"), "{e}");
        assert!(Theme::from_toml("[button\n").is_err());
    }

    #[test]
    fn watcher_reloads_on_change_and_reports_errors_once() {
        let dir = std::env::temp_dir().join(format!("libgui-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.toml");
        let mut w = ThemeWatcher::new(&path);
        w.interval = Duration::ZERO;
        assert!(w.poll().unwrap().is_err(), "missing file reported");
        assert!(w.poll().is_none(), "…only once");
        std::fs::write(&path, "[metrics]\nradius = 1").unwrap();
        assert_eq!(w.poll().unwrap().unwrap().metrics.radius, 1.0);
        assert!(w.poll().is_none(), "unchanged file is not reloaded");
        std::fs::write(&path, "[metrics]\nradius = 12.5").unwrap();
        assert_eq!(w.poll().unwrap().unwrap().metrics.radius, 12.5);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn shipped_theme_files_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "toml") && !path.ends_with("_exported.toml") {
                let src = std::fs::read_to_string(&path).unwrap();
                Theme::from_toml(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                n += 1;
            }
        }
        assert!(n >= 3);
        let t = Theme::from_toml("[tab]\nfill_hover = \"mix(#000000, #ffffff, 0.5)@0.25\"").unwrap();
        assert!((t.tab.fill_hover.a - 0.25).abs() < 0.01 && (t.tab.fill_hover.r - 0.5).abs() < 0.01);
    }

    #[test]
    fn export_round_trips() {
        let mut t = Theme::midnight();
        t.button.radius = 3.0;
        let back = Theme::from_toml(&format!("extends = \"midnight\"\n{}", t.to_toml())).unwrap();
        // Colours are stored as 8-bit hex, so compare the serialized form.
        assert_eq!(back.to_toml(), t.to_toml());
        assert_eq!(back.button.radius, 3.0);
    }
}
