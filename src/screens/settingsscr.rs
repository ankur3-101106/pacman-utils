// ──────────────────────────────────────────────────────────────────────
// settingsscr.rs — Settings screen + dependency check
//
// AUR helper selection, toggles, cache-keep count, reset to defaults,
// plus a standalone dependency report offered from Information.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Span,
    Frame,
};

use crate::app::{App, ExtCmd, Screen};
use crate::sys::{self};
use crate::widgets::{self, kv_table, Menu, Sev};

use super::args;

const ROOT_ITEMS: [&str; 8] = [
    "🔧 Change AUR Helper",
    "🧪 Toggle Dry-Run Mode",
    "✅ Toggle Confirm Actions",
    "🛡 Toggle Native Pkg-Mgr Confirm",
    "📝 Toggle Logging",
    "📦 Set Cache Keep Count",
    "🔄 Reset to Defaults",
    "🔙 Back",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Root,
    AurHelperMenu,
}

enum Await {
    CacheKeep,
    BuildHelper(String),
    ResetDefaults,
}

/// Copy of the displayed settings for rendering (draw has no App access).
#[derive(Default, Clone)]
struct Snapshot {
    aur_helper: String,
    dry_run: String,
    confirm_actions: String,
    native_confirm: String,
    log_enabled: String,
    keep: String,
}

impl Snapshot {
    fn from_app(app: &App) -> Self {
        let s = app.settings();
        let on_off = |k: &str| {
            if s.is_true(k) {
                "true".to_string()
            } else {
                "false".to_string()
            }
        };
        Self {
            aur_helper: s.get("AUR_HELPER").to_string(),
            dry_run: on_off("DRY_RUN"),
            confirm_actions: on_off("CONFIRM_ACTIONS"),
            native_confirm: on_off("NATIVE_CONFIRM"),
            log_enabled: on_off("LOG_ENABLED"),
            keep: s.get("PACCACHE_KEEP").to_string(),
        }
    }
}

pub struct SettingsScreen {
    mode: Mode,
    root: Menu,
    aur_menu: Menu,
    snapshot: Snapshot,
    await_kind: Option<Await>,
}

fn root_menu() -> Menu {
    Menu::new("⚙  Settings", ROOT_ITEMS.map(str::to_string).to_vec())
}

impl SettingsScreen {
    pub fn new(app: &App) -> Box<Self> {
        Box::new(Self {
            mode: Mode::Root,
            root: root_menu(),
            aur_menu: Menu::new("AUR helper", vec!["yay".to_string(), "paru".to_string()]),
            snapshot: Snapshot::from_app(app),
            await_kind: None,
        })
    }

    fn toggle(app: &mut App, key: &str) {
        let now_on = app.settings_mut().toggle(key);
        app.settings().save();
        if now_on {
            app.toast(format!("{key} enabled"), Sev::Info);
        } else {
            app.toast(format!("{key} disabled"), Sev::Info);
        }
    }

    fn build_helper(&self, app: &mut App, choice: String) {
        app.toast("Installing build dependencies...", Sev::Info);
        let deps_args =
            crate::sys::sudo_pacman_args(app.settings(), &["-S", "--needed", "git", "base-devel"]);
        app.queue_ext(
            ExtCmd::new("deps-buildtools", "sudo", &deps_args).note("Install git and base-devel"),
        );
        let tag = format!("build-{choice}");
        let noconfirm_flag = if !app.settings().is_true("NATIVE_CONFIRM") {
            " --noconfirm"
        } else {
            ""
        };
        let script = format!(
            "tmp=$(mktemp -d); git clone https://aur.archlinux.org/{choice}.git \"$tmp/{choice}\" && (cd \"$tmp/{choice}\" && makepkg -si{noconfirm_flag}); rm -rf \"$tmp\""
        );
        app.queue_ext(
            ExtCmd::new(&tag, "bash", &args(&["-c", &script]))
                .note(format!("Build and install {choice} from AUR"))
                .result(
                    format!("Installed and set AUR helper to {choice}"),
                    format!("Failed to build {choice} from AUR."),
                    "SETTINGS: AUR helper build finished",
                ),
        );
    }
}

