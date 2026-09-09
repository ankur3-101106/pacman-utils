// ──────────────────────────────────────────────────────────────────────
// lockfile.rs — Pacman lock file management (lib/lockfile.sh)
//
// Detects db.lck, warns if pacman is running, removes with safety
// checks.
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
use crate::sys::{self, LOCK_FILE};
use crate::widgets::{self, Sev};

use super::args;

pub struct LockfileScreen {
    exists: bool,
    info: Option<(String, String)>,
    pids: Vec<String>,
}

impl LockfileScreen {
    pub fn new() -> Self {
        let s = Self {
            exists: false,
            info: None,
            pids: Vec::new(),
        };
        s.refresh_state()
    }

    fn refresh_state(mut self) -> Self {
        self.exists = std::path::Path::new(LOCK_FILE).exists();
        self.info = if self.exists {
            sys::lockfile_info()
        } else {
            None
        };
        self.pids = if self.exists {
            sys::pacman_pids()
        } else {
            Vec::new()
        };
        self
    }
}

impl Default for LockfileScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl LockfileScreen {
    fn execute_remove(&self, app: &mut App) {
        app.log(&format!("LOCKFILE: Removing {LOCK_FILE}"));
        app.queue_ext(
            ExtCmd::new("lockfile-rm", "sudo", &args(&["rm", "-f", LOCK_FILE]))
                .note("Remove pacman lock file")
                .result(
                    "Lock file removed successfully.",
                    "Failed to remove lock file.",
                    "LOCKFILE: finished",
                ),
        );
    }
}

impl Screen for LockfileScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match key.code {
            KeyCode::Enter if self.exists => {
                if app.settings().is_true("CONFIRM_ACTIONS") {
                    app.confirm("Remove lock file?", true);
                } else {
                    self.execute_remove(app);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = widgets::panel("🔒  Pacman Lock File");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        if !self.exists {
            lines.push(Line::from(widgets::span(
                "✔ No lock file found — pacman is free to run.",
                widgets::success(),
            )));
        } else {
            lines.push(Line::from(widgets::span(
                "⚠ Lock file exists!",
                widgets::warning(),
            )));
            lines.push(Line::from(""));

            // Key/value table rendered inline.
            let (created, size) = self.info.clone().unwrap_or(("unknown".into(), "?".into()));
            let width = 8;
            let push_kv = |lines: &mut Vec<Line>, k: &str, v: String| {
                lines.push(Line::from(vec![
                    widgets::span(format!("  {k:<width$} │ "), widgets::accent_bold()),
                    Span::styled(v, Style::new()),
                ]));
            };
            push_kv(&mut lines, "File", LOCK_FILE.to_string());
            push_kv(&mut lines, "Created", created);
            push_kv(&mut lines, "Size", size);

            lines.push(Line::from(""));
            lines.push(Line::from(widgets::span(
                "ℹ This file prevents multiple pacman instances from running.",
                widgets::accent(),
            )));
            lines.push(Line::from(widgets::span(
                "ℹ Only remove it if you're sure no other pacman process is active.",
                widgets::accent(),
            )));
            lines.push(Line::from(""));

            if !self.pids.is_empty() {
                lines.push(Line::from(widgets::span(
                    format!(
                        "✘ WARNING: pacman appears to be running! PID(s): {}",
                        self.pids.join(" ")
                    ),
                    widgets::danger(),
                )));
                lines.push(Line::from(widgets::span(
                    "✘ Removing the lock file while pacman is running can corrupt your database.",
                    widgets::danger(),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(widgets::span(
                    "enter remove anyway · esc back",
                    widgets::warning(),
                )));
            } else {
                lines.push(Line::from(widgets::span(
                    "enter remove lock file · esc back",
                    widgets::accent(),
                )));
            }
        }

        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
        f.render_widget(Paragraph::new(lines), rows[1]);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        if self.exists {
            vec!["enter remove lock file", "esc back"]
        } else {
            vec!["esc back"]
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        if !yes {
            app.toast("Lock file kept.", Sev::Info);
            return;
        }
        self.execute_remove(app);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, _ok: bool) {
        if tag != "lockfile-rm" {
            return;
        }
        *self = Self::new();
        if !self.exists {
            app.toast("Lock file removed successfully.", Sev::Success);
            app.log("LOCKFILE: Lock file removed");
        } else {
            app.toast("Failed to remove lock file.", Sev::Error);
            app.log("LOCKFILE: Failed to remove lock file");
        }
    }
}
