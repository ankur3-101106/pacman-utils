// ──────────────────────────────────────────────────────────────────────
// mirrors.rs — Mirror management (lib/mirrors.sh)
//
// reflector integration with automatic backup, plus manual backup and
// restore of /etc/pacman.d/mirrorlist.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Span,
    Frame,
};

use crate::app::{App, ExtCmd, Screen};
use crate::sys::{self, MIRRORLIST, MIRRORLIST_BACKUP};
use crate::widgets::{self, kv_table, Menu, Sev};

use super::{args, viewer::ViewerScreen};

const MENU_ITEMS: [&str; 9] = [
    "🔄 Auto-update mirrors (reflector)",
    "🏎  Rank fastest mirrors",
    "📋 Show current mirrors",
    "💾 Backup current mirrorlist",
    "♻  Restore mirrorlist from backup",
    "🌀 Enable Chaotic-AUR repository",
    "⚡ Enable CachyOS repositories",
    "🏴 Enable BlackArch repository",
    "🔙 Back to Main Menu",
];

/// What an open modal refers to.
enum Await {
    InstallReflector,
    RestoreBackup,
    EnableChaoticAur,
    EnableCachyos,
    EnableBlackArch,
}

#[derive(Clone, Copy)]
enum ReflectorMode {
    Auto,
    Rank,
}

impl ReflectorMode {
    fn tag(self) -> &'static str {
        match self {
            ReflectorMode::Auto => "mirrors-auto",
            ReflectorMode::Rank => "mirrors-rank",
        }
    }
    fn latest(self) -> &'static str {
        match self {
            ReflectorMode::Auto => "20",
            ReflectorMode::Rank => "10",
        }
    }
}

pub struct MirrorsScreen {
    menu: Menu,
    /// Snapshot for the stats table; refreshed when reflector is installed.
    reflector_installed: bool,
    chaotic_aur_enabled: bool,
    cachyos_enabled: bool,
    blackarch_enabled: bool,
    /// Pending country prompt target.
    country_for: Option<ReflectorMode>,
    await_kind: Option<Await>,
}

impl MirrorsScreen {
    pub fn new(app: &App) -> Self {
        let caps = app.caps();
        Self {
            menu: Menu::new(
                "Mirror & Repository actions",
                MENU_ITEMS.map(str::to_string).to_vec(),
            ),
            reflector_installed: caps.reflector,
            chaotic_aur_enabled: caps.chaotic_aur,
            cachyos_enabled: caps.cachyos,
            blackarch_enabled: caps.blackarch,
            country_for: None,
            await_kind: None,
        }
    }

    /// Queue reflector run preceded by a silent backup, like the bash flow.
    fn run_reflector(&mut self, app: &mut App, mode: ReflectorMode, country: Option<String>) {
        app.log(&format!(
            "MIRRORS: {}",
            match mode {
                ReflectorMode::Auto => "Auto-updating mirrors with reflector",
                ReflectorMode::Rank => "Ranking fastest mirrors",
            }
        ));

        // Silent safety backup first (bash: sudo cp before running).
        app.queue_ext(
            ExtCmd::new(
                "mirrors-pre-backup",
                "sudo",
                &args(&["cp", MIRRORLIST, MIRRORLIST_BACKUP]),
            )
            .note("Backing up current mirrorlist"),
        );

        let mut cmd_args: Vec<String> = vec!["reflector".into()];
        if let Some(c) = country.clone() {
            cmd_args.push("--country".into());
            cmd_args.push(c);
        }
        cmd_args.push("--latest".into());
        cmd_args.push(mode.latest().into());
        cmd_args.push("--sort".into());
        cmd_args.push("rate".into());
        cmd_args.push("--save".into());
        cmd_args.push(MIRRORLIST.into());

        if let Some(c) = &country {
            app.toast(
                format!(
                    "Using: --country '{c}' --latest {} --sort rate",
                    mode.latest()
                ),
                Sev::Info,
            );
        } else {
            app.toast(
                format!("Using: --latest {} --sort rate", mode.latest()),
                Sev::Info,
            );
        }

        app.queue_ext(
            ExtCmd::new(mode.tag(), "sudo", &cmd_args)
                .note(match mode {
                    ReflectorMode::Auto => "Fetching and ranking mirrors...",
                    ReflectorMode::Rank => "Testing mirror speeds...",
                })
                .result(
                    "Mirrorlist updated!",
                    "reflector failed. Restoring backup...",
                    "MIRRORS: reflector run finished",
                ),
        );
    }

