// ──────────────────────────────────────────────────────────────────────
// tx.rs — Transaction Planning and Preflight Validation Layer
//
// Separates transaction construction, preflight checks, and non-destructive
// previews from command execution. CommandEngine remains responsible
// for execution, while tx.rs handles planning and safety checks.
// ──────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use crate::cmd::{CommandEngine, CommandSpec};
use crate::settings::Settings;

pub const DISK_BLOCK_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024; // 100 MB
pub const DISK_WARN_THRESHOLD_BYTES: u64 = 1024 * 1024 * 1024; // 1 GB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
    Install,
    Remove,
    Reinstall,
    Upgrade,
    RefreshDb,
    CleanCache,
    RemoveOrphans,
    Reflector,
    Custom(String),
}

impl std::fmt::Display for OperationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationKind::Install => write!(f, "Install"),
            OperationKind::Remove => write!(f, "Remove"),
            OperationKind::Reinstall => write!(f, "Reinstall"),
            OperationKind::Upgrade => write!(f, "System Upgrade"),
            OperationKind::RefreshDb => write!(f, "Refresh Database"),
            OperationKind::CleanCache => write!(f, "Clean Cache"),
            OperationKind::RemoveOrphans => write!(f, "Remove Orphans"),
            OperationKind::Reflector => write!(f, "Reflector Mirrors"),
            OperationKind::Custom(s) => write!(f, "{s}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageManager {
    Pacman,
    Aur(String),
    System,
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageManager::Pacman => write!(f, "pacman"),
            PackageManager::Aur(h) => write!(f, "{h} (AUR)"),
            PackageManager::System => write!(f, "system"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionSpec {
    pub operation: OperationKind,
    pub manager: PackageManager,
    pub command: CommandSpec,
    pub targets: Vec<String>,
    pub requires_root: bool,
    pub requires_network: bool,
    pub preview_cmd: Option<CommandSpec>,
    pub preview_note: Option<String>,
}

impl TransactionSpec {
    pub fn summary(&self) -> String {
        if self.targets.is_empty() {
            format!("{} via {}", self.operation, self.manager)
        } else if self.targets.len() == 1 {
            format!(
                "{} {} via {}",
                self.operation, self.targets[0], self.manager
            )
        } else {
            format!(
                "{} {} packages ({}) via {}",
                self.operation,
                self.targets.len(),
                self.targets.join(", "),
                self.manager
            )
        }
    }

    /// Construct an official pacman package install transaction.
    pub fn install_official(settings: &Settings, pkg: &str) -> Self {
        let cmd_args = crate::sys::sudo_pacman_args(settings, &["-S", pkg]);
        let command = CommandSpec::new("install-official", "sudo", &cmd_args)
            .note(format!("Install {pkg}"))
            .result(
                format!("{pkg} installed successfully!"),
                "Installation failed.",
                "INSTALL: official install finished",
            );
        let preview_cmd =
            CommandSpec::capture("pacman", &["-S", "--print", pkg]).tag("preview-install");

        Self {
            operation: OperationKind::Install,
            manager: PackageManager::Pacman,
            command,
            targets: vec![pkg.to_string()],
            requires_root: true,
            requires_network: true,
            preview_cmd: Some(preview_cmd),
            preview_note: None,
        }
    }

    /// Construct an AUR package install transaction.
    pub fn install_aur(settings: &Settings, helper: &str, pkg: &str) -> Self {
        let cmd_args = crate::sys::aur_args(settings, &["-S", pkg]);
        let command = CommandSpec::new("install-aur", helper, &cmd_args)
            .note(format!("Install {pkg} from AUR via {helper}"))
            .result(
                format!("{pkg} installed successfully!"),
                "Installation failed.",
                "INSTALL: AUR install finished",
            );

        Self {
            operation: OperationKind::Install,
            manager: PackageManager::Aur(helper.to_string()),
            command,
            targets: vec![pkg.to_string()],
            requires_root: false,
            requires_network: true,
            preview_cmd: None,
            preview_note: Some(format!(
                "Non-destructive preview unavailable for {helper} operations."
            )),
        }
    }

    /// Construct a package remove transaction.
    pub fn remove(settings: &Settings, flag: &'static str, pkg: &str) -> Self {
        let cmd_args = crate::sys::sudo_pacman_args(settings, &[flag, pkg]);
        let command = CommandSpec::new("remove", "sudo", &cmd_args)
            .note(format!("pacman {flag} {pkg}"))
            .result(
                "Package removed successfully!",
                "Removal may have failed. Package is still installed.",
                "REMOVE: finished",
            );

        // Note: pacman -Rns rejects --print because --nosave cannot be used with --print.
        // For -Rns we preview with -Rs and provide an explanatory note.
        let (preview_flag, note) = match flag {
            "-Rns" => (
                "-Rs",
                Some(
                    "Preview using -Rs; configuration files will also be removed (--nosave)."
                        .to_string(),
                ),
            ),
            "-Rs" => ("-Rs", None),
            _ => ("-R", None),
        };
        let preview_cmd =
            CommandSpec::capture("pacman", &[preview_flag, "--print", pkg]).tag("preview-remove");

        Self {
            operation: OperationKind::Remove,
            manager: PackageManager::Pacman,
            command,
            targets: vec![pkg.to_string()],
            requires_root: true,
            requires_network: false,
            preview_cmd: Some(preview_cmd),
            preview_note: note,
        }
    }

    /// Construct a package reinstall transaction.
    pub fn reinstall(settings: &Settings, pkg: &str) -> Self {
        let cmd_args = crate::sys::sudo_pacman_args(settings, &["-S", "--overwrite", "*", pkg]);
        let command = CommandSpec::new("reinstall", "sudo", &cmd_args)
            .note(format!("Reinstall {pkg} (--overwrite '*')"))
            .result(
                format!("{pkg} reinstalled successfully!"),
                "Reinstallation failed.",
                "PACKAGES: reinstall finished",
            );
        let preview_cmd =
            CommandSpec::capture("pacman", &["-S", "--print", pkg]).tag("preview-reinstall");

        Self {
            operation: OperationKind::Reinstall,
            manager: PackageManager::Pacman,
            command,
            targets: vec![pkg.to_string()],
            requires_root: true,
            requires_network: true,
            preview_cmd: Some(preview_cmd),
            preview_note: None,
        }
    }

    /// Construct an orphan package removal transaction.
    pub fn remove_orphans(settings: &Settings, orphans: &[String]) -> Self {
        let mut full_args = vec!["-Rns".to_string()];
        full_args.extend(orphans.iter().cloned());
        let cmd_args = crate::sys::sudo_pacman_args(settings, &full_args);
        let command = CommandSpec::new("orphans-rm", "sudo", &cmd_args)
            .note(format!("Remove {} orphan package(s)", orphans.len()))
            .result(
                "Orphans removed successfully!",
                "Removal failed.",
                "PACKAGES: orphans removal finished",
            );

        let mut preview_args = vec!["-Rs".to_string(), "--print".to_string()];
        preview_args.extend(orphans.iter().cloned());
        let preview_cmd =
            CommandSpec::capture_owned("pacman", &preview_args).tag("preview-orphans");

        Self {
            operation: OperationKind::RemoveOrphans,
            manager: PackageManager::Pacman,
            command,
            targets: orphans.to_vec(),
            requires_root: true,
            requires_network: false,
            preview_cmd: Some(preview_cmd),
            preview_note: Some(
                "Preview using -Rs; configuration files will also be removed (--nosave)."
                    .to_string(),
            ),
        }
    }

    /// Construct a full system upgrade via pacman.
    pub fn upgrade_pacman(settings: &Settings) -> Self {
        let cmd_args = crate::sys::sudo_pacman_args(settings, &["-Syu"]);
        let command = CommandSpec::new("run-upgrade", "sudo", &cmd_args)
            .note("Full system upgrade")
            .result(
                "Full system upgrade completed successfully!",
                "System upgrade failed.",
                "REGISTRY: pacman upgrade finished",
            );
        let preview_cmd =
            CommandSpec::capture("pacman", &["-Su", "--print"]).tag("preview-upgrade");

        Self {
            operation: OperationKind::Upgrade,
            manager: PackageManager::Pacman,
            command,
            targets: Vec::new(),
            requires_root: true,
            requires_network: true,
            preview_cmd: Some(preview_cmd),
            preview_note: None,
        }
    }

    /// Construct a full system upgrade via AUR helper.
    pub fn upgrade_aur(settings: &Settings, helper: &str) -> Self {
        let cmd_args = crate::sys::aur_args(settings, &["-Syu"]);
        let command = CommandSpec::new("run-upgrade-aur", helper, &cmd_args)
            .note(format!("Full system upgrade ({helper})"))
            .result(
                "Full system upgrade completed successfully!",
                "System upgrade failed.",
                "REGISTRY: AUR upgrade finished",
            );

        Self {
            operation: OperationKind::Upgrade,
            manager: PackageManager::Aur(helper.to_string()),
            command,
            targets: Vec::new(),
            requires_root: false,
            requires_network: true,
            preview_cmd: None,
            preview_note: Some(format!(
                "Non-destructive preview unavailable for {helper} upgrade."
            )),
        }
    }

    /// Construct a database refresh transaction.
    pub fn refresh_db(settings: &Settings, force: bool) -> Self {
        let flag = if force { "-Syy" } else { "-Sy" };
        let tag = if force { "run-db-syy" } else { "run-db-sy" };
        let note = if force {
            "Force refreshing package databases"
        } else {
            "Refreshing package databases"
        };
        let cmd_args = crate::sys::sudo_pacman_args(settings, &[flag]);
        let command = CommandSpec::new(tag, "sudo", &cmd_args).note(note);

        Self {
            operation: OperationKind::RefreshDb,
            manager: PackageManager::Pacman,
            command,
            targets: Vec::new(),
            requires_root: true,
            requires_network: true,
            preview_cmd: None,
            preview_note: Some(
                "Database refresh downloads sync databases from remote mirrors (no package changes)."
                    .to_string(),
            ),
        }
    }

    /// Construct a package cache clean transaction.
    pub fn clean_cache(settings: &Settings, all: bool) -> Self {
        let flag = if all { "-Scc" } else { "-Sc" };
        let tag = if all { "run-cache-scc" } else { "run-cache-sc" };
        let note = if all {
            "Remove ALL cached packages"
        } else {
            "Clean package cache"
        };
        let cmd_args = crate::sys::sudo_pacman_args(settings, &[flag]);
        let command = CommandSpec::new(tag, "sudo", &cmd_args).note(note);

        Self {
            operation: OperationKind::CleanCache,
            manager: PackageManager::Pacman,
            command,
            targets: Vec::new(),
            requires_root: true,
            requires_network: false,
            preview_cmd: None,
            preview_note: Some(
                if all {
                    "Removes ALL files from /var/cache/pacman/pkg."
                } else {
                    "Removes uninstalled packages from /var/cache/pacman/pkg."
                }
                .to_string(),
            ),
        }
    }

    /// Generate a non-destructive preview using the centralized CommandEngine.
    pub fn generate_preview(&self, engine: &CommandEngine) -> PreviewResult {
        if let Some(spec) = &self.preview_cmd {
            match engine.run_capture(spec) {
                Ok(result) => {
                    let mut lines = Vec::new();
                    let out_text = result.stdout_lossy();
                    for line in out_text.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            lines.push(trimmed.to_string());
                        }
                    }
                    if lines.is_empty() && !result.is_success() {
                        let err_text = result.stderr_lossy();
                        for line in err_text.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                lines.push(trimmed.to_string());
                            }
                        }
                    }
                    PreviewResult {
                        lines,
                        is_authoritative: true,
                        note: self.preview_note.clone(),
                    }
                }
                Err(e) => PreviewResult {
                    lines: vec![format!("Failed to generate preview: {e}")],
                    is_authoritative: false,
                    note: self.preview_note.clone(),
                },
            }
        } else {
            PreviewResult {
                lines: Vec::new(),
                is_authoritative: false,
                note: self.preview_note.clone().or_else(|| {
                    Some("Non-destructive preview unavailable for this operation.".to_string())
                }),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub lines: Vec<String>,
    pub is_authoritative: bool,
    pub note: Option<String>,
}

// ── Preflight Checks ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightStatus {
    Pass,
    Warning,
    Block,
}

