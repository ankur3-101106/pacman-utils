// ──────────────────────────────────────────────────────────────────────
// packages.rs — Package utilities as focused screens
//
// Each utility that used to live behind the nested "Package Utilities"
// menu is its own screen so the home browser can launch it directly.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, style::Style, text::Span, widgets::Paragraph, Frame};

use crate::app::{App, ExtCmd, Screen};
use crate::sys::{self};
use crate::widgets::{self, FuzzyList, Menu, Sev};

use super::viewer::ViewerScreen;

// ── Shared helpers ──────────────────────────────────────────────────

fn installed_rows(explicit_only: bool) -> Vec<String> {
    let flag = if explicit_only { "-Qe" } else { "-Q" };
    match sys::capture("pacman", &[flag]) {
        Some(s) => s.lines().map(str::to_string).collect(),
        None => Vec::new(),
    }
}

fn installed_names() -> Vec<String> {
    installed_rows(false)
        .iter()
        .map(|r| r.split_whitespace().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Look up a package across local db → repos → AUR and build an info
/// viewer, mirroring `_pkg_show_info_for`.
fn info_viewer_for(pkg: &str, aur_helper: Option<String>) -> Option<Box<ViewerScreen>> {
    use crate::screens::info_lines;

    if let Some(raw) = sys::qi(pkg) {
        let mut lines = vec![vec![widgets::span(
            "Installed package:",
            widgets::success(),
        )]];
        lines.extend(info_lines(&raw, &[]));
        return Some(ViewerScreen::new(
            format!("📖 Package Information — {pkg}"),
            lines,
        ));
    }
    if let Some(raw) = sys::si(&[pkg]) {
        let mut lines = vec![vec![widgets::span(
            "Available in repository:",
            widgets::accent(),
        )]];
        lines.extend(info_lines(&raw, &[]));
        return Some(ViewerScreen::new(
            format!("📖 Package Information — {pkg}"),
            lines,
        ));
    }
    if let Some(helper) = aur_helper {
        if let Some(raw) = crate::sys::capture(&helper, &["-Si", pkg]) {
            let mut lines = vec![vec![widgets::span("Available in AUR:", widgets::warning())]];
            lines.extend(info_lines(&raw, &[]));
            return Some(ViewerScreen::new(
                format!("📖 Package Information — {pkg}"),
                lines,
            ));
        }
    }
    None
}

fn name_of(entry: &str) -> String {
    entry.split_whitespace().next().unwrap_or("").to_string()
}

// ── Browse Installed / Explicit ─────────────────────────────────────

pub struct BrowsePackagesScreen {
    pick: FuzzyList,
    empty: bool,
    explicit: bool,
}

impl BrowsePackagesScreen {
    pub fn new(explicit: bool) -> Box<Self> {
        let rows = installed_rows(explicit);
        let total = rows.len();
        let title = if explicit {
            format!("Explicitly Installed Packages ({total})")
        } else {
            format!("Installed Packages ({total})")
        };
        Box::new(Self {
            pick: FuzzyList::new(title, rows),
            empty: total == 0,
            explicit,
        })
    }
}

impl Screen for BrowsePackagesScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.pick.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let Some(entry) = self.pick.take_selected() else {
                    return;
                };
                let pkg = name_of(&entry);
                match info_viewer_for(&pkg, app.aur_helper()) {
                    Some(v) => app.push(v),
                    None => app.toast(format!("Package '{pkg}' not found."), Sev::Error),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        if self.empty {
            let kind = if self.explicit {
                "explicitly installed"
            } else {
                "installed"
            };
            f.render_widget(widgets::panel(&format!("No {kind} packages found.")), area);
            return;
        }
        self.pick.render(f, area);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["type to filter", "enter details", "esc back"]
    }
}

// ── Files Owned by Package ──────────────────────────────────────────

pub struct FilesPickScreen {
    pick: FuzzyList,
}

impl FilesPickScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            pick: FuzzyList::new("Select a package...", installed_names()),
        })
    }
}

impl Screen for FilesPickScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.pick.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let Some(entry) = self.pick.take_selected() else {
                    return;
                };
                let pkg = name_of(&entry);
                let files = sys::ql_files(&pkg);
                if files.is_empty() {
                    app.toast(format!("No files found for {pkg}."), Sev::Error);
                } else {
                    let count = files.len();
                    let lines: Vec<Vec<Span>> = files
                        .into_iter()
                        .map(|f| vec![Span::styled(f, Style::new())])
                        .collect();
                    let title = format!("📂 Files in {pkg} — {count} files");
                    app.push(ViewerScreen::new(title, lines));
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        self.pick.render(f, area);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["type to filter", "enter select", "esc back"]
    }
}

// ── Which Package Owns a File ───────────────────────────────────────

