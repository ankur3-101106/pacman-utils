// ──────────────────────────────────────────────────────────────────────
// exportimport.rs — Import package lists
//
// Exports live as one-shot actions in the registry; this module holds
// the interactive import flow (pick file → preview → pacman or AUR).
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
use crate::sys;
use crate::widgets::{self, Menu, Sev};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    AskPath,
    Preview,
}

pub struct ImportScreen {
    stage: Stage,
    asked: bool,
    import_path: String,
    import_count: usize,
    menu: Menu,
    pending: Option<PendingImport>,
}

#[derive(Clone)]
enum PendingImport {
    Pacman(String),
    Aur(String),
}

impl ImportScreen {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            stage: Stage::AskPath,
            asked: false,
            import_path: String::new(),
            import_count: 0,
            menu: Menu::new("Install method", Vec::new()),
            pending: None,
        })
    }

    fn execute_import(&mut self, app: &mut App, pending: PendingImport) {
        match pending {
            PendingImport::Pacman(path) => {
                let p = std::path::PathBuf::from(&path);
                app.log("IMPORT: Installing packages via pacman");
                let cmd_args =
                    crate::sys::sudo_pacman_args(app.settings(), &["-S", "--needed", "-"]);
                app.queue_ext(
                    ExtCmd::new("import-pacman", "sudo", &cmd_args)
                        .note(format!("Install packages from {}", p.display()))
                        .stdin_file(p.clone())
                        .result("Import complete!", "Import failed.", "IMPORT: finished"),
                );
            }
            PendingImport::Aur(path) => {
                let Some(helper) = app.aur_helper() else {
                    app.toast("No AUR helper found.", Sev::Error);
                    return;
                };
                let p = std::path::PathBuf::from(&path);
                app.log(&format!("IMPORT: Installing packages via {helper}"));
                let cmd_args = crate::sys::aur_args(app.settings(), &["-S", "--needed", "-"]);
                app.queue_ext(
                    ExtCmd::new("import-aur", &helper, &cmd_args)
                        .note(format!(
                            "Install packages from {} via {helper}",
                            p.display()
                        ))
                        .stdin_file(p.clone())
                        .result("Import complete!", "Import failed.", "IMPORT: finished"),
                );
            }
        }
    }
}

impl Screen for ImportScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.stage != Stage::Preview {
            return;
        }
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let path = self.import_path.clone();
                let count = self.import_count;
                match self.menu.selected {
                    0 => {
                        if app.settings().is_true("CONFIRM_ACTIONS") {
                            app.confirm(format!("Install {count} packages from {path}?"), false);
                            self.pending = Some(PendingImport::Pacman(path));
                        } else {
                            self.execute_import(app, PendingImport::Pacman(path));
                        }
                    }
                    1 => {
                        if app.aur_helper().is_some() {
                            if app.settings().is_true("CONFIRM_ACTIONS") {
                                app.confirm(
                                    format!("Install {count} packages via AUR helper?"),
                                    false,
                                );
                                self.pending = Some(PendingImport::Aur(path));
                            } else {
                                self.execute_import(app, PendingImport::Aur(path));
                            }
                        } else {
                            app.toast("No AUR helper found.", Sev::Error);
                        }
                    }
                    _ => app.pop(),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn poll(&mut self, app: &mut App) {
        if !self.asked && !app.modal_open() {
            self.asked = true;
            app.ask_input(format!(
                "Path to package list file (empty = {})",
                sys::home_path("pkglist-explicit.txt").display()
            ));
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        if self.stage == Stage::AskPath {
            let block = widgets::panel("📥 Import Package List");
            let inner = block.inner(area);
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(widgets::span(
                        "Install packages from a pkglist text file",
                        widgets::accent(),
                    )),
                    Line::from(widgets::span(
                        format!(
                            "Default: {}",
                            sys::home_path("pkglist-explicit.txt").display()
                        ),
                        widgets::dim(),
                    )),
                ]),
                inner,
            );
            return;
        }

        // Preview pane + install method menu.
        let rows = Layout::vertical([
            Constraint::Length(6 + self.import_count.min(10) as u16 + 2),
            Constraint::Fill(1),
        ])
        .split(area);

        let block = widgets::panel(&format!(
            "📥 Import {} — {} packages",
            self.import_path, self.import_count
        ));
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(widgets::span(
            "Preview (first 10):",
            widgets::dim(),
        )));
        if let Ok(content) = std::fs::read_to_string(&self.import_path) {
            for pkg in content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(10)
            {
                lines.push(Line::from(vec![
                    widgets::span("• ", widgets::dim()),
                    Span::styled(pkg.to_string(), Style::new()),
                ]));
            }
            if self.import_count > 10 {
                lines.push(Line::from(widgets::span(
                    format!("  ... and {} more", self.import_count - 10),
                    widgets::dim(),
                )));
            }
        }
        f.render_widget(Paragraph::new(lines), inner);

        self.menu.render(f, rows[1]);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        match self.stage {
            Stage::AskPath => vec!["type a path", "enter submit", "esc cancel"],
            Stage::Preview => vec!["↑↓ navigate", "enter select", "esc back"],
        }
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let path = if value.trim().is_empty() {
            sys::home_path("pkglist-explicit.txt").display().to_string()
        } else {
            value.trim().to_string()
        };
        if !std::path::Path::new(&path).exists() {
            app.toast(format!("File not found: {path}"), Sev::Error);
            app.pop();
            return;
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let count = content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .count();
        if count == 0 {
            app.toast("Package list is empty.", Sev::Warn);
            app.pop();
            return;
        }
        self.import_path = path;
        self.import_count = count;
        self.menu = Menu::new(
            "Install method",
            vec![
                "📦 Install via pacman (official repos)".to_string(),
                format!(
                    "🌟 Install via AUR helper ({})",
                    app.settings().get("AUR_HELPER")
                ),
                "🔙 Cancel".to_string(),
            ],
        );
        self.stage = Stage::Preview;
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if !yes {
            app.toast("Import cancelled.", Sev::Info);
            return;
        }
        self.execute_import(app, pending);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if tag == "import-pacman" || tag == "import-aur" {
            if ok {
                app.toast("Import complete!", Sev::Success);
                app.log("IMPORT: Import complete");
            } else {
                app.toast("Import failed.", Sev::Error);
                app.log("IMPORT: Import failed");
            }
        }
    }
}
