// ──────────────────────────────────────────────────────────────────────
// settings.rs — Configuration, favorites and activity log
//
// Keeps the exact same on-disk format as archman 1.x so existing
// ~/.config/archman/ directories keep working.
// ──────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingDef {
    pub key: &'static str,
    pub default: &'static str,
}

/// Settings in display/save order.
pub const SETTING_DEFS: [SettingDef; 5] = [
    SettingDef { key: "AUR_HELPER", default: "yay" },
    SettingDef { key: "DRY_RUN", default: "false" },
    SettingDef { key: "CONFIRM_ACTIONS", default: "true" },
    SettingDef { key: "LOG_ENABLED", default: "true" },
    SettingDef { key: "PACCACHE_KEEP", default: "3" },
];

pub const SETTING_DOCS: [(&str, &str); 5] = [
    ("AUR_HELPER", "# AUR helper to use (yay / paru)"),
    ("DRY_RUN", "# Dry-run mode — preview changes before executing (true / false)"),
    ("CONFIRM_ACTIONS", "# Confirm before destructive operations (true / false)"),
    ("LOG_ENABLED", "# Enable activity logging (true / false)"),
    ("PACCACHE_KEEP", "# Number of package versions to keep when cleaning cache"),
];

#[derive(Debug, Clone)]
pub struct Settings {
    map: HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut map = HashMap::new();
        for def in SETTING_DEFS {
            map.insert(def.key.to_string(), def.default.to_string());
        }
        Self { map }
    }
}

impl Settings {
    /// Load settings from disk, creating a default config if missing.
    pub fn init() -> Self {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        let path = settings_file();

        if !path.exists() {
            let s = Self::default();
            s.save();
            s
        } else {
            let mut s = Self::default();
            s.load_from(&path);
            s
        }
    }

    fn load_from(&mut self, path: &std::path::Path) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if !key.is_empty() && !value.is_empty() {
                    self.map.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    pub fn get(&self, key: &str) -> &str {
        self.map.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.map.insert(key.to_string(), value.into());
    }

    pub fn is_true(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn toggle(&mut self, key: &str) -> bool {
        let new_val = !self.is_true(key);
        self.set(key, new_val.to_string());
        new_val
    }

    /// Write settings.conf in the same format the bash version produced.
    pub fn save(&self) {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        let mut body = String::new();
        body.push_str("# Arch System Manager Configuration\n");
        body.push_str(&format!("# Generated on {}\n\n", timestamp()));
        for def in SETTING_DEFS {
            let doc = SETTING_DOCS.iter().find(|(k, _)| *k == def.key).map(|(_, d)| *d).unwrap_or("");
            body.push_str(doc);
            body.push('\n');
            body.push_str(&format!("{}={}\n\n", def.key, self.get(def.key)));
        }
        if let Ok(mut f) = fs::File::create(settings_file()) {
            let _ = f.write_all(body.as_bytes());
        }
    }

    /// Reset to defaults (used by Settings → Reset).
    pub fn reset_defaults(&mut self) {
        for def in SETTING_DEFS {
            self.set(def.key, def.default);
        }
    }
}

// ── Paths ───────────────────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
        .join(".config")
        .join("archman")
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.conf")
}

pub fn favorites_file() -> PathBuf {
    config_dir().join("favorites.txt")
}

pub fn log_file() -> PathBuf {
    config_dir().join("archman.log")
}

// ── Activity Log ────────────────────────────────────────────────────

/// Append an entry to archman.log when LOG_ENABLED is true.
pub fn log_action(settings: &Settings, msg: &str) {
    if !settings.is_true("LOG_ENABLED") {
        return;
    }
    let _ = fs::create_dir_all(config_dir());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log_file()) {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

// ── Time Formatting ────────────────────────────────────────────────
// Small civil-time conversion so we don't need chrono for log lines.

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_epoch(secs)
}

/// Format seconds-since-epoch as "%Y-%m-%d %H:%M:%S" (UTC-free local guess:
/// uses the epoch offset directly, matching what `date` reports up to TZ).
fn format_epoch(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formats() {
        assert_eq!(format_epoch(0), "1970-01-01 00:00:00");
        assert_eq!(format_epoch(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn roundtrip_toggle() {
        let mut s = Settings::default();
        assert!(!s.is_true("DRY_RUN"));
        assert!(s.toggle("DRY_RUN"));
        assert!(s.is_true("DRY_RUN"));
    }
}
