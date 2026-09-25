// ──────────────────────────────────────────────────────────────────────
// repos.rs — Third-party repository management (Chaotic-AUR, CachyOS, BlackArch)
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};

use crate::app::{App, Screen};
use crate::sys;
use crate::widgets::{kv_table, Menu, Sev};

use super::mirrors::{
    enable_blackarch, enable_cachyos, enable_chaotic_aur, remove_blackarch, remove_cachyos,
    remove_chaotic_aur,
};

const ADD_MENU_ITEMS: [&str; 4] = [
    "🌀 Enable Chaotic-AUR (Automated pre-built AUR binary repository)",
    "⚡ Enable CachyOS (CPU-optimized x86-64-v3/v4/zen4 packages)",
    "🏴 Enable BlackArch (Penetration testing & security repository)",
    "🔙 Back",
];

const REMOVE_MENU_ITEMS: [&str; 4] = [
    "🌀 Remove Chaotic-AUR repository",
    "⚡ Remove CachyOS repositories",
    "🏴 Remove BlackArch repository",
    "🔙 Back",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RepoMode {
    Add,
    Remove,
}

enum RepoAwait {
    ChaoticAur,
    Cachyos,
    BlackArch,
}

pub struct ReposScreen {
    mode: RepoMode,
    menu: Menu,
    chaotic_aur_enabled: bool,
    cachyos_enabled: bool,
    blackarch_enabled: bool,
    await_kind: Option<RepoAwait>,
}

impl ReposScreen {
    pub fn new(app: &App, mode: RepoMode) -> Self {
        let caps = app.caps();
        let title = match mode {
            RepoMode::Add => "Add Third-Party Repositories",
            RepoMode::Remove => "Remove Third-Party Repositories",
        };
        let items = match mode {
            RepoMode::Add => ADD_MENU_ITEMS.map(str::to_string).to_vec(),
            RepoMode::Remove => REMOVE_MENU_ITEMS.map(str::to_string).to_vec(),
        };
        Self {
            mode,
            menu: Menu::new(title, items),
            chaotic_aur_enabled: caps.chaotic_aur,
            cachyos_enabled: caps.cachyos,
            blackarch_enabled: caps.blackarch,
            await_kind: None,
        }
    }
}

impl Screen for ReposScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => match self.menu.selected {
                0 => match self.mode {
                    RepoMode::Add => {
                        if self.chaotic_aur_enabled {
                            app.toast(
                                "Chaotic-AUR is already enabled. Refreshing/re-enabling...",
                                Sev::Info,
                            );
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(
                                "Enable Chaotic-AUR repository (automated pre-built AUR packages)?",
                                false,
                            );
                            self.await_kind = Some(RepoAwait::ChaoticAur);
                        } else {
                            enable_chaotic_aur(app);
                        }
                    }
                    RepoMode::Remove => {
                        if !self.chaotic_aur_enabled {
                            app.toast("Chaotic-AUR is not currently enabled.", Sev::Warn);
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(
                                "Remove Chaotic-AUR repository, mirrorlist, and keyrings?",
                                false,
                            );
                            self.await_kind = Some(RepoAwait::ChaoticAur);
                        } else {
                            remove_chaotic_aur(app);
                        }
                    }
                },
                1 => match self.mode {
                    RepoMode::Add => {
                        if self.cachyos_enabled {
                            app.toast(
                                "CachyOS is already enabled. Refreshing/re-enabling...",
                                Sev::Info,
                            );
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(
                                "Enable CachyOS repositories (CPU-optimized x86-64-v3/v4/zen4 packages)?",
                                false,
                            );
                            self.await_kind = Some(RepoAwait::Cachyos);
                        } else {
                            enable_cachyos(app);
                        }
                    }
                    RepoMode::Remove => {
                        if !self.cachyos_enabled {
                            app.toast("CachyOS repositories are not currently enabled.", Sev::Warn);
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(
                                "Remove CachyOS repositories, mirrorlist, and keyrings?",
                                false,
                            );
                            self.await_kind = Some(RepoAwait::Cachyos);
                        } else {
                            remove_cachyos(app);
                        }
                    }
                },
                2 => match self.mode {
                    RepoMode::Add => {
                        if self.blackarch_enabled {
                            app.toast(
                                "BlackArch is already enabled. Refreshing/re-enabling...",
                                Sev::Info,
                            );
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm("Enable BlackArch penetration testing repository?", false);
                            self.await_kind = Some(RepoAwait::BlackArch);
                        } else {
                            enable_blackarch(app);
                        }
                    }
                    RepoMode::Remove => {
                        if !self.blackarch_enabled {
                            app.toast("BlackArch repository is not currently enabled.", Sev::Warn);
                        }
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(
                                "Remove BlackArch repository, mirrorlist, and keyrings?",
                                false,
                            );
                            self.await_kind = Some(RepoAwait::BlackArch);
                        } else {
                            remove_blackarch(app);
                        }
                    }
                },
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

        let rows = Layout::vertical([Constraint::Length(8), Constraint::Fill(1)]).split(area);
        let title = match self.mode {
            RepoMode::Add => "➕  Add Third-Party Repositories",
            RepoMode::Remove => "➖  Remove Third-Party Repositories",
        };
        kv_table(
            f,
            rows[0],
            title,
            &[
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
                ("Pacman Config", "/etc/pacman.conf".to_string()),
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
        if !yes {
            return;
        }
        match (self.mode, await_kind) {
            (RepoMode::Add, RepoAwait::ChaoticAur) => enable_chaotic_aur(app),
            (RepoMode::Add, RepoAwait::Cachyos) => enable_cachyos(app),
            (RepoMode::Add, RepoAwait::BlackArch) => enable_blackarch(app),
            (RepoMode::Remove, RepoAwait::ChaoticAur) => remove_chaotic_aur(app),
            (RepoMode::Remove, RepoAwait::Cachyos) => remove_cachyos(app),
            (RepoMode::Remove, RepoAwait::BlackArch) => remove_blackarch(app),
        }
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        match tag {
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
    fn test_repos_screen_creation() {
        let app = App::new();
        let add_screen = ReposScreen::new(&app, RepoMode::Add);
        assert_eq!(add_screen.menu.items.len(), 4);
        let remove_screen = ReposScreen::new(&app, RepoMode::Remove);
        assert_eq!(remove_screen.menu.items.len(), 4);
    }
}
