// ──────────────────────────────────────────────────────────────────────
// sys.rs — System command wrappers
//
// All pacman / AUR helper / reflector / filesystem queries live here so
// screens only deal with plain data. Long-running interactive commands
// are NOT run here — those go through App::run_external (see app.rs),
// which suspends the TUI and hands the terminal to the child process.
// ──────────────────────────────────────────────────────────────────────

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use crate::cmd::{CommandEngine, CommandSpec};

// ── Constants (mirroring the bash version) ──────────────────────────

pub const LOCK_FILE: &str = "/var/lib/pacman/db.lck";
pub const MIRRORLIST: &str = "/etc/pacman.d/mirrorlist";
pub const MIRRORLIST_BACKUP: &str = "/etc/pacman.d/mirrorlist.bak";
pub const PACMAN_LOG: &str = "/var/log/pacman.log";
pub const CACHE_DIR: &str = "/var/cache/pacman/pkg";

/// The 12 curated package groups from lib/groups.sh.
pub const PKG_GROUPS: [(&str, &str, &[&str]); 12] = [
    (
        "gnome",
        "GNOME Desktop Environment",
        &["gnome", "gnome-extra", "gdm", "gnome-tweaks"],
    ),
    (
        "kde",
        "KDE Plasma Desktop",
        &["plasma", "kde-applications", "sddm"],
    ),
    (
        "hyprland",
        "Hyprland Wayland Compositor",
        &[
            "hyprland", "waybar", "wofi", "kitty", "swaybg", "swaylock", "mako", "grim", "slurp",
        ],
    ),
    (
        "sway",
        "Sway Wayland Compositor",
        &[
            "sway", "swaylock", "swayidle", "waybar", "wofi", "foot", "mako", "grim", "slurp",
        ],
    ),
    (
        "i3",
        "i3 Window Manager",
        &[
            "i3-wm",
            "i3status",
            "i3lock",
            "dmenu",
            "alacritty",
            "picom",
            "feh",
            "dunst",
        ],
    ),
    (
        "dev",
        "Development Essentials",
        &[
            "base-devel",
            "git",
            "nodejs",
            "npm",
            "python",
            "python-pip",
            "go",
            "rustup",
            "docker",
            "docker-compose",
        ],
    ),
    (
        "gaming",
        "Gaming (Steam, Lutris, Wine)",
        &[
            "steam",
            "lutris",
            "wine-staging",
            "gamemode",
            "lib32-mesa",
            "lib32-vulkan-icd-loader",
            "mangohud",
        ],
    ),
    (
        "multimedia",
        "Multimedia (Video, Audio, Graphics)",
        &[
            "vlc",
            "obs-studio",
            "gimp",
            "inkscape",
            "audacity",
            "ffmpeg",
            "mpv",
            "imagemagick",
        ],
    ),
    (
        "networking",
        "Networking Tools",
        &[
            "networkmanager",
            "nm-connection-editor",
            "openssh",
            "curl",
            "wget",
            "nmap",
            "wireshark-qt",
        ],
    ),
    (
        "fonts",
        "Essential Fonts",
        &[
            "ttf-dejavu",
            "ttf-liberation",
            "noto-fonts",
            "noto-fonts-cjk",
            "noto-fonts-emoji",
            "ttf-fira-code",
            "ttf-jetbrains-mono",
        ],
    ),
    (
        "terminal",
        "Terminal Power Tools",
        &[
            "zsh", "fish", "starship", "tmux", "neovim", "htop", "btop", "bat", "eza", "fd",
            "ripgrep", "fzf", "gum",
        ],
    ),
    (
        "security",
        "Security Tools",
        &["ufw", "gufw", "clamav", "firejail", "keepassxc", "gnupg"],
    ),
];

// ── Capabilities ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Caps {
    pub reflector: bool,
    pub paccache: bool,
    pub checkupdates: bool,
}

impl Caps {
    pub fn detect() -> Self {
        Self {
            reflector: has_bin("reflector"),
            paccache: has_bin("paccache"),
            checkupdates: has_bin("checkupdates"),
        }
    }
}

