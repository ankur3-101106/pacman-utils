// ──────────────────────────────────────────────────────────────────────
// mirrors/backup.rs — Mirrorlist Backup & Restoration Engine
// ──────────────────────────────────────────────────────────────────────

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::discovery::parse_mirrorlist_file;
use super::model::BackupEntry;
use crate::cmd::CommandSpec;

/// Generate a filesystem-safe timestamp string (YYYYMMDD_HHMMSS) for backup filenames.
pub fn current_file_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format timestamp using basic arithmetic
    let secs = now % 60;
    let mins = (now / 60) % 60;
    let hours = (now / 3600) % 24;
    // Approximating date (days since epoch)
    let days = now / 86400;
    // Standard Unix epoch day 0 was 1970-01-01
    let mut year = 1970;
    let mut d = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        month += 1;
    }
    let day = d + 1;

    format!("{year:04}{month:02}{day:02}_{hours:02}{mins:02}{secs:02}")
}

/// Generate a human-readable timestamp string (YYYY-MM-DD HH:MM:SS) for the current time.
pub fn current_human_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    crate::settings::format_epoch(now)
}

/// Convert any raw timestamp (epoch seconds, "epoch-<secs>", or "YYYYMMDD_HHMMSS") into
/// a human-readable format: "YYYY-MM-DD HH:MM:SS".
pub fn format_human_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();

    // If it starts with "epoch-" or "epoch_", strip it and parse as epoch seconds
    if let Some(rest) = trimmed
        .strip_prefix("epoch-")
        .or_else(|| trimmed.strip_prefix("epoch_"))
    {
        if let Ok(secs) = rest.parse::<u64>() {
            return crate::settings::format_epoch(secs);
        }
    }

    // If it's pure digits and looks like unix epoch seconds (9 to 11 digits)
    if let Ok(secs) = trimmed.parse::<u64>() {
        if trimmed.len() >= 9 && trimmed.len() <= 11 {
            return crate::settings::format_epoch(secs);
        }
    }

    // If it's YYYYMMDD_HHMMSS or YYYYMMDD-HHMMSS (length 15)
    if trimmed.len() == 15 {
        let b = trimmed.as_bytes();
        if (b[8] == b'_' || b[8] == b'-')
            && b[..8].iter().all(u8::is_ascii_digit)
            && b[9..].iter().all(u8::is_ascii_digit)
        {
            return format!(
                "{}-{}-{} {}:{}:{}",
                &trimmed[0..4],
                &trimmed[4..6],
                &trimmed[6..8],
                &trimmed[9..11],
                &trimmed[11..13],
                &trimmed[13..15]
            );
        }
    }

    // If already YYYY-MM-DD HH:MM:SS (length 19)
    if trimmed.len() == 19 && trimmed.contains('-') && trimmed.contains(':') {
        return trimmed.to_string();
    }

    trimmed.to_string()
}

/// Generate a formatted human-readable timestamp string (YYYY-MM-DD HH:MM:SS).
pub fn current_timestamp() -> String {
    current_human_timestamp()
}

/// Generate timestamped backup path for a target mirrorlist.
pub fn generate_backup_path(target: &Path) -> PathBuf {
    let ts = current_file_timestamp();
    let filename = target
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mirrorlist".to_string());

    let parent = target
        .parent()
        .unwrap_or_else(|| Path::new("/etc/pacman.d"));
    parent.join(format!("{filename}.bak.{ts}"))
}

/// Primary legacy backup path (e.g. `<mirrorlist>.bak`).
pub fn legacy_backup_path(target: &Path) -> PathBuf {
    let filename = target
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mirrorlist".to_string());
    let parent = target
        .parent()
        .unwrap_or_else(|| Path::new("/etc/pacman.d"));
    parent.join(format!("{filename}.bak"))
}

/// Create a backup using direct filesystem operations (used when running as root or in unit tests).
pub fn create_backup_fs(target: &Path) -> Result<BackupEntry, String> {
    if !target.exists() {
        return Err(format!("Target file does not exist: {}", target.display()));
    }
    let backup_path = generate_backup_path(target);
    fs::copy(target, &backup_path)
        .map_err(|e| format!("Failed to copy target to {}: {e}", backup_path.display()))?;

    // Also update legacy .bak file
    let legacy = legacy_backup_path(target);
    let _ = fs::copy(target, &legacy);

    let (active, _) = parse_mirrorlist_file(&backup_path);
    let md = fs::metadata(&backup_path).map_err(|e| e.to_string())?;

    Ok(BackupEntry {
        mirrorlist_path: target.to_path_buf(),
        backup_path,
        timestamp: current_timestamp(),
        mirror_count: active.len(),
        size_bytes: md.len(),
    })
}

