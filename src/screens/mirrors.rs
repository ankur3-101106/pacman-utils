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

const MENU_ITEMS: [&str; 6] = [
    "🔄 Auto-update mirrors (reflector)",
    "🏎  Rank fastest mirrors",
    "📋 Show current mirrors",
    "💾 Backup current mirrorlist",
    "♻  Restore mirrorlist from backup",
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
    /// Pending country prompt target.
    country_for: Option<ReflectorMode>,
    await_kind: Option<Await>,
}

impl MirrorsScreen {
    pub fn new(app: &App) -> Self {
        Self {
            menu: Menu::new("Mirror actions", MENU_ITEMS.map(str::to_string).to_vec()),
            reflector_installed: app.caps().reflector,
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
                _ => app.pop(),
            },
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let count = sys::mirror_count();
        let rows = Layout::vertical([Constraint::Length(7), Constraint::Fill(1)]).split(area);
        kv_table(
            f,
            rows[0],
            "🌍  Mirror Management",
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
            _ => {}
        }
    }
}