impl PreflightStatus {
    pub fn label(&self) -> &'static str {
        match self {
            PreflightStatus::Pass => "PASS",
            PreflightStatus::Warning => "WARN",
            PreflightStatus::Block => "BLOCK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightCheck {
    pub name: String,
    pub status: PreflightStatus,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PreflightResult {
    pub checks: Vec<PreflightCheck>,
}

impl PreflightResult {
    pub fn is_blocked(&self) -> bool {
        self.checks
            .iter()
            .any(|c| c.status == PreflightStatus::Block)
    }

    pub fn has_warnings(&self) -> bool {
        self.checks
            .iter()
            .any(|c| c.status == PreflightStatus::Warning)
    }
}

/// Environment interface for preflight checks to allow deterministic unit testing.
#[derive(Clone)]
pub struct PreflightEnv {
    pub lock_exists: bool,
    pub active_pids: Vec<String>,
    pub has_bin: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    pub is_root: bool,
    pub sudo_noninteractive: bool,
    pub network_available: bool,
    pub disk_available_bytes: u64,
}

impl PreflightEnv {
    /// Probe the real system environment.
    pub fn system(engine: &CommandEngine) -> Self {
        let lock_exists = std::path::Path::new(crate::sys::LOCK_FILE).exists();
        let active_pids = crate::sys::pacman_pids();

        let has_bin = Arc::new(|name: &str| {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    let bin = dir.join(name);
                    if bin.is_file() {
                        return true;
                    }
                }
            }
            false
        });

        let is_root = unsafe { libc::geteuid() == 0 };

        // Test non-interactive sudo capability (never prompts).
        let sudo_check = CommandSpec::capture("sudo", &["-n", "true"]).tag("sudo-check");
        let sudo_noninteractive = engine
            .run_capture(&sudo_check)
            .map(|r| r.is_success())
            .unwrap_or(false);

        // Check network availability via default gateway in /proc/net/route.
        let network_available = std::fs::read_to_string("/proc/net/route")
            .map(|s| {
                s.lines()
                    .skip(1)
                    .any(|l| l.split_whitespace().nth(1) == Some("00000000"))
            })
            .unwrap_or(false);

        // Check available bytes on root filesystem via statvfs (panic-free).
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        const ROOT_PATH: &[u8] = b"/\0";
        let disk_available_bytes = if unsafe {
            libc::statvfs(ROOT_PATH.as_ptr() as *const libc::c_char, &mut stat)
        } == 0
        {
            stat.f_bavail as u64 * stat.f_frsize as u64
        } else {
            u64::MAX
        };

        Self {
            lock_exists,
            active_pids,
            has_bin,
            is_root,
            sudo_noninteractive,
            network_available,
            disk_available_bytes,
        }
    }