impl Screen for SettingsScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            Mode::Root => {
                if self.root.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => match self.root.selected {
                        0 => self.mode = Mode::AurHelperMenu,
                        1 => {
                            Self::toggle(app, "DRY_RUN");
                            self.snapshot = Snapshot::from_app(app);
                        }
                        2 => {
                            Self::toggle(app, "CONFIRM_ACTIONS");
                            self.snapshot = Snapshot::from_app(app);
                        }
                        3 => {
                            Self::toggle(app, "NATIVE_CONFIRM");
                            self.snapshot = Snapshot::from_app(app);
                        }
                        4 => {
                            Self::toggle(app, "LOG_ENABLED");
                            self.snapshot = Snapshot::from_app(app);
                        }
                        5 => {
                            self.await_kind = Some(Await::CacheKeep);
                            app.ask_input("Number of versions to keep (1-10)");
                        }
                        6 => {
                            if app.settings().is_true("CONFIRM_ACTIONS") {
                                app.confirm("Reset all settings to defaults?", true);
                                self.await_kind = Some(Await::ResetDefaults);
                            } else {
                                app.settings_mut().reset_defaults();
                                app.settings().save();
                                self.snapshot = Snapshot::from_app(app);
                                app.toast("Settings reset to defaults.", Sev::Success);
                            }
                        }
                        _ => app.pop(),
                    },
                    KeyCode::Esc | KeyCode::Char('q') => app.pop(),
                    _ => {}
                }
            }
            Mode::AurHelperMenu => {
                if self.aur_menu.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        let choice = self.aur_menu.selected_item().unwrap_or("yay").to_string();
                        if sys::has_bin(&choice) {
                            app.settings_mut().set("AUR_HELPER", choice.clone());
                            app.settings().save();
                            app.toast(format!("AUR helper set to {choice}"), Sev::Success);
                        } else {
                            app.toast(format!("{choice} is not installed."), Sev::Error);
                            if app.settings().is_true("CONFIRM_ACTIONS") {
                                app.confirm(format!("Install {choice}?"), false);
                                self.await_kind = Some(Await::BuildHelper(choice));
                            } else {
                                self.build_helper(app, choice);
                            }
                        }
                        self.snapshot = Snapshot::from_app(app);
                        self.mode = Mode::Root;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Root,
                    _ => {}
                }
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let rows = Layout::vertical([Constraint::Length(13), Constraint::Fill(1)]).split(area);
        let snap = &self.snapshot;
        kv_table(
            f,
            rows[0],
            "⚙  Settings",
            &[
                ("AUR Helper", snap.aur_helper.clone()),
                ("Dry-Run Mode", snap.dry_run.clone()),
                ("Confirm Actions", snap.confirm_actions.clone()),
                ("Native Pkg-Mgr Confirm", snap.native_confirm.clone()),
                ("Logging", snap.log_enabled.clone()),
                ("Cache Keep", format!("{} versions", snap.keep)),
                (
                    "Config File",
                    crate::settings::settings_file().display().to_string(),
                ),
                (
                    "Log File",
                    crate::settings::log_file().display().to_string(),
                ),
            ],
        );
        match self.mode {
            Mode::Root => self.root.render(f, rows[1]),
            Mode::AurHelperMenu => self.aur_menu.render(f, rows[1]),
        }
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓ navigate", "enter select", "esc back"]
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let Some(await_kind) = self.await_kind.take() else {
            return;
        };
        if let Await::CacheKeep = await_kind {
            match value.trim().parse::<u32>() {
                Ok(n) if (1..=10).contains(&n) => {
                    app.settings_mut().set("PACCACHE_KEEP", n.to_string());
                    app.settings().save();
                    self.snapshot = Snapshot::from_app(app);
                    app.toast(
                        format!("Will keep {n} package versions when cleaning cache."),
                        Sev::Success,
                    );
                }
                _ => app.toast("Invalid number. Please enter 1-10.", Sev::Error),
            }
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(await_kind) = self.await_kind.take() else {
            return;
        };
        match await_kind {
            Await::ResetDefaults => {
                if !yes {
                    return;
                }
                app.settings_mut().reset_defaults();
                app.settings().save();
                self.snapshot = Snapshot::from_app(app);
                app.toast("Settings reset to defaults.", Sev::Success);
            }
            Await::BuildHelper(choice) => {
                if !yes {
                    return;
                }
                self.build_helper(app, choice);
            }
            _ => {}
        }
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, _ok: bool) {
        match tag {
            "deps-buildtools" => { /* outcome reported by build-* tag */ }
            "build-yay" | "build-paru" => {
                let choice = tag.trim_start_matches("build-");
                if sys::has_bin(choice) {
                    app.settings_mut().set("AUR_HELPER", choice.to_string());
                    app.settings().save();
                    self.snapshot = Snapshot::from_app(app);
                    app.toast(
                        format!("Installed and set AUR helper to {choice}"),
                        Sev::Success,
                    );
                } else {
                    app.toast(format!("Failed to install {choice}."), Sev::Error);
                }
            }
            _ => {}
        }
    }
}