/// Create a backup of a target mirrorlist file.
/// Attempts direct filesystem copying first (succeeds if running as root or file is user-writable).
/// If permission is denied, attempts privileged copy via `sudo -n cp` or `sudo cp`.
pub fn create_backup(target: &Path) -> Result<BackupEntry, String> {
    match create_backup_fs(target) {
        Ok(entry) => Ok(entry),
        Err(err) => {
            if !target.exists() {
                return Err(err);
            }
            let backup_path = generate_backup_path(target);
            let legacy = legacy_backup_path(target);
            let target_str = target.to_string_lossy().into_owned();
            let backup_str = backup_path.to_string_lossy().into_owned();
            let legacy_str = legacy.to_string_lossy().into_owned();

            let cmd_str = "cp -p \"$1\" \"$2\" && cp -p \"$1\" \"$3\"".to_string();
            // Try sudo -n first
            let status = std::process::Command::new("sudo")
                .args([
                    "-n",
                    "sh",
                    "-c",
                    &cmd_str,
                    "backup-mirrorlist",
                    &target_str,
                    &backup_str,
                    &legacy_str,
                ])
                .status();

            let success = match status {
                Ok(s) if s.success() => true,
                _ => {
                    let s2 = std::process::Command::new("sudo")
                        .args([
                            "sh",
                            "-c",
                            &cmd_str,
                            "backup-mirrorlist",
                            &target_str,
                            &backup_str,
                            &legacy_str,
                        ])
                        .status();
                    matches!(s2, Ok(s) if s.success())
                }
            };

            if success && backup_path.exists() {
                let (active, _) = parse_mirrorlist_file(&backup_path);
                let md = fs::metadata(&backup_path).map_err(|e| e.to_string())?;
                Ok(BackupEntry {
                    mirrorlist_path: target.to_path_buf(),
                    backup_path,
                    timestamp: current_timestamp(),
                    mirror_count: active.len(),
                    size_bytes: md.len(),
                })
            } else {
                Err(err)
            }
        }
    }
}

/// Construct external command spec to create a privileged backup.
pub fn build_backup_command(target: &Path, backup_path: &Path) -> CommandSpec {
    let target_str = target.to_string_lossy().into_owned();
    let backup_str = backup_path.to_string_lossy().into_owned();
    let legacy_str = legacy_backup_path(target).to_string_lossy().into_owned();

    let args = vec![
        "sh".to_string(),
        "-c".to_string(),
        "cp -p \"$1\" \"$2\" && cp -p \"$1\" \"$3\"".to_string(),
        "backup-mirrorlist".to_string(),
        target_str,
        backup_str,
        legacy_str,
    ];

    CommandSpec::new("mirrors-backup", "sudo", &args)
        .note(format!("Backup {}", target.display()))
        .result(
            "Mirrorlist backed up.",
            "Backup failed.",
            "MIRRORS: backup finished",
        )
}

/// Validate that a backup file contains valid mirror configuration.
pub fn validate_backup(backup_path: &Path) -> Result<usize, String> {
    if !backup_path.exists() {
        return Err(format!(
            "Backup file does not exist: {}",
            backup_path.display()
        ));
    }
    let md = fs::metadata(backup_path).map_err(|e| format!("Cannot read backup metadata: {e}"))?;
    if md.len() == 0 {
        return Err("Backup file is empty".to_string());
    }

    let (active, _) = parse_mirrorlist_file(backup_path);
    if active.is_empty() {
        return Err("Backup file does not contain any active 'Server = ...' entries".to_string());
    }

    for mirror in &active {
        let u = mirror.url.to_ascii_lowercase();
        if !u.starts_with("http://") && !u.starts_with("https://") && !u.starts_with("ftp://") {
            return Err(format!(
                "Backup contains invalid mirror URL: {}",
                mirror.url
            ));
        }
    }

    Ok(active.len())
}