    /// Construct a deterministic mock environment for testing.
    pub fn mock() -> Self {
        Self {
            lock_exists: false,
            active_pids: Vec::new(),
            has_bin: Arc::new(|_| true),
            is_root: false,
            sudo_noninteractive: true,
            network_available: true,
            disk_available_bytes: 10 * 1024 * 1024 * 1024, // 10 GiB
        }
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Run all deterministic preflight checks for a transaction spec.
pub fn run_preflight(tx: &TransactionSpec, env: &PreflightEnv) -> PreflightResult {
    let mut checks = Vec::new();

    // 1. Pacman database lock check
    let lock_check = if env.lock_exists {
        if !env.active_pids.is_empty() {
            PreflightCheck {
                name: "Database Lock".to_string(),
                status: PreflightStatus::Block,
                message: format!(
                    "Pacman database is locked by active process (PID: {}).",
                    env.active_pids.join(", ")
                ),
            }
        } else {
            PreflightCheck {
                name: "Database Lock".to_string(),
                status: PreflightStatus::Block,
                message: format!(
                    "Pacman database is locked ({}). If stale, remove via Lock File screen.",
                    crate::sys::LOCK_FILE
                ),
            }
        }
    } else {
        PreflightCheck {
            name: "Database Lock".to_string(),
            status: PreflightStatus::Pass,
            message: "Pacman database is unlocked".to_string(),
        }
    };
    checks.push(lock_check);

    // 2. Executable availability check
    let prog = &tx.command.program;
    let mut missing = Vec::new();
    if !(env.has_bin)(prog) {
        missing.push(prog.clone());
    }
    if prog == "sudo" && !tx.command.args.is_empty() {
        let sub_bin = &tx.command.args[0];
        if !(env.has_bin)(sub_bin) {
            missing.push(sub_bin.clone());
        }
    }
    let exec_check = if missing.is_empty() {
        PreflightCheck {
            name: "Executables".to_string(),
            status: PreflightStatus::Pass,
            message: format!("Required binary '{prog}' available in PATH"),
        }
    } else {
        PreflightCheck {
            name: "Executables".to_string(),
            status: PreflightStatus::Block,
            message: format!("Required binary '{}' not found in PATH", missing.join(", ")),
        }
    };
    checks.push(exec_check);

    // 3. Privilege readiness check (never prompts interactively)
    let priv_check = if !tx.requires_root {
        PreflightCheck {
            name: "Privileges".to_string(),
            status: PreflightStatus::Pass,
            message: "No elevated privileges required".to_string(),
        }
    } else if env.is_root {
        PreflightCheck {
            name: "Privileges".to_string(),
            status: PreflightStatus::Pass,
            message: "Running as root".to_string(),
        }
    } else if env.sudo_noninteractive {
        PreflightCheck {
            name: "Privileges".to_string(),
            status: PreflightStatus::Pass,
            message: "Sudo privilege escalation available without prompt".to_string(),
        }
    } else {
        PreflightCheck {
            name: "Privileges".to_string(),
            status: PreflightStatus::Warning,
            message: "Root privileges required; sudo password will be prompted on execution"
                .to_string(),
        }
    };
    checks.push(priv_check);

    // 4. Network requirement check
    let net_check = if !tx.requires_network {
        PreflightCheck {
            name: "Network".to_string(),
            status: PreflightStatus::Pass,
            message: "Local operation; no network access required".to_string(),
        }
    } else if env.network_available {
        PreflightCheck {
            name: "Network".to_string(),
            status: PreflightStatus::Pass,
            message: "Network connectivity verified".to_string(),
        }
    } else {
        PreflightCheck {
            name: "Network".to_string(),
            status: PreflightStatus::Warning,
            message: "Network may be offline; transaction requires remote repository access"
                .to_string(),
        }
    };
    checks.push(net_check);

    // 5. Disk space check
    let disk_check = if env.disk_available_bytes < DISK_BLOCK_THRESHOLD_BYTES {
        PreflightCheck {
            name: "Disk Space".to_string(),
            status: PreflightStatus::Block,
            message: format!(
                "Critically low disk space (< {} available on /)",
                format_bytes(DISK_BLOCK_THRESHOLD_BYTES)
            ),
        }
    } else if env.disk_available_bytes < DISK_WARN_THRESHOLD_BYTES {
        PreflightCheck {
            name: "Disk Space".to_string(),
            status: PreflightStatus::Warning,
            message: format!(
                "Low disk space ({} available on /)",
                format_bytes(env.disk_available_bytes)
            ),
        }
    } else {
        PreflightCheck {
            name: "Disk Space".to_string(),
            status: PreflightStatus::Pass,
            message: format!(
                "Disk space sufficient ({} available on /)",
                format_bytes(env.disk_available_bytes)
            ),
        }
    };
    checks.push(disk_check);

    // 6. Target validation check
    let target_check = if tx.targets.is_empty() {
        match tx.operation {
            OperationKind::Install
            | OperationKind::Remove
            | OperationKind::Reinstall
            | OperationKind::RemoveOrphans => PreflightCheck {
                name: "Targets".to_string(),
                status: PreflightStatus::Block,
                message: "No package targets specified for transaction".to_string(),
            },
            _ => PreflightCheck {
                name: "Targets".to_string(),
                status: PreflightStatus::Pass,
                message: "No package targets required for this operation".to_string(),
            },
        }
    } else {
        let mut invalid = Vec::new();
        for t in &tx.targets {
            if t.trim().is_empty()
                || t.chars()
                    .any(|c| c.is_whitespace() || ";|&<>$`\"'\\".contains(c))
            {
                invalid.push(t.clone());
            }
        }
        if invalid.is_empty() {
            PreflightCheck {
                name: "Targets".to_string(),
                status: PreflightStatus::Pass,
                message: format!("Targets validated ({} package(s))", tx.targets.len()),
            }
        } else {
            PreflightCheck {
                name: "Targets".to_string(),
                status: PreflightStatus::Block,
                message: format!(
                    "Target specification contains invalid characters: {}",
                    invalid.join(", ")
                ),
            }
        }
    };
    checks.push(target_check);

    PreflightResult { checks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn test_preflight_no_lock_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let env = PreflightEnv::mock();
        let res = run_preflight(&tx, &env);

        let lock = res
            .checks
            .iter()
            .find(|c| c.name == "Database Lock")
            .unwrap();
        assert_eq!(lock.status, PreflightStatus::Pass);
        assert!(!res.is_blocked());
    }

    #[test]
    fn test_preflight_lock_active_pacman_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.lock_exists = true;
        env.active_pids = vec!["1234".into()];

        let res = run_preflight(&tx, &env);
        let lock = res
            .checks
            .iter()
            .find(|c| c.name == "Database Lock")
            .unwrap();
        assert_eq!(lock.status, PreflightStatus::Block);
        assert!(lock.message.contains("PID: 1234"));
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_lock_stale_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.lock_exists = true;
        env.active_pids = Vec::new();

        let res = run_preflight(&tx, &env);
        let lock = res
            .checks
            .iter()
            .find(|c| c.name == "Database Lock")
            .unwrap();
        assert_eq!(lock.status, PreflightStatus::Block);
        assert!(lock.message.contains("Lock File screen"));
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_executable_missing_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.has_bin = Arc::new(|name| name != "sudo");

        let res = run_preflight(&tx, &env);
        let exec = res.checks.iter().find(|c| c.name == "Executables").unwrap();
        assert_eq!(exec.status, PreflightStatus::Block);
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_executable_present_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let env = PreflightEnv::mock();

        let res = run_preflight(&tx, &env);
        let exec = res.checks.iter().find(|c| c.name == "Executables").unwrap();
        assert_eq!(exec.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_preflight_privilege_root_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.is_root = true;

        let res = run_preflight(&tx, &env);
        let priv_chk = res.checks.iter().find(|c| c.name == "Privileges").unwrap();
        assert_eq!(priv_chk.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_preflight_privilege_sudo_cached_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.is_root = false;
        env.sudo_noninteractive = true;

        let res = run_preflight(&tx, &env);
        let priv_chk = res.checks.iter().find(|c| c.name == "Privileges").unwrap();
        assert_eq!(priv_chk.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_preflight_privilege_sudo_uncached_warn() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.is_root = false;
        env.sudo_noninteractive = false;

        let res = run_preflight(&tx, &env);
        let priv_chk = res.checks.iter().find(|c| c.name == "Privileges").unwrap();
        assert_eq!(priv_chk.status, PreflightStatus::Warning);
        // Warning should not block
        assert!(!res.is_blocked());
    }

    #[test]
    fn test_preflight_network_required_offline_warn() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.network_available = false;

        let res = run_preflight(&tx, &env);
        let net = res.checks.iter().find(|c| c.name == "Network").unwrap();
        assert_eq!(net.status, PreflightStatus::Warning);
        assert!(!res.is_blocked());
    }

    #[test]
    fn test_preflight_network_not_required_offline_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::remove(&settings, "-R", "ripgrep");
        let mut env = PreflightEnv::mock();
        env.network_available = false;

        let res = run_preflight(&tx, &env);
        let net = res.checks.iter().find(|c| c.name == "Network").unwrap();
        assert_eq!(net.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_preflight_disk_space_critical_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.disk_available_bytes = 50 * 1024 * 1024; // 50 MB < 100 MB

        let res = run_preflight(&tx, &env);
        let disk = res.checks.iter().find(|c| c.name == "Disk Space").unwrap();
        assert_eq!(disk.status, PreflightStatus::Block);
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_disk_space_low_warn() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.disk_available_bytes = 500 * 1024 * 1024; // 500 MB (100 MB .. 1 GB)

        let res = run_preflight(&tx, &env);
        let disk = res.checks.iter().find(|c| c.name == "Disk Space").unwrap();
        assert_eq!(disk.status, PreflightStatus::Warning);
        assert!(!res.is_blocked());
    }

    #[test]
    fn test_preflight_disk_space_sufficient_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let mut env = PreflightEnv::mock();
        env.disk_available_bytes = 5 * 1024 * 1024 * 1024; // 5 GiB > 1 GB

        let res = run_preflight(&tx, &env);
        let disk = res.checks.iter().find(|c| c.name == "Disk Space").unwrap();
        assert_eq!(disk.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_preflight_target_validation_invalid_chars_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "foo;rm -rf /");
        let env = PreflightEnv::mock();

        let res = run_preflight(&tx, &env);
        let targets = res.checks.iter().find(|c| c.name == "Targets").unwrap();
        assert_eq!(targets.status, PreflightStatus::Block);
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_target_validation_empty_block() {
        let settings = Settings::default();
        let mut tx = TransactionSpec::install_official(&settings, "ripgrep");
        tx.targets.clear();
        let env = PreflightEnv::mock();

        let res = run_preflight(&tx, &env);
        let targets = res.checks.iter().find(|c| c.name == "Targets").unwrap();
        assert_eq!(targets.status, PreflightStatus::Block);
        assert!(res.is_blocked());
    }

    #[test]
    fn test_preflight_target_validation_valid_pass() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        let env = PreflightEnv::mock();

        let res = run_preflight(&tx, &env);
        let targets = res.checks.iter().find(|c| c.name == "Targets").unwrap();
        assert_eq!(targets.status, PreflightStatus::Pass);
    }

    #[test]
    fn test_tx_install_official_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        assert_eq!(tx.operation, OperationKind::Install);
        assert_eq!(tx.manager, PackageManager::Pacman);
        assert!(tx.requires_root);
        assert!(tx.requires_network);
        assert!(tx.preview_cmd.is_some());
        let pcmd = tx.preview_cmd.unwrap();
        assert_eq!(pcmd.program, "pacman");
        assert_eq!(pcmd.args, vec!["-S", "--print", "ripgrep"]);
    }

    #[test]
    fn test_tx_install_aur_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_aur(&settings, "yay", "visual-studio-code-bin");
        assert_eq!(tx.operation, OperationKind::Install);
        assert_eq!(tx.manager, PackageManager::Aur("yay".into()));
        assert!(!tx.requires_root);
        assert!(tx.requires_network);
        assert!(tx.preview_cmd.is_none());
        assert!(tx.preview_note.is_some());
    }