pub struct OwnerQueryScreen {
    asked: bool,
}

impl OwnerQueryScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self { asked: false })
    }
}

impl Screen for OwnerQueryScreen {
    fn handle_key(&mut self, _app: &mut App, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            // Handled after the input modal closes; harmless otherwise.
        }
    }

    fn poll(&mut self, app: &mut App) {
        if !self.asked && !app.modal_open() {
            self.asked = true;
            app.ask_input("Enter absolute file path");
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        f.render_widget(widgets::panel("🔎 Find which package owns a file"), area);
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let path = value.trim().to_string();
        if path.is_empty() {
            app.toast("No path entered.", Sev::Warn);
            app.pop();
            return;
        }
        match sys::qo(&path) {
            Ok(msg) => {
                app.toast(msg, Sev::Success);
                app.pop();
            }
            Err(_) => {
                app.toast(format!("No package owns '{path}'"), Sev::Error);
                app.toast(
                    "The file may belong to an AUR package or was manually created.",
                    Sev::Info,
                );
                app.pop();
            }
        }
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["type a path", "enter submit", "esc cancel"]
    }
}

// ── Package Info Lookup ─────────────────────────────────────────────

pub struct InfoQueryScreen {
    asked: bool,
}

impl InfoQueryScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self { asked: false })
    }
}

impl Screen for InfoQueryScreen {
    fn handle_key(&mut self, _app: &mut App, _key: KeyEvent) {}

    fn poll(&mut self, app: &mut App) {
        if !self.asked && !app.modal_open() {
            self.asked = true;
            app.ask_input("Enter package name");
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        f.render_widget(widgets::panel("📖 Package Information"), area);
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let pkg = value.trim().to_string();
        if pkg.is_empty() {
            app.toast("No package name entered.", Sev::Warn);
            app.pop();
            return;
        }
        match info_viewer_for(&pkg, app.aur_helper()) {
            Some(v) => {
                // Replace this query stub with the info viewer.
                app.pop();
                app.push(v);
            }
            None => {
                app.toast(format!("Package '{pkg}' not found."), Sev::Error);
                app.pop();
            }
        }
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["type a name", "enter submit", "esc cancel"]
    }
}

// ── Installation History ────────────────────────────────────────────

pub fn history_viewer() -> Box<dyn Screen> {
    let entries = sys::history_entries(50);
    let lines: Vec<Vec<Span>> = if entries.is_empty() {
        vec![vec![widgets::span(
            "(no installation history found)",
            widgets::dim(),
        )]]
    } else {
        entries
            .into_iter()
            .map(|(kind, line)| {
                let (marker, color) = kind.marker();
                let style = match color {
                    "green" => widgets::success(),
                    "blue" => widgets::accent(),
                    _ => widgets::danger(),
                };
                vec![
                    widgets::span(format!("{marker} "), style),
                    Span::styled(line, Style::new()),
                ]
            })
            .collect()
    };
    ViewerScreen::new("🕒 Installation History (Recent)", lines)
}

// ── Transaction Log Viewer ──────────────────────────────────────────

const LOG_ITEMS: [&str; 6] = [
    "📋 Full log (last 200 lines)",
    "📦 Installations only",
    "⬆  Upgrades only",
    "🗑  Removals only",
    "⚠  Warnings & errors",
    "🔙 Back",
];

pub struct TransactionLogScreen {
    menu: Menu,
}

impl TransactionLogScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            menu: Menu::new("Transaction Log", LOG_ITEMS.map(str::to_string).to_vec()),
        })
    }
}

impl Screen for TransactionLogScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let screen = match self.menu.selected {
                    0 => ViewerScreen::from_text(
                        "Transaction Log (last 200)",
                        &sys::log_tail(&[], 200).join("\n"),
                    ),
                    1 => ViewerScreen::from_text(
                        "Installations (last 100)",
                        &sys::log_tail(&["[ALPM] installed"], 100).join("\n"),
                    ),
                    2 => ViewerScreen::from_text(
                        "Upgrades (last 100)",
                        &sys::log_tail(&["[ALPM] upgraded"], 100).join("\n"),
                    ),
                    3 => ViewerScreen::from_text(
                        "Removals (last 100)",
                        &sys::log_tail(&["[ALPM] removed"], 100).join("\n"),
                    ),
                    4 => ViewerScreen::from_text(
                        "Warnings & Errors (last 100)",
                        &sys::log_tail(&["[ALPM] warning", "[ALPM] error"], 100).join("\n"),
                    ),
                    _ => {
                        app.pop();
                        return;
                    }
                };
                app.push(screen);
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        self.menu.render(f, area);
    }
}

// ── Reinstall Package ───────────────────────────────────────────────

pub struct ReinstallPickScreen {
    pick: FuzzyList,
    pending: Option<String>,
}

