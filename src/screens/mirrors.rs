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

const MENU_ITEMS: [&str; 8] = [
    "🔄 Auto-update mirrors (reflector)",
    "🏎  Rank fastest mirrors",
    "📋 Show current mirrors",
    "💾 Backup current mirrorlist",
    "♻  Restore mirrorlist from backup",
    "➕ Add repositories",
    "➖ Remove repositories",
    "🔙 Back to Main Menu",
];

/// What an open modal refers to.
enum Await {
    InstallReflector,
    RestoreBackup,
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

/// Remove Chaotic-AUR repository, mirrorlist, keyrings, and pacman.conf configuration.
pub fn remove_chaotic_aur(app: &mut App) {
    app.log("REPO: Removing Chaotic-AUR repository");
    let script = r#"set -e
echo "==> [1/4] Removing [chaotic-aur] configuration from /etc/pacman.conf..."
if grep -q "^\s*\[chaotic-aur\]" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[chaotic-aur\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/chaotic-mirrorlist/d' /etc/pacman.conf
    sed -i '/^\s*\[chaotic-aur\]/d' /etc/pacman.conf
    echo "    Removed [chaotic-aur] from /etc/pacman.conf"
fi

echo "==> [2/4] Removing chaotic-keyring and chaotic-mirrorlist..."
pacman -Rdd --noconfirm chaotic-keyring chaotic-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/chaotic-mirrorlist
rm -f /var/lib/pacman/sync/chaotic-aur.*

echo "==> [3/4] Removing Chaotic-AUR GPG keys from pacman keyring..."
pacman-key --delete FBA220DFC880C036 3056513887B78AEB 2>/dev/null || true

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> Chaotic-AUR repository removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-chaotic-aur-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove Chaotic-AUR repository")
        .result(
            "Chaotic-AUR repository removed!",
            "Failed to remove Chaotic-AUR repository.",
            "REPO: Chaotic-AUR repository removed",
        ),
    );
}

/// Remove CachyOS repositories, mirrorlists, keyrings, and pacman.conf configuration.
pub fn remove_cachyos(app: &mut App) {
    app.log("REPO: Removing CachyOS repositories");
    let script = r#"set -e
echo "==> [1/4] Preparing workspace..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/4] Downloading official CachyOS repository uninstaller..."
if curl -sSL https://mirror.cachyos.org/cachyos-repo.tar.xz -o "$tmp/cachyos-repo.tar.xz"; then
    tar -xf "$tmp/cachyos-repo.tar.xz" -C "$tmp"
    echo "==> Running CachyOS uninstaller..."
    (cd "$tmp/cachyos-repo" && ./cachyos-repo.sh --remove) || true
fi

echo "==> [3/4] Cleaning CachyOS configurations and packages..."
if grep -q "cachyos" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[cachyos.*\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/cachyos.*mirrorlist/d' /etc/pacman.conf
    sed -i '/cachyos/d' /etc/pacman.conf
fi
pacman -Rdd --noconfirm cachyos-keyring cachyos-mirrorlist cachyos-v3-mirrorlist cachyos-v4-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/cachyos-mirrorlist* /etc/pacman.d/cachyos-v*-mirrorlist*
rm -f /var/lib/pacman/sync/cachyos*

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> CachyOS repositories removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-cachyos-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove CachyOS repositories")
        .result(
            "CachyOS repositories removed!",
            "Failed to remove CachyOS repositories.",
            "REPO: CachyOS repositories removed",
        ),
    );
}

/// Remove BlackArch penetration testing repository and pacman.conf configuration.
pub fn remove_blackarch(app: &mut App) {
    app.log("REPO: Removing BlackArch repository");
    let script = r#"set -e
echo "==> [1/3] Removing BlackArch entries from /etc/pacman.conf..."
if grep -q "^\s*\[blackarch\]" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[blackarch\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/blackarch-mirrorlist/d' /etc/pacman.conf
    sed -i '/^\s*\[blackarch\]/d' /etc/pacman.conf
    echo "    Removed [blackarch] from /etc/pacman.conf"
fi

echo "==> [2/3] Removing BlackArch keyring and mirrorlist..."
pacman -Rdd --noconfirm blackarch-keyring blackarch-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/blackarch-mirrorlist
rm -f /var/lib/pacman/sync/blackarch.*

echo "==> [3/3] Refreshing package databases..."
pacman -Sy
echo "==> BlackArch repository removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-blackarch-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove BlackArch repository")
        .result(
            "BlackArch repository removed!",
            "Failed to remove BlackArch repository.",
            "REPO: BlackArch repository removed",
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
                    app.push(Box::new(super::repos::ReposScreen::new(
                        app,
                        super::repos::RepoMode::Add,
                    )));
                }
                6 => {
                    app.push(Box::new(super::repos::ReposScreen::new(
                        app,
                        super::repos::RepoMode::Remove,
                    )));
                }
                _ => app.pop(),
            },
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        self.chaotic_aur_enabled = sys::is_chaotic_aur_enabled();
        self.cachyos_enabled = sys::is_cachyos_repo_enabled();
        self.blackarch_enabled = sys::is_blackarch_enabled();
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
            "mirrors-chaotic-aur-remove" | "run-remove-chaotic-aur" => {
                app.refresh_caps();
                self.chaotic_aur_enabled = app.caps().chaotic_aur;
                if ok {
                    app.toast("Chaotic-AUR repository removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove Chaotic-AUR repository.", Sev::Error);
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
            "mirrors-cachyos-remove" | "run-remove-cachyos-repo" => {
                app.refresh_caps();
                self.cachyos_enabled = app.caps().cachyos;
                if ok {
                    app.toast("CachyOS repositories removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove CachyOS repositories.", Sev::Error);
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
            "mirrors-blackarch-remove" | "run-remove-blackarch" => {
                app.refresh_caps();
                self.blackarch_enabled = app.caps().blackarch;
                if ok {
                    app.toast("BlackArch repository removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove BlackArch repository.", Sev::Error);
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
        assert_eq!(MENU_ITEMS.len(), 8);
        assert!(MENU_ITEMS[5].contains("Add repositories"));
        assert!(MENU_ITEMS[6].contains("Remove repositories"));
    }

    #[test]
    fn test_mirrors_screen_creation() {
        let app = App::new();
        let screen = MirrorsScreen::new(&app);
        assert_eq!(screen.menu.items.len(), 8);
    }
}
