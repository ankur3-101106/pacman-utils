// ──────────────────────────────────────────────────────────────────────
// groups.rs — Pre-defined package group installer (lib/groups.sh)
//
// The 12 curated groups with per-package install status and three
// installation strategies.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ExtCmd, Screen};
use crate::sys::{self, PKG_GROUPS};
use crate::widgets::{self, FuzzyList, Menu, Sev};

use super::install::install_specific_package;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Root,
    Detail,
    SpecificPick,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GStatus {
    Installed,
    Available,
    AurMissing,
}

impl GStatus {
    fn label(self) -> &'static str {
        match self {
            GStatus::Installed => "[installed]",
            GStatus::Available => "[available]",
            GStatus::AurMissing => "[AUR/missing]",
        }
    }
    fn style(self) -> Style {
        match self {
            GStatus::Installed => widgets::success(),
            GStatus::Available => widgets::dim(),
            GStatus::AurMissing => widgets::warning(),
        }
    }
}

struct Detail {
    desc: String,
    names: Vec<String>,
    statuses: Vec<GStatus>,
}

impl Default for Detail {
    fn default() -> Self {
        Self { desc: String::new(), names: Vec::new(), statuses: Vec::new() }
    }
}

impl Detail {
    fn build(idx: usize) -> Self {
        let (_, desc, pkgs) = PKG_GROUPS[idx];
        let names: Vec<String> = pkgs.iter().map(|s| s.to_string()).collect();
        let installed = sys::installed_set();
        let (official, _) = sys::filter_official(&names);
        let official_set: std::collections::HashSet<&String> = official.iter().collect();
        let statuses = names
            .iter()
            .map(|n| {
                if installed.contains(n) {
                    GStatus::Installed
                } else if official_set.contains(n) {
                    GStatus::Available
                } else {
                    GStatus::AurMissing
                }
            })
            .collect();
        Self { desc: desc.to_string(), names, statuses }
    }
}

enum Await {
    GroupBatch(Vec<String>),
    GroupOne(String),
}

pub struct GroupsScreen {
    mode: Mode,
    root: Menu,
    detail: Detail,
    detail_menu: Menu,
    specific: FuzzyList,
    await_kind: Option<Await>,
}

fn root_menu() -> Menu {
    let items: Vec<String> = PKG_GROUPS
        .iter()
        .map(|(_, desc, _)| desc.to_string())
        .chain(std::iter::once("🔙 Back to Main Menu".to_string()))
        .collect();
    Menu::new("📦  Package Groups", items)
}

fn detail_method_menu() -> Menu {
    Menu::new(
        "Installation method",
        vec![
            "📦 Install all packages".to_string(),
            "✅ Install only missing packages".to_string(),
            "🔍 Select specific packages".to_string(),
            "🔙 Cancel".to_string(),
        ],
    )
}

impl GroupsScreen {
    pub fn new() -> Self {
        Self {
            mode: Mode::Root,
            root: root_menu(),
            detail: Detail::default(),
            detail_menu: detail_method_menu(),
            specific: FuzzyList::new("Select packages...", Vec::new()),
            await_kind: None,
        }
    }

    /// Queue official then AUR installs for a package set.
    fn do_install(&mut self, app: &mut App, pkgs: Vec<String>, desc: &str) {
        let (official, aur) = sys::filter_official(&pkgs);
        let has_official = !official.is_empty();
        let has_aur = !aur.is_empty();

        if has_official {
            let n = official.len();
            app.toast(format!("Installing {n} official packages..."), Sev::Info);
            let mut cmd_args = vec!["-S".to_string(), "--needed".to_string()];
            cmd_args.extend(official);
            app.queue_ext(
                ExtCmd::new("grp-official", "sudo", &cmd_args)
                    .note(format!("Install group: {desc} (official)"))
                    .result(
                        "Group installation complete!",
                        "Group installation had issues.",
                        "PKG_GROUPS: official batch finished",
                    ),
            );
        }
        if has_aur {
            let n = aur.len();
            match app.aur_helper() {
                Some(helper) => {
                    app.toast(format!("Installing {n} AUR packages via {helper}..."), Sev::Info);
                    let mut cmd_args = vec!["-S".to_string(), "--needed".to_string()];
                    cmd_args.extend(aur);
                    app.queue_ext(
                        ExtCmd::new("grp-aur", &helper, &cmd_args)
                            .note(format!("Install group: {desc} (AUR via {helper})"))
                            .result(
                                "Group installation complete!",
                                "Group installation had issues.",
                                "PKG_GROUPS: batch finished",
                            ),
                    );
                }
                None => {
                    app.toast("Cannot install AUR packages — no AUR helper found:", Sev::Warn);
                    app.log(&format!(
                        "PKG_GROUPS: skipped {n} AUR packages for {desc}: {}",
                        aur.join(", ")
                    ));
                }
            }
        }
        if !has_official && !has_aur {
            app.toast("Group installation complete!", Sev::Success);
        }
        app.log(&format!("PKG_GROUPS: Installing {} packages for {desc}", pkgs.len()));
    }
}

