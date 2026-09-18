//! Hot reload for TOML themes: polls a file's modification time. This is the
//! only part of libgui that touches the filesystem, so it lives behind the
//! `theme-watch` feature; the core (and `theme-toml` parsing) never does I/O.
//! Hosts without a filesystem (consoles, WASM, sandboxes) simply don't enable it
//! and call `Theme::from_toml` with text they obtained themselves.

use crate::theme_file::err;
use crate::{Density, Theme, ThemeError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

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
}