    fn execute_install_reflector(&self, app: &mut App) {
        let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-S", "reflector"]);
        app.queue_ext(
            ExtCmd::new("mirrors-dep", "sudo", &cmd_args)
                .note("Install reflector")
                .result(
                    "reflector installed.",
                    "Failed to install reflector.",
                    "MIRRORS: reflector install finished",
                ),
        );
    }

    fn execute_restore_backup(&self, app: &mut App) {
        app.log("MIRRORS: Restoring mirrorlist from backup");
        app.queue_ext(
            ExtCmd::new(
                "mirrors-restore",
                "sudo",
                &args(&["cp", MIRRORLIST_BACKUP, MIRRORLIST]),
            )
            .note("Restore mirrorlist from backup")
            .result(
                "Mirrorlist restored!",
                "Restore failed.",
                "MIRRORS: restore finished",
            ),
        );
    }
}

/// Enable Chaotic-AUR repository by importing keys, installing keyring/mirrorlist,
/// configuring /etc/pacman.conf, and refreshing databases.
pub fn enable_chaotic_aur(app: &mut App) {
    app.log("REPO: Enabling Chaotic-AUR repository");
    let script = r#"set -e
echo "==> [1/4] Receiving and signing Chaotic-AUR GPG keys..."
pacman-key --recv-key FBA220DFC880C036 --keyserver keyserver.ubuntu.com || pacman-key --recv-key FBA220DFC880C036 --keyserver hkp://keyserver.ubuntu.com:80
pacman-key --lsign-key FBA220DFC880C036
pacman-key --recv-key 3056513887B78AEB --keyserver keyserver.ubuntu.com 2>/dev/null && pacman-key --lsign-key 3056513887B78AEB 2>/dev/null || true

echo "==> [2/4] Installing chaotic-keyring and chaotic-mirrorlist..."
pacman -U --noconfirm 'https://cdn-mirror.chaotic.cx/chaotic-aur/chaotic-keyring.pkg.tar.zst' 'https://cdn-mirror.chaotic.cx/chaotic-aur/chaotic-mirrorlist.pkg.tar.zst'

echo "==> [3/4] Configuring /etc/pacman.conf..."
if grep -q "^\s*\[chaotic-aur\]" /etc/pacman.conf; then
    echo "    [chaotic-aur] already configured in /etc/pacman.conf"
else
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    printf '\n[chaotic-aur]\nInclude = /etc/pacman.d/chaotic-mirrorlist\n' >> /etc/pacman.conf
    echo "    Added [chaotic-aur] repository to /etc/pacman.conf"
fi

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> Chaotic-AUR repository enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-chaotic-aur",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Enable Chaotic-AUR repository")
        .result(
            "Chaotic-AUR repository enabled!",
            "Failed to enable Chaotic-AUR repository.",
            "REPO: Chaotic-AUR repository enabled",
        ),
    );
}

/// Enable CachyOS repositories by downloading official installer script,
/// auto-detecting CPU microarchitecture, and configuring optimized repositories.
pub fn enable_cachyos(app: &mut App) {
    app.log("REPO: Enabling CachyOS repositories");
    let script = r#"set -e
echo "==> [1/4] Preparing temporary workspace..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/4] Downloading official CachyOS repository installer..."
curl -sSL https://mirror.cachyos.org/cachyos-repo.tar.xz -o "$tmp/cachyos-repo.tar.xz"
tar -xf "$tmp/cachyos-repo.tar.xz" -C "$tmp"

echo "==> [3/4] Running CachyOS repository setup and CPU instruction set detection..."
(cd "$tmp/cachyos-repo" && ./cachyos-repo.sh --install)

echo "==> [4/4] Refreshing package databases with CachyOS repositories..."
pacman -Sy
echo "==> CachyOS repositories enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new("mirrors-cachyos", "sudo", &args(&["bash", "-c", script]))
            .note("Enable CachyOS repositories")
            .result(
                "CachyOS repositories enabled!",
                "Failed to enable CachyOS repositories.",
                "REPO: CachyOS repositories enabled",
            ),
    );
}

/// Enable BlackArch penetration testing repository via official strap.sh.
pub fn enable_blackarch(app: &mut App) {
    app.log("REPO: Enabling BlackArch repository");
    let script = r#"set -e
echo "==> [1/3] Preparing workspace for BlackArch setup..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/3] Downloading official BlackArch bootstrap script (strap.sh)..."
curl -sSL https://blackarch.org/strap.sh -o "$tmp/strap.sh"
chmod +x "$tmp/strap.sh"

echo "==> [3/3] Running BlackArch repository setup..."
(cd "$tmp" && ./strap.sh)

echo "==> Refreshing package databases..."
pacman -Sy
echo "==> BlackArch repository enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new("mirrors-blackarch", "sudo", &args(&["bash", "-c", script]))
            .note("Enable BlackArch repository")
            .result(
                "BlackArch repository enabled!",
                "Failed to enable BlackArch repository.",
                "REPO: BlackArch repository enabled",
            ),
    );
}