impl Default for GroupsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for GroupsScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            Mode::Root => {
                if self.root.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if self.root.selected >= PKG_GROUPS.len() {
                            app.pop();
                        } else {
                            self.detail = Detail::build(self.root.selected);
                            self.detail_menu = detail_method_menu();
                            self.mode = Mode::Detail;
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => app.pop(),
                    _ => {}
                }
            }
            Mode::SpecificPick => {
                if self.specific.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if let Some(pkg) = self.specific.take_selected() {
                            app.confirm(format!("Install {pkg}?"), false);
                            self.await_kind = Some(Await::GroupOne(pkg));
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Root,
                    _ => {}
                }
            }
            Mode::Detail => {
                if self.detail_menu.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        let desc = self.detail.desc.clone();
                        let names = self.detail.names.clone();
                        match self.detail_menu.selected {
                            0 => {
                                let total = names.len();
                                app.confirm(format!("Install all {total} packages?"), false);
                                self.await_kind = Some(Await::GroupBatch(names));
                            }
                            1 => {
                                let missing: Vec<String> = names
                                    .into_iter()
                                    .zip(self.detail.statuses.iter().copied())
                                    .filter(|(_, s)| *s != GStatus::Installed)
                                    .map(|(n, _)| n)
                                    .collect();
                                if missing.is_empty() {
                                    app.toast("All packages are already installed!", Sev::Success);
                                } else {
                                    let n = missing.len();
                                    app.confirm(format!("Install {n} missing packages?"), false);
                                    self.await_kind = Some(Await::GroupBatch(missing));
                                }
                            }
                            2 => {
                                self.specific =
                                    FuzzyList::new("Select packages...", self.detail.names.clone());
                                self.mode = Mode::SpecificPick;
                            }
                            _ => self.mode = Mode::Root,
                        }
                        let _ = desc;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Root,
                    _ => {}
                }
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        match self.mode {
            Mode::Root => self.root.render(f, area),
            Mode::SpecificPick => self.specific.render(f, area),
            Mode::Detail => {
                let rows = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(self.detail_menu.items.len() as u16 + 2),
                ])
                .split(area);

                let block = widgets::panel(&format!(
                    "📦  {} — {} packages",
                    self.detail.desc,
                    self.detail.names.len()
                ));
                let inner = block.inner(rows[0]);
                f.render_widget(block, rows[0]);
                let lines: Vec<Line> = self
                    .detail
                    .names
                    .iter()
                    .zip(self.detail.statuses.iter())
                    .map(|(n, s)| {
                        Line::from(vec![
                            widgets::span("• ", widgets::accent()),
                            Span::styled(n.clone(), Style::new()),
                            widgets::span(format!("  {}", s.label()), s.style()),
                        ])
                    })
                    .collect();
                f.render_widget(Paragraph::new(lines), inner);

                self.detail_menu.render(f, rows[1]);
            }
        }
    }

    fn help_hints(&self) -> Vec<&'static str> {
        match self.mode {
            Mode::Root => vec!["↑↓ navigate", "enter select", "esc back"],
            Mode::Detail => vec!["↑↓ navigate", "enter select", "esc cancel"],
            Mode::SpecificPick => vec!["type to filter", "enter select", "esc back"],
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(await_kind) = self.await_kind.take() else { return };
        match await_kind {
            Await::GroupBatch(pkgs) => {
                if !yes {
                    return;
                }
                let desc = self.detail.desc.clone();
                self.do_install(app, pkgs, &desc);
            }
            Await::GroupOne(pkg) => {
                if yes {
                    install_specific_package(app, &pkg);
                }
            }
        }
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if matches!(tag, "grp-official" | "grp-aur" | "install-specific-official" | "install-specific-aur") {
            if ok {
                app.toast("Group installation complete!", Sev::Success);
                app.log("PKG_GROUPS: installation successful");
            } else {
                app.toast("Group installation had issues.", Sev::Error);
                app.log("PKG_GROUPS: installation failed");
            }
        }
    }
}
