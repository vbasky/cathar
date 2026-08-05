//! Lightweight session prefs: recent files + player defaults.
//!
//! Stored as JSON under the OS app-data directory (no eframe persistence feature).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme::Appearance;

/// Max paths shown under File → Open Recent.
pub(crate) const MAX_RECENT: usize = 12;

/// Durable player/editor preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppPrefs {
    /// Most-recent first; existing files only when loaded into the menu.
    pub recent: Vec<PathBuf>,
    pub volume: f32,
    pub muted: bool,
    pub loop_file: bool,
    pub playlist_auto_advance: bool,
    /// When auto-advance hits the end of the queue, wrap to the first track.
    pub playlist_wrap: bool,
    pub shuffle: bool,
    /// When true, open the most recent existing file on launch (paused).
    pub open_last_on_launch: bool,
    pub appearance: PrefsAppearance,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            volume: 1.0,
            muted: false,
            loop_file: false,
            playlist_auto_advance: true,
            playlist_wrap: true,
            shuffle: false,
            open_last_on_launch: true,
            appearance: PrefsAppearance::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PrefsAppearance {
    #[default]
    System,
    Dark,
    Light,
}

impl From<Appearance> for PrefsAppearance {
    fn from(a: Appearance) -> Self {
        match a {
            Appearance::System => Self::System,
            Appearance::Dark => Self::Dark,
            Appearance::Light => Self::Light,
        }
    }
}

impl From<PrefsAppearance> for Appearance {
    fn from(a: PrefsAppearance) -> Self {
        match a {
            PrefsAppearance::System => Self::System,
            PrefsAppearance::Dark => Self::Dark,
            PrefsAppearance::Light => Self::Light,
        }
    }
}

impl AppPrefs {
    pub(crate) fn load() -> Self {
        let path = prefs_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut p) => {
                p.sanitize();
                p
            }
            Err(_) => Self::default(),
        }
    }

    pub(crate) fn save(&self) {
        let path = prefs_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, bytes);
        }
    }

    fn sanitize(&mut self) {
        self.volume = self.volume.clamp(0.0, 1.5);
        // Drop missing paths; keep order (most recent first).
        self.recent.retain(|p| p.is_file());
        self.recent.truncate(MAX_RECENT);
        // Dedupe while preserving order.
        let mut seen = std::collections::HashSet::new();
        self.recent.retain(|p| seen.insert(p.clone()));
    }

    /// Push a successfully opened path to the front of the recent list.
    pub(crate) fn push_recent(&mut self, path: PathBuf) {
        if !path.is_file() {
            return;
        }
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
        self.save();
    }

    pub(crate) fn clear_recent(&mut self) {
        self.recent.clear();
        self.save();
    }

    /// Display labels for the Open Recent menu (filename, path still exists).
    pub(crate) fn recent_menu_entries(&self) -> Vec<(String, PathBuf, bool)> {
        self.recent
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string());
                let exists = p.is_file();
                (name, p.clone(), exists)
            })
            .collect()
    }
}

fn prefs_path() -> PathBuf {
    data_dir().join("prefs.json")
}

/// Platform app-data directory for Cathar.
pub(crate) fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home_dir()
            .map(|h| h.join("Library/Application Support/Cathar"))
            .unwrap_or_else(|| PathBuf::from(".").join("Cathar"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(home_dir)
            .map(|h| h.join("Cathar"))
            .unwrap_or_else(|| PathBuf::from(".").join("Cathar"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".config")))
            .map(|h| h.join("cathar"))
            .unwrap_or_else(|| PathBuf::from(".").join("cathar"))
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// True if `path` looks like a local audio file we can open.
pub(crate) fn is_audio_path(path: &Path) -> bool {
    const EXTS: &[&str] =
        &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif", "aac", "opus", "caf", "wma"];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// True if `path` is an M3U / M3U8 playlist.
pub(crate) fn is_playlist_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_last_on_launch_defaults_on() {
        // PRODUCT Behavior 2: new installs / missing field → on.
        assert!(AppPrefs::default().open_last_on_launch);
        let partial = r#"{"recent":[],"volume":1.0}"#;
        let p: AppPrefs = serde_json::from_str(partial).expect("partial prefs");
        assert!(p.open_last_on_launch);
    }
}