/// True if `name` resolves to an executable on PATH.
pub fn has_bin(name: &str) -> bool {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join(name);
        if let Ok(md) = fs::metadata(&candidate) {
            use std::os::unix::fs::PermissionsExt;
            if md.is_file() && (md.permissions().mode() & 0o111 != 0) {
                return true;
            }
        }
    }
    false
}

/// Resolve which AUR helper to use, respecting the user's setting.
pub fn get_aur_helper(settings: &crate::settings::Settings) -> Option<String> {
    let configured = settings.get("AUR_HELPER");
    if !configured.is_empty() && has_bin(configured) {
        return Some(configured.to_string());
    }
    for candidate in ["yay", "paru"] {
        if has_bin(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

// ── Command Capture Helpers ─────────────────────────────────────────

/// Run a command and return its trimmed stdout, or None on failure.
pub fn capture(program: &str, args: &[&str]) -> Option<String> {
    let spec = CommandSpec::capture(program, args);
    let res = CommandEngine::default().run_capture(&spec).ok()?;
    if res.is_success() {
        Some(res.stdout_trimmed())
    } else {
        None
    }
}

// ── Pacman Queries ──────────────────────────────────────────────────

/// Names of every package available (`pacman -Slq`, or AUR helper variant).
pub fn pkg_list_all(aur_helper: Option<&str>) -> Vec<String> {
    let (program, args): (&str, &[&str]) = match aur_helper {
        Some(h) => (h, &["-Slq"]),
        None => ("pacman", &["-Slq"]),
    };
    match capture(program, args) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// Set of installed package names (`pacman -Qq`).
pub fn installed_set() -> HashSet<String> {
    match capture("pacman", &["-Qq"]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => HashSet::new(),
    }
}

/// `pacman -Qi` output for one package, or None.
pub fn qi(pkg: &str) -> Option<String> {
    capture("pacman", &["-Qi", pkg])
}

/// `pacman -Si` output for one or more packages, or None.
pub fn si(pkgs: &[&str]) -> Option<String> {
    if pkgs.is_empty() {
        return None;
    }
    let mut args = vec!["-Si"];
    args.extend_from_slice(pkgs);
    capture("pacman", &args)
}

/// Which of `pkgs` exist in official repos (single -Si call).
pub fn filter_official(pkgs: &[String]) -> (Vec<String>, Vec<String>) {
    let refs: Vec<&str> = pkgs.iter().map(String::as_str).collect();
    let found: HashSet<String> = si(&refs)
        .map(|out| {
            out.lines()
                .filter_map(|l| l.strip_prefix("Name"))
                .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut official = Vec::new();
    let mut aur = Vec::new();
    for p in pkgs {
        if found.contains(p) {
            official.push(p.clone());
        } else {
            aur.push(p.clone());
        }
    }
    (official, aur)
}

/// Search results lines from `pacman -Ss` (repo/name lines only).
pub fn ss(term: &str) -> Vec<String> {
    match capture("pacman", &["-Ss", term]) {
        Some(s) => s.lines().take(20).map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// AUR search result lines starting with `aur/`.
pub fn aur_ss(helper: &str, term: &str) -> Vec<String> {
    match capture(helper, &["-Ss", term]) {
        Some(s) => s
            .lines()
            .filter(|l| l.starts_with("aur/"))
            .take(20)
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}

/// Files owned by an installed package.
pub fn ql_files(pkg: &str) -> Vec<String> {
    match capture("pacman", &["-Ql", pkg]) {
        Some(s) => s
            .lines()
            .filter_map(|l| l.split_once(' ').map(|x| x.1.to_string()))
            .collect(),
        None => Vec::new(),
    }
}

/// `pacman -Qo` — Ok(owner message) or Err(stderr).
pub fn qo(path: &str) -> Result<String, String> {
    let spec = CommandSpec::capture("pacman", &["-Qo", path]);
    match CommandEngine::default().run_capture(&spec) {
        Ok(res) if res.is_success() => Ok(res.stdout_trimmed()),
        Ok(res) => Err(res.stderr_trimmed()),
        Err(e) => Err(e.to_string()),
    }
}

/// Available updates via `checkupdates` (pacman-contrib).
pub fn checkupdates_lines() -> Vec<String> {
    match capture("checkupdates", &[]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// Orphan package names (`pacman -Qtdq`).
pub fn orphan_names() -> Vec<String> {
    match capture("pacman", &["-Qtdq"]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// Explicitly-installed native package names (`pacman -Qqen`).
pub fn explicit_names() -> Vec<String> {
    match capture("pacman", &["-Qqen"]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// Foreign (AUR) package names (`pacman -Qqem`).
pub fn foreign_names() -> Vec<String> {
    match capture("pacman", &["-Qqem"]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

/// Explicitly installed as "name version" display rows.
pub fn explicit_rows() -> Vec<String> {
    match capture("pacman", &["-Qe"]) {
        Some(s) => s
            .lines()
            .map(|l| {
                let mut parts = l.split_whitespace();
                let name = parts.next().unwrap_or("");
                let ver = parts.next().unwrap_or("");
                format!("{name} ({ver})")
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Running pacman PIDs (`pgrep -x pacman`).
pub fn pacman_pids() -> Vec<String> {
    match capture("pgrep", &["-x", "pacman"]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

// ── Log Parsing ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Installed,
    Upgraded,
    Removed,
}

impl LogKind {
    pub fn marker(self) -> (&'static str, &'static str) {
        match self {
            // (marker, semantic color key)
            LogKind::Installed => ("+", "green"),
            LogKind::Upgraded => ("↑", "blue"),
            LogKind::Removed => ("-", "red"),
        }
    }
}

fn read_pacman_log() -> Vec<String> {
    fs::read_to_string(PACMAN_LOG)
        .map(|c| c.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Recent ALPM history entries (installed/upgraded/removed), newest last.
pub fn history_entries(limit: usize) -> Vec<(LogKind, String)> {
    let kinds = ["installed", "upgraded", "removed"];
    let entries: Vec<(LogKind, String)> = read_pacman_log()
        .into_iter()
        .filter(|l| l.contains("[ALPM]") && kinds.iter().any(|k| l.contains(&format!("] {k} "))))
        .map(|l| {
            let kind = if l.contains("] installed ") {
                LogKind::Installed
            } else if l.contains("] upgraded ") {
                LogKind::Upgraded
            } else {
                LogKind::Removed
            };
            (kind, l)
        })
        .collect();
    if entries.len() > limit {
        entries[entries.len() - limit..].to_vec()
    } else {
        entries
    }
}

/// Last `limit` raw log lines matching one grep-like predicate set.
pub fn log_tail(kinds: &[&str], limit: usize) -> Vec<String> {
    let all = read_pacman_log();
    let filtered: Vec<String> = all
        .into_iter()
        .filter(|l| kinds.iter().any(|k| l.contains(k)))
        .collect();
    if filtered.len() > limit {
        filtered[filtered.len() - limit..].to_vec()
    } else {
        filtered
    }
}

// ── Mirrors / Cache / Disk ──────────────────────────────────────────

/// Active server URLs from the mirrorlist.
pub fn mirror_servers() -> Vec<String> {
    fs::read_to_string(MIRRORLIST)
        .map(|c| {
            c.lines()
                .filter_map(|l| l.strip_prefix("Server = "))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn mirror_count() -> usize {
    mirror_servers().len()
}

/// (total bytes, file count) of the pacman package cache.
pub fn cache_stats() -> (Option<u64>, usize) {
    let mut total = 0u64;
    let mut count = 0usize;
    if let Ok(entries) = fs::read_dir(CACHE_DIR) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains(".pkg.tar.") {
                count += 1;
                if let Ok(md) = entry.metadata() {
                    total += md.len();
                }
            }
        }
    }
    (if count > 0 { Some(total) } else { None }, count)
}

/// `df -h /` → "used / size (pct% used)", mirroring the bash formatting.
pub fn disk_usage_root() -> String {
    match capture("df", &["-h", "/"]) {
        Some(out) => out
            .lines()
            .nth(1)
            .and_then(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 5 {
                    Some(format!("{} / {} ({} used)", f[2], f[1], f[4]))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".into()),
        None => "unknown".into(),
    }
}

/// Human-readable size in du -sh style (binary units).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{val:.1}{}", UNITS[unit])
    }
}

/// Lock file metadata: (created timestamp string, size string).
pub fn lockfile_info() -> Option<(String, String)> {
    let md = fs::metadata(LOCK_FILE).ok()?;
    let created = capture("stat", &["-c", "%y", LOCK_FILE])
        .map(|s| s.split('.').next().unwrap_or(&s).to_string())
        .unwrap_or_else(|| "unknown".into());
    Some((created, format!("{} bytes", md.len())))
}

// ── Background Job ──────────────────────────────────────────────────
// Screens spawn potentially-slow queries (AUR network calls, package
// lists) on a thread and poll the result each frame so the UI keeps
// drawing a spinner instead of freezing.

pub struct Job<T> {
    rx: mpsc::Receiver<T>,
    pub label: String,
    pub started: Instant,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(label: impl Into<String>, f: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        Self {
            rx,
            label: label.into(),
            started: Instant::now(),
        }
    }

    /// Returns the job's result once the worker thread finished it.
    pub fn poll(&mut self) -> Option<T> {
        self.rx.try_recv().ok()
    }
}

// ── Misc helpers ────────────────────────────────────────────────────

pub fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| capture("hostname", &[]))
        .unwrap_or_else(|| "unknown".into())
}

pub fn kernel_version() -> String {
    capture("uname", &["-r"]).unwrap_or_else(|| "unknown".into())
}

pub fn arch() -> String {
    capture("uname", &["-m"]).unwrap_or_else(|| "unknown".into())
}

pub fn uptime_pretty() -> String {
    capture("uptime", &["-p"]).unwrap_or_else(|| "unknown".into())
}

pub fn pacman_version() -> String {
    capture("pacman", &["--version"])
        .and_then(|out| {
            out.lines().find(|l| l.contains("Pacman")).and_then(|l| {
                let idx = l.find('v')?;
                let rest = &l[idx..];
                let end = rest.find(' ').unwrap_or(rest.len());
                Some(rest[..end].to_string())
            })
        })
        .unwrap_or_else(|| "unknown".into())
}

pub fn home_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(name)
}

// ── Sidebar system brief ────────────────────────────────────────────

/// Compact CPU/RAM/DISK/GPU summary for the dashboard sidebar.
#[derive(Debug, Clone, Default)]
pub struct SysBrief {
    pub cpu: String,
    pub ram: String,
    pub disk: String,
    pub gpu: String,
}

pub fn sys_brief() -> SysBrief {
    // CPU: first "model name" from /proc/cpuinfo.
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
        })
        .unwrap_or_else(|| "unknown".into());

    // RAM: MemTotal from /proc/meminfo (kB) → GiB.
    let ram = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|c| {
            c.lines().find(|l| l.starts_with("MemTotal")).and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|kb| kb.parse::<u64>().ok())
                    .map(|kb| format!("{:.2} GiB", kb as f64 / 1024.0 / 1024.0))
            })
        })
        .unwrap_or_else(|| "unknown".into());

    // DISK: total size of / in GiB.
    let disk = capture("df", &["-B1", "/"])
        .and_then(|out| {
            out.lines().nth(1).and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|b| b.parse::<u64>().ok())
                    .map(|b| format!("{:.2} GiB", b as f64 / 1024.0 / 1024.0 / 1024.0))
            })
        })
        .unwrap_or_else(|| "unknown".into());

    // GPU: first VGA/3D controller from lspci (optional tool).
    let gpu = capture("lspci", &[])
        .and_then(|out| {
            out.lines()
                .find(|l| l.contains("VGA") || l.contains("3D controller"))
                .and_then(|l| l.split_once(": ").map(|(_, v)| v.to_string()))
        })
        .unwrap_or_else(|| "n/a".into());

    SysBrief {
        cpu,
        ram,
        disk,
        gpu,
    }
}

// ── Native Confirmation and Argument Builders ───────────────────────

/// Returns true if the given arguments represent a pacman transaction that
/// surfaces native package transaction confirmation and accepts `--noconfirm`.
///
/// Operations covered:
/// - `-S` (package installation)
/// - `-R` / `-Rns` / `-Rs` / `-Rdd` / etc. (package removal)
/// - `-U` (local package installation)
/// - `-Syu` / `-Syyu` (system upgrade transactions)
///
/// Operations excluded:
/// - Database refresh without upgrade (`-Sy`, `-Syy`)
/// - Cache cleaning (`-Sc`, `-Scc`)
/// - Queries (`-Q*`, `-Ss`, `-Si`, `-Sl`, etc.)
/// - Commands with `--print`
pub fn supports_noconfirm<S: AsRef<str>>(args: &[S]) -> bool {
    let mut is_tx = false;
    let mut is_excluded = false;

    for arg in args {
        let s = arg.as_ref();
        if s == "--print" {
            is_excluded = true;
        }
        if s.starts_with('-') && !s.starts_with("--") {
            let chars = &s[1..];
            if chars.starts_with('Q') {
                return false;
            }
            if chars.starts_with('D') || chars.starts_with('F') || chars.starts_with('T') {
                return false;
            }
            if chars.starts_with('R') || chars.starts_with('U') {
                is_tx = true;
            }
            if chars.starts_with('S') {
                if chars.contains('s')
                    || chars.contains('i')
                    || chars.contains('l')
                    || chars.contains('c')
                    || (chars.contains('y') && !chars.contains('u'))
                {
                    is_excluded = true;
                } else {
                    is_tx = true;
                }
            }
        }
    }

    is_tx && !is_excluded
}

/// Construct the argument vector for pacman.
///
/// Does NOT include the "pacman" executable prefix.
/// If NATIVE_CONFIRM is false and the command supports --noconfirm, appends --noconfirm.
/// Ensures --noconfirm is never duplicated.
pub fn pacman_args<S: AsRef<str>>(settings: &crate::settings::Settings, args: &[S]) -> Vec<String> {
    let mut out: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();
    if !settings.is_true("NATIVE_CONFIRM")
        && supports_noconfirm(args)
        && !out.iter().any(|a| a == "--noconfirm")
    {
        out.push("--noconfirm".to_string());
    }
    out
}

/// Convenience helper for `sudo pacman` command invocations.
/// Prepends `"pacman"` to the argument list returned by [`pacman_args`].
pub fn sudo_pacman_args<S: AsRef<str>>(
    settings: &crate::settings::Settings,
    args: &[S],
) -> Vec<String> {
    let mut out = vec!["pacman".to_string()];
    out.extend(pacman_args(settings, args));
    out
}

/// Construct the argument vector for AUR helpers (e.g. yay/paru).
///
/// Does NOT include the helper executable prefix.
/// If NATIVE_CONFIRM is false and the command supports --noconfirm, appends --noconfirm.
/// Ensures --noconfirm is never duplicated.
pub fn aur_args<S: AsRef<str>>(settings: &crate::settings::Settings, args: &[S]) -> Vec<String> {
    let mut out: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();
    if !settings.is_true("NATIVE_CONFIRM")
        && supports_noconfirm(args)
        && !out.iter().any(|a| a == "--noconfirm")
    {
        out.push("--noconfirm".to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn test_supports_noconfirm() {
        // Transactions that prompt confirmation
        assert!(supports_noconfirm(&["-S", "foo"]));
        assert!(supports_noconfirm(&["-Syu"]));
        assert!(supports_noconfirm(&["-Syyu"]));
        assert!(supports_noconfirm(&["-Rns", "foo"]));
        assert!(supports_noconfirm(&["-R", "foo"]));
        assert!(supports_noconfirm(&["-Rs", "foo"]));
        assert!(supports_noconfirm(&["-Rdd", "foo"]));
        assert!(supports_noconfirm(&["-U", "pkg.pkg.tar.zst"]));

        // Non-transactions / excluded operations
        assert!(!supports_noconfirm(&["-Sy"]));
        assert!(!supports_noconfirm(&["-Syy"]));
        assert!(!supports_noconfirm(&["-Sc"]));
        assert!(!supports_noconfirm(&["-Scc"]));
        assert!(!supports_noconfirm(&["-Qi", "foo"]));
        assert!(!supports_noconfirm(&["-Qs", "foo"]));
        assert!(!supports_noconfirm(&["-Ss", "foo"]));
        assert!(!supports_noconfirm(&["-Si", "foo"]));
        assert!(!supports_noconfirm(&["-Sl"]));
        assert!(!supports_noconfirm(&["-Slq"]));
        assert!(!supports_noconfirm(&["-S", "foo", "--print"]));
    }

    #[test]
    fn test_pacman_args_vectors() {
        let mut settings = Settings::default();
        // Default: NATIVE_CONFIRM = true
        assert_eq!(pacman_args(&settings, &["-S", "foo"]), vec!["-S", "foo"]);
        assert_eq!(pacman_args(&settings, &["-Syu"]), vec!["-Syu"]);
        assert_eq!(
            pacman_args(&settings, &["-Rns", "foo"]),
            vec!["-Rns", "foo"]
        );

        // NATIVE_CONFIRM = false
        settings.set("NATIVE_CONFIRM", "false");
        assert_eq!(
            pacman_args(&settings, &["-S", "foo"]),
            vec!["-S", "foo", "--noconfirm"]
        );
        assert_eq!(
            pacman_args(&settings, &["-Syu"]),
            vec!["-Syu", "--noconfirm"]
        );
        assert_eq!(
            pacman_args(&settings, &["-Rns", "foo"]),
            vec!["-Rns", "foo", "--noconfirm"]
        );
        assert_eq!(
            pacman_args(&settings, &["-U", "pkg.pkg.tar.zst"]),
            vec!["-U", "pkg.pkg.tar.zst", "--noconfirm"]
        );
        // Excluded:
        assert_eq!(pacman_args(&settings, &["-Qi", "foo"]), vec!["-Qi", "foo"]);
        assert_eq!(pacman_args(&settings, &["-Sy"]), vec!["-Sy"]);
        assert_eq!(pacman_args(&settings, &["-Syy"]), vec!["-Syy"]);
        assert_eq!(pacman_args(&settings, &["-Sc"]), vec!["-Sc"]);
        assert_eq!(pacman_args(&settings, &["-Scc"]), vec!["-Scc"]);
        assert_eq!(
            pacman_args(&settings, &["-S", "foo", "--print"]),
            vec!["-S", "foo", "--print"]
        );

        // Never duplicate --noconfirm
        assert_eq!(
            pacman_args(&settings, &["-S", "foo", "--noconfirm"]),
            vec!["-S", "foo", "--noconfirm"]
        );
    }

    #[test]
    fn test_sudo_pacman_args() {
        let mut settings = Settings::default();
        assert_eq!(
            sudo_pacman_args(&settings, &["-S", "foo"]),
            vec!["pacman", "-S", "foo"]
        );

        settings.set("NATIVE_CONFIRM", "false");
        assert_eq!(
            sudo_pacman_args(&settings, &["-S", "foo"]),
            vec!["pacman", "-S", "foo", "--noconfirm"]
        );
    }

    #[test]
    fn test_aur_args() {
        let mut settings = Settings::default();
        assert_eq!(aur_args(&settings, &["-S", "foo"]), vec!["-S", "foo"]);

        settings.set("NATIVE_CONFIRM", "false");
        assert_eq!(
            aur_args(&settings, &["-S", "foo"]),
            vec!["-S", "foo", "--noconfirm"]
        );
        assert_eq!(
            aur_args(&settings, &["-S", "foo", "--noconfirm"]),
            vec!["-S", "foo", "--noconfirm"]
        );
    }
}