/// List all available backups for a given mirrorlist target.
pub fn list_backups(target: &Path) -> Vec<BackupEntry> {
    let Some(parent) = target.parent() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };

    let target_name = target
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut backups = Vec::new();

    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }

        if let Some(fname) = p.file_name().and_then(|f| f.to_str()) {
            let is_match = (fname.starts_with(&target_name)
                && (fname.contains(".bak")
                    || fname.contains("-backup")
                    || fname.contains(".backup")))
                && fname != target_name;

            if is_match {
                let md = match fs::metadata(&p) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let (active, _) = parse_mirrorlist_file(&p);

                // Extract or format human-readable timestamp
                let ts = if let Some(sub) = fname.split(".bak.").nth(1) {
                    let formatted = format_human_timestamp(sub);
                    if formatted == sub && !sub.contains('-') && !sub.contains(':') {
                        if let Ok(mod_time) = md.modified() {
                            let d = mod_time
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            crate::settings::format_epoch(d)
                        } else {
                            formatted
                        }
                    } else {
                        formatted
                    }
                } else if let Ok(mod_time) = md.modified() {
                    let d = mod_time
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    crate::settings::format_epoch(d)
                } else {
                    "unknown".to_string()
                };

                backups.push(BackupEntry {
                    mirrorlist_path: target.to_path_buf(),
                    backup_path: p,
                    timestamp: ts,
                    mirror_count: active.len(),
                    size_bytes: md.len(),
                });
            }
        }
    }

    backups.sort_by(|a, b| b.backup_path.cmp(&a.backup_path));
    backups
}

/// Direct filesystem restoration of a backup file (with pre-restoration safety backup).
pub fn restore_backup_fs(backup_path: &Path, target_path: &Path) -> Result<(), String> {
    validate_backup(backup_path)?;

    // Create safety backup of the existing target if it exists
    if target_path.exists() {
        let _ = create_backup_fs(target_path);
    }

    // Atomic replacement using temp file next to target
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let temp_path = parent.join(format!(
        ".restore_tmp_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    fs::copy(backup_path, &temp_path)
        .map_err(|e| format!("Failed to copy backup to temporary file: {e}"))?;

    // Rename atomically
    if let Err(e) = fs::rename(&temp_path, target_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "Failed to atomically replace target mirrorlist: {e}"
        ));
    }

    Ok(())
}

/// Construct command spec to restore a backup with root privileges.
pub fn build_restore_command(backup_path: &Path, target_path: &Path) -> CommandSpec {
    let backup_str = backup_path.to_string_lossy().into_owned();
    let target_str = target_path.to_string_lossy().into_owned();

    let args = vec!["cp".to_string(), "-p".to_string(), backup_str, target_str];

    CommandSpec::new("mirrors-restore", "sudo", &args)
        .note(format!("Restore {} from backup", target_path.display()))
        .result(
            "Mirrorlist restored!",
            "Restore failed.",
            "MIRRORS: restore finished",
        )
}

/// Construct command spec to delete a backup file with root privileges.
pub fn build_delete_backup_command(backup_path: &Path) -> CommandSpec {
    let backup_str = backup_path.to_string_lossy().into_owned();
    let filename = backup_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| backup_str.clone());

    let args = vec!["rm".to_string(), "-f".to_string(), backup_str];

    CommandSpec::new("mirrors-delete-backup", "sudo", &args)
        .note(format!("Delete backup {filename}"))
        .result(
            "Backup deleted.",
            "Failed to delete backup.",
            "MIRRORS: delete backup finished",
        )
}

/// Direct filesystem deletion of a backup file (used in tests or as root).
pub fn delete_backup_fs(backup_path: &Path) -> Result<(), String> {
    if !backup_path.exists() {
        return Err(format!(
            "Backup file does not exist: {}",
            backup_path.display()
        ));
    }
    fs::remove_file(backup_path).map_err(|e| format!("Failed to delete backup: {e}"))
}