// ── Dependency Check ────────────────────────────────────────────────

pub struct DepsCheckScreen {
    lines: Vec<Vec<Span<'static>>>,
    missing: Vec<String>,
    await_install: bool,
}

impl DepsCheckScreen {
    pub fn new(app: &App) -> Box<Self> {
        let caps = app.caps().clone();
        let mut missing: Vec<String> = Vec::new();
        let mut lines: Vec<Vec<Span>> = Vec::new();

        let yay = sys::has_bin("yay");
        let paru = sys::has_bin("paru");

        let push_row = |lines: &mut Vec<Vec<Span>>, name: &str, desc: &str, present: bool| {
            let (icon, style) = if present {
                ("✔", widgets::success())
            } else {
                ("⚠", widgets::warning())
            };
            lines.push(vec![
                widgets::span(format!("{icon} "), style),
                widgets::span(name.to_string(), widgets::accent_bold()),
                widgets::span(format!(" — {desc}"), Style::new()),
            ]);
            if !present {
                lines.push(vec![widgets::span(
                    "     not installed".to_string(),
                    widgets::dim(),
                )]);
            }
        };

        push_row(&mut lines, "yay", "AUR helper", yay);
        push_row(&mut lines, "paru", "alternative AUR helper", paru);
        push_row(&mut lines, "reflector", "mirror management", caps.reflector);
        push_row(
            &mut lines,
            "paccache",
            "cache cleaning (pacman-contrib)",
            caps.paccache,
        );
        push_row(
            &mut lines,
            "checkupdates",
            "update listing (pacman-contrib)",
            caps.checkupdates,
        );

        // Only pacman-installable packages are offered here; AUR helpers
        // go through Settings ▸ Change AUR Helper.
        if !caps.reflector {
            missing.push("reflector".into());
        }
        if !caps.paccache {
            missing.push("pacman-contrib".into());
        }

        Box::new(Self {
            lines,
            missing,
            await_install: false,
        })
    }
}

impl DepsCheckScreen {
    fn install_deps(&self, app: &mut App) {
        if self.missing.is_empty() {
            return;
        }
        let missing = self.missing.clone();
        let mut raw = vec!["-S", "--needed"];
        let missing_refs: Vec<&str> = missing.iter().map(String::as_str).collect();
        raw.extend(missing_refs);
        let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &raw);
        app.queue_ext(
            ExtCmd::new("deps-install", "sudo", &cmd_args)
                .note(format!("Install {}", missing.join(", ")))
                .result(
                    "Dependencies installed.",
                    "Dependency installation failed.",
                    "SETTINGS: dependencies finished",
                ),
        );
    }
}

impl Screen for DepsCheckScreen {
    fn handle_key(&mut self, _app: &mut App, _key: KeyEvent) {}

    fn poll(&mut self, app: &mut App) {
        if !self.await_install && !self.missing.is_empty() && !app.modal_open() {
            self.await_install = true;
            if app.settings().is_true("CONFIRM_ACTIONS") {
                app.confirm("Install missing dependencies?", false);
            } else {
                self.install_deps(app);
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = widgets::panel("🔍 Dependency Check");
        let inner = block.inner(area);
        f.render_widget(block, area);
        let styled: Vec<ratatui::text::Line> = self
            .lines
            .iter()
            .map(|l| ratatui::text::Line::from(l.clone()))
            .collect();
        f.render_widget(ratatui::widgets::Paragraph::new(styled), inner);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["esc back"]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        if !yes || self.missing.is_empty() {
            return;
        }
        self.install_deps(app);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if tag != "deps-install" {
            return;
        }
        app.refresh_caps();
        if ok {
            app.toast("Dependencies installed.", Sev::Success);
        } else {
            app.toast("Dependency installation failed.", Sev::Error);
        }
    }
}
