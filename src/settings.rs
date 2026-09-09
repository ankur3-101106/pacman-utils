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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingDef {
    pub key: &'static str,
    pub default: &'static str,
}

/// Settings in display/save order.
pub const SETTING_DEFS: [SettingDef; 6] = [
    SettingDef {
        key: "AUR_HELPER",
        default: "yay",
    },
    SettingDef {
        key: "DRY_RUN",
        default: "false",
    },
    SettingDef {
        key: "CONFIRM_ACTIONS",
        default: "true",
    },
    SettingDef {
        key: "NATIVE_CONFIRM",
        default: "true",
    },
    SettingDef {
        key: "LOG_ENABLED",
        default: "true",
    },
    SettingDef {
        key: "PACCACHE_KEEP",
        default: "3",
    },
];

pub const SETTING_DOCS: [(&str, &str); 6] = [
    ("AUR_HELPER", "# AUR helper to use (yay / paru)"),
    (
        "DRY_RUN",
        "# Dry-run mode — preview changes before executing (true / false)",
    ),
    (
        "CONFIRM_ACTIONS",
        "# Confirm before destructive operations (true / false)",
    ),
    (
        "NATIVE_CONFIRM",
        "# Native package manager confirmation prompts (true / false)",
    ),
    ("LOG_ENABLED", "# Enable activity logging (true / false)"),
    (
        "PACCACHE_KEEP",
        "# Number of package versions to keep when cleaning cache",
    ),
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

    pub fn parse_str(&mut self, content: &str) {
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
        sync_log_enabled(self);
    }

    pub fn load_from(&mut self, path: &std::path::Path) {
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        self.parse_str(&content);
    }

    pub fn get(&self, key: &str) -> &str {
        self.map.get(key).map(String::as_str).unwrap_or("")
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let val = value.into();
        if key == "LOG_ENABLED" {
            set_log_enabled_flag(val == "true");
        }
        self.map.insert(key.to_string(), val);
    }

    pub fn is_true(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn toggle(&mut self, key: &str) -> bool {
        let new_val = !self.is_true(key);
        self.set(key, new_val.to_string());
        new_val
    }

    /// Serialize current configuration to the INI-style file format.
    pub fn serialize(&self) -> String {
        let mut body = String::new();
        body.push_str("# Arch System Manager Configuration\n");
        body.push_str(&format!("# Generated on {}\n\n", timestamp()));
        for def in SETTING_DEFS {
            let doc = SETTING_DOCS
                .iter()
                .find(|(k, _)| *k == def.key)
                .map(|(_, d)| *d)
                .unwrap_or("");
            body.push_str(doc);
            body.push('\n');
            body.push_str(&format!("{}={}\n\n", def.key, self.get(def.key)));
        }
        body
    }

    /// Save configuration to a specific path.
    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, self.serialize())
    }

    /// Write settings.conf in the same format the bash version produced.
    pub fn save(&self) {
        let _ = self.save_to(&settings_file());
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

static LOG_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn is_log_enabled() -> bool {
    LOG_ENABLED.load(Ordering::Relaxed)
}

pub fn set_log_enabled_flag(enabled: bool) {
    LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn sync_log_enabled(settings: &Settings) {
    set_log_enabled_flag(settings.is_true("LOG_ENABLED"));
}

/// Append an entry to archman.log when LOG_ENABLED is true.
pub fn log_if_enabled(msg: &str) {
    if !is_log_enabled() {
        return;
    }
    let _ = fs::create_dir_all(config_dir());
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
    {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

/// Append an entry to archman.log when LOG_ENABLED is true.
pub fn log_action(settings: &Settings, msg: &str) {
    sync_log_enabled(settings);
    log_if_enabled(msg);
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

    #[test]
    fn native_confirm_default_and_toggle() {
        let mut s = Settings::default();
        assert!(s.is_true("NATIVE_CONFIRM"));
        assert!(!s.toggle("NATIVE_CONFIRM"));
        assert!(!s.is_true("NATIVE_CONFIRM"));
    }

    #[test]
    fn config_missing_file_leaves_defaults() {
        let mut s = Settings::default();
        let missing = PathBuf::from("/nonexistent/archman/path/settings.conf");
        s.load_from(&missing);
        assert_eq!(s.get("AUR_HELPER"), "yay");
        assert!(s.is_true("CONFIRM_ACTIONS"));
        assert!(s.is_true("NATIVE_CONFIRM"));
        assert!(!s.is_true("DRY_RUN"));
    }

    #[test]
    fn config_malformed_lines_gracefully_ignored() {
        let mut s = Settings::default();
        let malformed = "
# Comment line
INVALID_LINE_WITHOUT_EQUALS
AUR_HELPER = paru
=MISSING_KEY
EMPTY_VALUE=
   SPACED_KEY   =   custom_val   
";
        s.parse_str(malformed);
        assert_eq!(s.get("AUR_HELPER"), "paru");
        assert_eq!(s.get("SPACED_KEY"), "custom_val");
        // Defaults remain intact for unspecified settings
        assert!(s.is_true("NATIVE_CONFIRM"));
        assert!(s.is_true("CONFIRM_ACTIONS"));
    }

    #[test]
    fn config_legacy_backward_compatibility() {
        // Legacy config file written before NATIVE_CONFIRM was introduced
        let legacy_conf = "
AUR_HELPER=paru
CONFIRM_ACTIONS=false
DRY_RUN=false
LOG_ENABLED=true
";
        let mut s = Settings::default();
        s.parse_str(legacy_conf);

        assert_eq!(s.get("AUR_HELPER"), "paru");
        assert!(!s.is_true("CONFIRM_ACTIONS"));
        // NATIVE_CONFIRM must default to true even when missing in legacy config
        assert!(s.is_true("NATIVE_CONFIRM"));
    }

    #[test]
    fn config_save_and_load_roundtrip() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp_file = std::env::temp_dir().join(format!(
            "archman_test_settings_{}_{}.conf",
            std::process::id(),
            id
        ));

        let mut s1 = Settings::default();
        s1.set("AUR_HELPER", "paru");
        s1.set("CONFIRM_ACTIONS", "false");
        s1.set("NATIVE_CONFIRM", "false");
        s1.set("DRY_RUN", "true");

        s1.save_to(&tmp_file).expect("save_to must succeed");

        let mut s2 = Settings::default();
        s2.load_from(&tmp_file);

        assert_eq!(s2.get("AUR_HELPER"), "paru");
        assert!(!s2.is_true("CONFIRM_ACTIONS"));
        assert!(!s2.is_true("NATIVE_CONFIRM"));
        assert!(s2.is_true("DRY_RUN"));

        let _ = fs::remove_file(&tmp_file);
    }

    #[test]
    fn test_config_unknown_keys_parsed() {
        let mut s = Settings::default();
        s.parse_str("UNKNOWN_CUSTOM_KEY=custom_value\nANOTHER_KEY=123\n");
        assert_eq!(s.get("UNKNOWN_CUSTOM_KEY"), "custom_value");
        assert_eq!(s.get("ANOTHER_KEY"), "123");
        // Predefined defaults are preserved
        assert_eq!(s.get("AUR_HELPER"), "yay");
    }

    #[test]
    fn test_config_failed_save_handled() {
        let s = Settings::default();
        // Path in non-existent directory without permissions
        let invalid_path = PathBuf::from("/dev/null/forbidden/cannot_write.conf");
        let res = s.save_to(&invalid_path);
        assert!(res.is_err(), "Writing to an invalid path must return Err");
    }

    #[test]
    fn test_config_non_utf8_file_handled() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(100);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp_file = std::env::temp_dir().join(format!(
            "archman_test_corrupt_{}_{}.conf",
            std::process::id(),
            id
        ));

        // Write invalid non-UTF8 binary bytes (0xFF, 0xFE, etc.)
        let _ = fs::write(&tmp_file, [0xFF, 0xFE, 0xFD, 0x80, 0x00]);

        let mut s = Settings::default();
        // Must not panic, and must preserve default settings
        s.load_from(&tmp_file);
        assert_eq!(s.get("AUR_HELPER"), "yay");
        assert!(s.is_true("CONFIRM_ACTIONS"));

        let _ = fs::remove_file(&tmp_file);
    }
}