impl Screen for MirrorsScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => match self.menu.selected {
                0 | 1 => {
                    let mode = if self.menu.selected == 0 {
                        ReflectorMode::Auto
                    } else {
                        ReflectorMode::Rank
                    };
                    if !app.caps().reflector {
                        app.toast("reflector is not installed.", Sev::Warn);
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm("Install reflector?", false);
                            self.await_kind = Some(Await::InstallReflector);
                        } else {
                            self.execute_install_reflector(app);
                        }
                    } else {
                        self.country_for = Some(mode);
                        app.ask_input("Country name or code (leave blank for global)");
                    }
                }
                2 => {
                    let servers = sys::mirror_servers();
                    if servers.is_empty() {
                        app.toast(
                            format!("No active mirrors found in {MIRRORLIST}"),
                            Sev::Warn,
                        );
                    } else {
                        let lines: Vec<Vec<Span>> = servers
                            .into_iter()
                            .enumerate()
                            .map(|(i, m)| {
                                vec![
                                    widgets::span(
                                        format!("{:>3}. ", i + 1),
                                        widgets::accent_bold(),
                                    ),
                                    Span::styled(m, Style::new()),
                                ]
                            })
                            .collect();
                        app.push(ViewerScreen::new("📋 Current Mirrors", lines));
                    }
                }
                3 => {
                    app.log("MIRRORS: Backing up mirrorlist");
                    app.queue_ext(
                        ExtCmd::new(
                            "mirrors-backup",
                            "sudo",
                            &args(&["cp", MIRRORLIST, MIRRORLIST_BACKUP]),
                        )
                        .note("Backup current mirrorlist")
                        .result(
                            "Mirrorlist backed up.",
                            "Backup failed.",
                            "MIRRORS: backup finished",
                        ),
                    );
                }
                4 => {
                    if std::path::Path::new(MIRRORLIST_BACKUP).exists() {
                        let date = crate::sys::capture("stat", &["-c", "%y", MIRRORLIST_BACKUP])
                            .map(|s| s.split('.').next().unwrap_or(&s).to_string())
                            .unwrap_or_else(|| "unknown".into());
                        app.toast(format!("Backup found: {MIRRORLIST_BACKUP}"), Sev::Info);
                        app.toast(format!("Created: {date}"), Sev::Info);
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm("Restore mirrorlist from backup?", false);
                            self.await_kind = Some(Await::RestoreBackup);
                        } else {
                            self.execute_restore_backup(app);
                        }
                    } else {
                        app.toast(
                            format!("No backup found at {MIRRORLIST_BACKUP}"),
                            Sev::Error,
                        );
                    }
                }
                5 => {
                    if app.settings().is_true("CONFIRM_ACTIONS") {
                        app.confirm(
                            "Enable Chaotic-AUR repository (automated pre-built AUR packages)?",
                            false,
                        );
                        self.await_kind = Some(Await::EnableChaoticAur);
                    } else {
                        enable_chaotic_aur(app);
                    }
                }
                6 => {
                    if app.settings().is_true("CONFIRM_ACTIONS") {
                        app.confirm(
                            "Enable CachyOS repositories (CPU-optimized x86-64-v3/v4/zen4 packages)?",
                            false,
                        );
                        self.await_kind = Some(Await::EnableCachyos);
                    } else {
                        enable_cachyos(app);
                    }
                }
                7 => {
                    if app.settings().is_true("CONFIRM_ACTIONS") {
                        app.confirm("Enable BlackArch penetration testing repository?", false);
                        self.await_kind = Some(Await::EnableBlackArch);
                    } else {
                        enable_blackarch(app);
                    }
                }
                _ => app.pop(),
            },
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let count = sys::mirror_count();
        let rows = Layout::vertical([Constraint::Length(10), Constraint::Fill(1)]).split(area);
        kv_table(
            f,
            rows[0],
            "🌍  Mirrors & Repositories",
            &[
                ("Mirrorlist", MIRRORLIST.to_string()),
                ("Active Mirrors", count.to_string()),
                (
                    "Reflector",
                    if self.reflector_installed {
                        "installed"
                    } else {
                        "not installed"
                    }
                    .to_string(),
                ),
                (
                    "Chaotic-AUR",
                    if self.chaotic_aur_enabled {
                        "enabled"
                    } else {
                        "not enabled"
                    }
                    .to_string(),
                ),
                (
                    "CachyOS Repos",
                    if self.cachyos_enabled {
                        "enabled"
                    } else {
                        "not enabled"
                    }
                    .to_string(),
                ),
                (
                    "BlackArch",
                    if self.blackarch_enabled {
                        "enabled"
                    } else {
                        "not enabled"
                    }
                    .to_string(),
                ),
            ],
        );
        self.menu.render(f, rows[1]);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓ navigate", "enter select", "esc back"]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(await_kind) = self.await_kind.take() else {
            return;
        };
        match await_kind {
            Await::InstallReflector => {
                if !yes {
                    return;
                }
                self.execute_install_reflector(app);
            }
            Await::RestoreBackup => {
                if !yes {
                    return;
                }
                self.execute_restore_backup(app);
            }
            Await::EnableChaoticAur => {
                if !yes {
                    return;
                }
                enable_chaotic_aur(app);
            }
            Await::EnableCachyos => {
                if !yes {
                    return;
                }
                enable_cachyos(app);
            }
            Await::EnableBlackArch => {
                if !yes {
                    return;
                }
                enable_blackarch(app);
            }
        }
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let Some(mode) = self.country_for.take() else {
            return;
        };
        let country = value.trim().to_string();
        self.run_reflector(
            app,
            mode,
            if country.is_empty() {
                None
            } else {
                Some(country)
            },
        );
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        match tag {
            "mirrors-dep" => {
                app.refresh_caps();
                self.reflector_installed = app.caps().reflector;
                if self.reflector_installed {
                    app.toast("reflector installed.", Sev::Success);
                } else {
                    app.toast("Failed to install reflector.", Sev::Error);
                }
            }
            "mirrors-auto" | "mirrors-rank" => {
                if ok {
                    let count = sys::mirror_count();
                    let msg = if tag == "mirrors-auto" {
                        format!("Mirrorlist updated! ({count} mirrors)")
                    } else {
                        "Mirrorlist updated with fastest mirrors!".to_string()
                    };
                    app.toast(msg, Sev::Success);
                    app.log(&format!("MIRRORS: Updated to {count} mirrors"));
                } else {
                    app.toast("reflector failed. Restoring backup...", Sev::Error);
                    app.log("MIRRORS: reflector failed, backup restored");
                    if std::path::Path::new(MIRRORLIST_BACKUP).exists() {
                        app.queue_ext(
                            ExtCmd::new(
                                "mirrors-fallback",
                                "sudo",
                                &args(&["cp", MIRRORLIST_BACKUP, MIRRORLIST]),
                            )
                            .note("Restore mirrorlist backup"),
                        );
                    }
                }
            }
            "mirrors-backup" => {
                if ok {
                    app.toast(
                        format!("Mirrorlist backed up to {MIRRORLIST_BACKUP}"),
                        Sev::Success,
                    );
                    app.log("MIRRORS: Backup successful");
                } else {
                    app.toast("Backup failed.", Sev::Error);
                    app.log("MIRRORS: Backup failed");
                }
            }
            "mirrors-restore" => {
                if ok {
                    app.toast("Mirrorlist restored!", Sev::Success);
                    app.log("MIRRORS: Mirrorlist restored");
                } else {
                    app.toast("Restore failed.", Sev::Error);
                    app.log("MIRRORS: Restore failed");
                }
            }
            "mirrors-chaotic-aur" | "run-enable-chaotic-aur" => {
                app.refresh_caps();
                self.chaotic_aur_enabled = app.caps().chaotic_aur;
                if ok {
                    app.toast("Chaotic-AUR repository enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable Chaotic-AUR repository.", Sev::Error);
                }
            }
            "mirrors-cachyos" | "run-enable-cachyos-repo" => {
                app.refresh_caps();
                self.cachyos_enabled = app.caps().cachyos;
                if ok {
                    app.toast("CachyOS repositories enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable CachyOS repositories.", Sev::Error);
                }
            }
            "mirrors-blackarch" | "run-enable-blackarch" => {
                app.refresh_caps();
                self.blackarch_enabled = app.caps().blackarch;
                if ok {
                    app.toast("BlackArch repository enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable BlackArch repository.", Sev::Error);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirrors_menu_items() {
        assert_eq!(MENU_ITEMS.len(), 9);
        assert!(MENU_ITEMS[5].contains("Chaotic-AUR"));
        assert!(MENU_ITEMS[6].contains("CachyOS"));
        assert!(MENU_ITEMS[7].contains("BlackArch"));
    }

    #[test]
    fn test_mirrors_screen_creation() {
        let app = App::new();
        let screen = MirrorsScreen::new(&app);
        assert_eq!(screen.menu.items.len(), 9);
    }
}