impl ReinstallPickScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            pick: FuzzyList::new("Select a package to reinstall...", installed_names()),
            pending: None,
        })
    }

    fn execute_reinstall(&mut self, app: &mut App, pkg: &str) {
        app.log(&format!("PACKAGES: Reinstalling {pkg}"));
        let cmd_args =
            crate::sys::sudo_pacman_args(app.settings(), &["-S", "--overwrite", "*", pkg]);
        app.queue_ext(
            ExtCmd::new("reinstall", "sudo", &cmd_args)
                .note(format!("Reinstall {pkg} (--overwrite '*')"))
                .result(
                    "Package reinstalled!",
                    "Reinstallation failed.",
                    "PACKAGES: reinstall finished",
                ),
        );
    }
}

impl Screen for ReinstallPickScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.pick.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let Some(entry) = self.pick.take_selected() else {
                    return;
                };
                let pkg = name_of(&entry);
                let tx = crate::tx::TransactionSpec::reinstall(app.settings(), &pkg);
                app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        self.pick.render(f, area);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["type to filter", "enter select", "esc back"]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(pkg) = self.pending.take() else {
            return;
        };
        if !yes {
            return;
        }
        self.execute_reinstall(app, &pkg);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if tag != "reinstall" {
            return;
        }
        if ok {
            app.toast("Package reinstalled!", Sev::Success);
            app.log("PACKAGES: reinstall successful");
        } else {
            app.toast("Reinstallation failed.", Sev::Error);
            app.log("PACKAGES: reinstall failed");
        }
    }
}

// ── Remove Orphan Packages ──────────────────────────────────────────

pub struct OrphansScreen {
    orphans: Vec<String>,
    menu: Menu,
    pending: Option<Vec<String>>,
}

impl OrphansScreen {
    pub fn new() -> Box<Self> {
        let orphans = sys::orphan_names();
        let menu = Menu::new(
            "Orphan action",
            vec![
                "🧹 Remove all orphan packages".to_string(),
                "🔙 Cancel".to_string(),
            ],
        );
        Box::new(Self {
            orphans,
            menu,
            pending: None,
        })
    }
}

impl OrphansScreen {
    fn execute_remove_orphans(&mut self, app: &mut App, orphans: Vec<String>) {
        let count = orphans.len();
        app.log(&format!("PACKAGES: Removing {count} orphan packages"));
        let mut raw = vec!["-Rns"];
        let orphan_refs: Vec<&str> = orphans.iter().map(String::as_str).collect();
        raw.extend(orphan_refs);
        let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &raw);
        app.queue_ext(
            ExtCmd::new("orphans-rm", "sudo", &cmd_args)
                .note(format!("Remove {count} orphan packages"))
                .result(
                    "Orphan packages removed!",
                    "Some packages may not have been removed.",
                    "PACKAGES: orphan removal finished",
                ),
        );
    }
}

impl Screen for OrphansScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => match self.menu.selected {
                0 => {
                    let v = self.orphans.clone();
                    let tx = crate::tx::TransactionSpec::remove_orphans(app.settings(), &v);
                    app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
                }
                _ => app.pop(),
            },
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        if self.orphans.is_empty() {
            f.render_widget(
                Paragraph::new(ratatui::text::Line::from(Span::styled(
                    " ✔ No orphan packages found!",
                    widgets::success(),
                )))
                .block(widgets::panel("🧹 Orphan Packages")),
                area,
            );
            return;
        }
        let rows = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Fill(1),
            ratatui::layout::Constraint::Length(4),
        ])
        .split(area);
        let block = widgets::panel(&format!(
            "🧹 {} orphan package(s) found",
            self.orphans.len()
        ));
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);
        let lines: Vec<ratatui::text::Line> = self
            .orphans
            .iter()
            .map(|p| {
                ratatui::text::Line::from(vec![
                    widgets::span("• ", widgets::dim()),
                    Span::styled(p.clone(), Style::new()),
                ])
            })
            .collect();
        f.render_widget(ratatui::widgets::Paragraph::new(lines), inner);
        self.menu.render(f, rows[1]);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        if self.orphans.is_empty() {
            vec!["esc back"]
        } else {
            vec!["↑↓ navigate", "enter confirm", "esc back"]
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(orphans) = self.pending.take() else {
            return;
        };
        if !yes {
            return;
        }
        self.execute_remove_orphans(app, orphans);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if tag != "orphans-rm" {
            return;
        }
        if ok {
            app.toast("Orphan packages removed!", Sev::Success);
            app.log("PACKAGES: Orphan removal successful");
            self.orphans = sys::orphan_names();
        } else {
            app.toast("Some packages may not have been removed.", Sev::Error);
            app.log("PACKAGES: Orphan removal had issues");
        }
    }
}