/// Delete a backup file from disk.
/// Attempts standard filesystem removal first (succeeds if running as root or file is user-writable).
/// If permission is denied, attempts removal via non-interactive `sudo -n rm -f` or `sudo rm -f`.
pub fn delete_backup(backup_path: &Path) -> Result<(), String> {
    match delete_backup_fs(backup_path) {
        Ok(()) => Ok(()),
        Err(err) => {
            if !backup_path.exists() {
                return Err(err);
            }
            // Try non-interactive sudo first (cached credentials or NOPASSWD)
            let status = std::process::Command::new("sudo")
                .args(["-n", "rm", "-f", backup_path.to_string_lossy().as_ref()])
                .status();
            if let Ok(s) = status {
                if s.success() {
                    return Ok(());
                }
            }

            // Try standard sudo rm -f as fallback
            let status = std::process::Command::new("sudo")
                .args(["rm", "-f", backup_path.to_string_lossy().as_ref()])
                .status();
            match status {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => Err(format!("sudo rm failed with exit status {s}")),
                Err(e) => Err(format!("Failed to execute sudo: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("archman_test_bkp_{prefix}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_backup_creation_and_history() {
        let dir = create_test_dir("creation");
        let target = dir.join("mirrorlist");
        fs::write(
            &target,
            "Server = https://mirror.example.com/$repo/os/$arch\nServer = https://backup.example.com\n",
        )
        .unwrap();

        let entry = create_backup_fs(&target).unwrap();
        assert!(entry.backup_path.exists());
        assert_eq!(entry.mirror_count, 2);

        // Also legacy .bak should exist
        let legacy = legacy_backup_path(&target);
        assert!(legacy.exists());

        // Check history
        let history = list_backups(&target);
        assert!(!history.is_empty());
        assert!(history.iter().any(|b| b.backup_path == entry.backup_path));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backup_restore_lifecycle() {
        let dir = create_test_dir("restore");
        let target = dir.join("mirrorlist");
        let bkp = dir.join("mirrorlist.bak.original");

        fs::write(&target, "Server = https://bad-corrupt-mirror.org\n").unwrap();
        fs::write(
            &bkp,
            "Server = https://good-arch-mirror.org/$repo/os/$arch\n",
        )
        .unwrap();

        // Validate backup first
        let count = validate_backup(&bkp).unwrap();
        assert_eq!(count, 1);

        // Restore
        restore_backup_fs(&bkp, &target).unwrap();

        // Verify target content now contains good mirror
        let content = fs::read_to_string(&target).unwrap();
        assert!(content.contains("https://good-arch-mirror.org"));
        assert!(!content.contains("https://bad-corrupt-mirror.org"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backup_invalid_backup() {
        let dir = create_test_dir("invalid_bkp");
        let bkp_empty = dir.join("empty.bak");
        fs::write(&bkp_empty, "").unwrap();
        assert!(validate_backup(&bkp_empty).is_err());

        let bkp_no_servers = dir.join("noservers.bak");
        fs::write(&bkp_no_servers, "# Just a comment\n# No Server lines\n").unwrap();
        assert!(validate_backup(&bkp_no_servers).is_err());

        let bkp_invalid_url = dir.join("invalidurl.bak");
        fs::write(&bkp_invalid_url, "Server = ftp-broken\n").unwrap();
        assert!(validate_backup(&bkp_invalid_url).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backup_missing_target() {
        let dir = create_test_dir("missing_target");
        let target = dir.join("does_not_exist");
        assert!(create_backup_fs(&target).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete_backup() {
        let dir = create_test_dir("del_bkp");
        let bkp = dir.join("mirrorlist.bak.test");
        fs::write(&bkp, "Server = https://example.com\n").unwrap();
        assert!(bkp.exists());

        let cmd = build_delete_backup_command(&bkp);
        assert_eq!(cmd.program, "sudo");
        assert!(cmd.args.contains(&"-f".to_string()));

        // Test delete_backup deletes file from disk
        assert!(delete_backup(&bkp).is_ok());
        assert!(!bkp.exists());
        assert!(delete_backup(&bkp).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_human_timestamp() {
        // Epoch formats
        assert_eq!(format_human_timestamp("epoch-0"), "1970-01-01 00:00:00");
        assert_eq!(
            format_human_timestamp("epoch-1700000000"),
            "2023-11-14 22:13:20"
        );
        assert_eq!(format_human_timestamp("1700000000"), "2023-11-14 22:13:20");

        // YYYYMMDD_HHMMSS formats
        assert_eq!(
            format_human_timestamp("20261031_153000"),
            "2026-10-31 15:30:00"
        );
        assert_eq!(
            format_human_timestamp("20261031-153000"),
            "2026-10-31 15:30:00"
        );

        // Already human-readable
        assert_eq!(
            format_human_timestamp("2026-10-31 15:30:00"),
            "2026-10-31 15:30:00"
        );

        // Unparseable string fallback
        assert_eq!(format_human_timestamp("custom-snapshot"), "custom-snapshot");
    }
}