    #[test]
    fn test_tx_remove_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::remove(&settings, "-Rns", "ripgrep");
        assert_eq!(tx.operation, OperationKind::Remove);
        assert_eq!(tx.manager, PackageManager::Pacman);
        assert!(tx.requires_root);
        assert!(!tx.requires_network);
        // Preview uses -Rs to avoid --nosave error with --print
        assert!(tx.preview_cmd.is_some());
        let pcmd = tx.preview_cmd.unwrap();
        assert_eq!(pcmd.args, vec!["-Rs", "--print", "ripgrep"]);
    }

    #[test]
    fn test_preview_does_not_inject_noconfirm() {
        let mut settings = Settings::default();
        settings.set("NATIVE_CONFIRM", "false");
        let tx = TransactionSpec::install_official(&settings, "ripgrep");
        // Real command gets --noconfirm because NATIVE_CONFIRM=false
        assert!(tx.command.args.contains(&"--noconfirm".to_string()));
        // But preview command NEVER has --noconfirm
        let pcmd = tx.preview_cmd.unwrap();
        assert!(!pcmd.args.contains(&"--noconfirm".to_string()));
    }

    #[test]
    fn test_tx_reinstall_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::reinstall(&settings, "neovim");
        assert_eq!(tx.operation, OperationKind::Reinstall);
        assert_eq!(tx.manager, PackageManager::Pacman);
        assert!(tx.requires_root);
        assert!(tx.requires_network);
        assert_eq!(tx.targets, vec!["neovim"]);
        assert_eq!(tx.command.program, "sudo");
        assert!(tx.command.args.contains(&"--overwrite".to_string()));
        let pcmd = tx.preview_cmd.expect("preview_cmd must be present");
        assert_eq!(pcmd.program, "pacman");
        assert_eq!(pcmd.args, vec!["-S", "--print", "neovim"]);
    }

    #[test]
    fn test_tx_remove_orphans_spec() {
        let settings = Settings::default();
        let orphans = vec!["liborphan1".to_string(), "liborphan2".to_string()];
        let tx = TransactionSpec::remove_orphans(&settings, &orphans);
        assert_eq!(tx.operation, OperationKind::RemoveOrphans);
        assert_eq!(tx.manager, PackageManager::Pacman);
        assert!(tx.requires_root);
        assert!(!tx.requires_network);
        assert_eq!(tx.targets, orphans);
        assert_eq!(tx.command.program, "sudo");
        assert!(tx.command.args.contains(&"-Rns".to_string()));
        let pcmd = tx.preview_cmd.expect("preview_cmd must be present");
        assert_eq!(pcmd.program, "pacman");
        assert_eq!(
            pcmd.args,
            vec!["-Rs", "--print", "liborphan1", "liborphan2"]
        );
        assert!(tx.preview_note.is_some());
    }

    #[test]
    fn test_tx_upgrade_pacman_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::upgrade_pacman(&settings);
        assert_eq!(tx.operation, OperationKind::Upgrade);
        assert_eq!(tx.manager, PackageManager::Pacman);
        assert!(tx.requires_root);
        assert!(tx.requires_network);
        assert_eq!(tx.command.program, "sudo");
        assert_eq!(tx.command.args, vec!["pacman", "-Syu"]);
        let pcmd = tx.preview_cmd.expect("preview_cmd must be present");
        assert_eq!(pcmd.program, "pacman");
        assert_eq!(pcmd.args, vec!["-Su", "--print"]);
    }

    #[test]
    fn test_tx_upgrade_aur_spec() {
        let settings = Settings::default();
        let tx = TransactionSpec::upgrade_aur(&settings, "paru");
        assert_eq!(tx.operation, OperationKind::Upgrade);
        assert_eq!(tx.manager, PackageManager::Aur("paru".into()));
        assert!(!tx.requires_root);
        assert!(tx.requires_network);
        assert_eq!(tx.command.program, "paru");
        assert_eq!(tx.command.args, vec!["-Syu"]);
        assert!(tx.preview_cmd.is_none());
        assert!(tx.preview_note.is_some());
    }

    #[test]
    fn test_tx_refresh_db_spec() {
        let settings = Settings::default();

        let tx_normal = TransactionSpec::refresh_db(&settings, false);
        assert_eq!(tx_normal.operation, OperationKind::RefreshDb);
        assert_eq!(tx_normal.manager, PackageManager::Pacman);
        assert!(tx_normal.requires_root);
        assert!(tx_normal.requires_network);
        assert_eq!(tx_normal.command.args, vec!["pacman", "-Sy"]);
        assert!(tx_normal.preview_cmd.is_none());

        let tx_force = TransactionSpec::refresh_db(&settings, true);
        assert_eq!(tx_force.operation, OperationKind::RefreshDb);
        assert_eq!(tx_force.manager, PackageManager::Pacman);
        assert_eq!(tx_force.command.args, vec!["pacman", "-Syy"]);
    }

    #[test]
    fn test_tx_clean_cache_spec() {
        let settings = Settings::default();

        let tx_clean = TransactionSpec::clean_cache(&settings, false);
        assert_eq!(tx_clean.operation, OperationKind::CleanCache);
        assert_eq!(tx_clean.manager, PackageManager::Pacman);
        assert!(tx_clean.requires_root);
        assert!(!tx_clean.requires_network);
        assert_eq!(tx_clean.command.args, vec!["pacman", "-Sc"]);
        assert!(tx_clean.preview_cmd.is_none());

        let tx_clean_all = TransactionSpec::clean_cache(&settings, true);
        assert_eq!(tx_clean_all.operation, OperationKind::CleanCache);
        assert_eq!(tx_clean_all.command.args, vec!["pacman", "-Scc"]);
    }

    #[test]
    fn test_tx_remove_flag_variations() {
        let settings = Settings::default();

        let tx_r = TransactionSpec::remove(&settings, "-R", "pkg");
        assert_eq!(tx_r.command.args, vec!["pacman", "-R", "pkg"]);
        assert_eq!(tx_r.preview_cmd.unwrap().args, vec!["-R", "--print", "pkg"]);

        let tx_rs = TransactionSpec::remove(&settings, "-Rs", "pkg");
        assert_eq!(tx_rs.command.args, vec!["pacman", "-Rs", "pkg"]);
        assert_eq!(
            tx_rs.preview_cmd.unwrap().args,
            vec!["-Rs", "--print", "pkg"]
        );

        let tx_rns = TransactionSpec::remove(&settings, "-Rns", "pkg");
        assert_eq!(tx_rns.command.args, vec!["pacman", "-Rns", "pkg"]);
        // Preview safely falls back to -Rs because --print rejects -n/--nosave
        assert_eq!(
            tx_rns.preview_cmd.unwrap().args,
            vec!["-Rs", "--print", "pkg"]
        );
    }

    #[test]
    fn test_preflight_aur_helper_missing_block() {
        let settings = Settings::default();
        let tx = TransactionSpec::install_aur(&settings, "yay", "visual-studio-code-bin");
        let mut env = PreflightEnv::mock();
        // simulate yay not existing in PATH
        env.has_bin = Arc::new(|b| b != "yay");

        let res = run_preflight(&tx, &env);
        let exec = res.checks.iter().find(|c| c.name == "Executables").unwrap();
        assert_eq!(exec.status, PreflightStatus::Block);
        assert!(res.is_blocked());
    }

    #[test]
    fn test_tx_target_validation_unusual_valid_chars() {
        let settings = Settings::default();
        // Arch package names can contain dashes, underscores, dots, pluses, @ signs
        for pkg in &[
            "linux-lts",
            "gcc-libs+",
            "gtk3@1.2",
            "python-setuptools_scm",
            "x86_64-pkg.tar.zst",
        ] {
            let tx = TransactionSpec::install_official(&settings, pkg);
            let env = PreflightEnv::mock();
            let res = run_preflight(&tx, &env);
            let targets = res.checks.iter().find(|c| c.name == "Targets").unwrap();
            assert_eq!(
                targets.status,
                PreflightStatus::Pass,
                "pkg {pkg} should pass validation"
            );
        }
    }

    #[test]
    fn test_tx_target_validation_whitespace_or_empty_block() {
        let settings = Settings::default();
        for bad_pkg in &["", "   ", "pkg with space", "pkg\tname"] {
            let mut tx = TransactionSpec::install_official(&settings, "dummy");
            tx.targets = vec![bad_pkg.to_string()];
            let env = PreflightEnv::mock();
            let res = run_preflight(&tx, &env);
            let targets = res.checks.iter().find(|c| c.name == "Targets").unwrap();
            assert_eq!(
                targets.status,
                PreflightStatus::Block,
                "bad pkg {bad_pkg:?} should block"
            );
            assert!(res.is_blocked());
        }
    }

    #[test]
    fn test_preview_generation_command_failure_captured() {
        let settings = Settings::default();
        let mut tx = TransactionSpec::install_official(&settings, "dummy");
        // Replace preview command with one that writes to stderr and exits with failure
        tx.preview_cmd = Some(CommandSpec::capture(
            "sh",
            &["-c", "echo 'failed to resolve dependencies' >&2; exit 1"],
        ));
        let engine = CommandEngine::new();
        let preview = tx.generate_preview(&engine);
        assert_eq!(preview.lines, vec!["failed to resolve dependencies"]);
    }
}
