// ──────────────────────────────────────────────────────────────────────
// update.rs — System update (lib/update.sh)
//
// Shows checkupdates output, offers database refreshes and full
// upgrades (pacman or AUR helper), gated by CONFIRM_ACTIONS.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::Instant;

use crate::app::{App, Screen};
use crate::sys::{self, Job};
use crate::widgets::{self, Menu, Sev};

const MENU_ITEMS: [&str; 5] = [
    "🔄 Refresh databases only (pacman -Sy)",
    "🔄 Force refresh databases (pacman -Syy)",
    "⬆  Full system upgrade — pacman (pacman -Syu)",
    "🌟 Full upgrade — AUR + official (yay/paru -Syu)",
    "🔙 Back to Main Menu",
];

#[derive(Clone, Copy)]
enum UpdOp {
    Sy,
    Syy,
    Syu,
    Aur,
}

impl UpdOp {
    #[allow(dead_code)]
    fn tag(self) -> &'static str {
        match self {
            UpdOp::Sy => "upd-sy",
            UpdOp::Syy => "upd-syy",
            UpdOp::Syu => "upd-syu",
            UpdOp::Aur => "upd-aur",
        }
    }
}

pub struct UpdateScreen {
    menu: Menu,
    updates: Option<Vec<String>>,
    job: Option<Job<Vec<String>>>,
    pending: Option<UpdOp>,
}

impl UpdateScreen {
    pub fn new() -> Self {
        Self {
            menu: Menu::new("Update actions", MENU_ITEMS.map(str::to_string).to_vec()),
            updates: None,
            job: Some(Job::spawn("Checking for updates...", || {
                sys::checkupdates_lines()
            })),
            pending: None,
        }
    }

    fn refresh_updates(&mut self) {
        self.job = Some(Job::spawn("Checking for updates...", || {
            sys::checkupdates_lines()
        }));
    }

    fn run_op(&mut self, app: &mut App, op: UpdOp) {
        match op {
            UpdOp::Sy => {
                let tx = crate::tx::TransactionSpec::refresh_db(app.settings(), false);
                app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
            }
            UpdOp::Syy => {
                let tx = crate::tx::TransactionSpec::refresh_db(app.settings(), true);
                app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
            }
            UpdOp::Syu => {
                let tx = crate::tx::TransactionSpec::upgrade_pacman(app.settings());
                app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
            }
            UpdOp::Aur => {
                let Some(helper) = app.aur_helper() else {
                    app.toast(
                        "No AUR helper found. Install yay or paru first.",
                        Sev::Error,
                    );
                    app.toast(
                        "You can install one via Settings > Change AUR Helper.",
                        Sev::Info,
                    );
                    return;
                };
                let tx = crate::tx::TransactionSpec::upgrade_aur(app.settings(), &helper);
                app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
            }
        }
    }
}

impl Default for UpdateScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for UpdateScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.menu.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let op = match self.menu.selected {
                    0 => Some(UpdOp::Sy),
                    1 => Some(UpdOp::Syy),
                    2 => Some(UpdOp::Syu),
                    3 => Some(UpdOp::Aur),
                    _ => None,
                };
                match op {
                    Some(op) => self.start_with_confirm(app, op),
                    None => app.pop(),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn poll(&mut self, _app: &mut App) {
        if let Some(mut job) = self.job.take() {
            match job.poll() {
                Some(lines) => self.updates = Some(lines),
                None => self.job = Some(job),
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let rows = Layout::vertical([Constraint::Percentage(55), Constraint::Fill(1)]).split(area);

        // Status panel.
        let block = widgets::panel("⬆  System Update");
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let mut lines: Vec<Line> = Vec::new();
        match &self.updates {
            None => {
                lines.push(Line::from(widgets::span(
                    "Checking for updates...",
                    widgets::dim(),
                )));
            }
            Some(updates) if updates.is_empty() => {
                lines.push(Line::from(widgets::span(
                    "✔ System is up to date!",
                    widgets::success(),
                )));
            }
            Some(updates) => {
                let icon = widgets::span("⚠ ", widgets::warning());
                let msg = widgets::span(
                    format!("{} package update(s) available.", updates.len()),
                    widgets::warning(),
                );
                lines.push(Line::from(vec![icon, msg]));
                for u in updates.iter().take(20) {
                    lines.push(Line::from(Span::styled(
                        format!("  {u}"),
                        Style::new().fg(widgets::ACCENT),
                    )));
                }
                if updates.len() > 20 {
                    lines.push(Line::from(widgets::span(
                        format!("  ... and {} more", updates.len() - 20),
                        widgets::dim(),
                    )));
                }
            }
        }
        if !crate::sys::has_bin("checkupdates") {
            lines.push(Line::from(widgets::span(
                "(checkupdates not found — install pacman-contrib to list updates)",
                widgets::dim(),
            )));
        }
        f.render_widget(Paragraph::new(lines), inner);

        self.menu.render(f, rows[1]);
    }

    fn busy(&self) -> Option<(String, Instant)> {
        self.job.as_ref().map(|j| (j.label.clone(), j.started))
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓ navigate", "enter select", "esc back"]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(op) = self.pending.take() else {
            return;
        };
        if !yes {
            app.toast("Update cancelled.", Sev::Info);
            return;
        }
        self.run_op(app, op);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        let (ok_msg, fail_msg, ok_log, fail_log) = match tag {
            "upd-sy" => (
                "Package databases refreshed!",
                "Database refresh failed.",
                "UPDATE: Database refresh successful",
                "UPDATE: Database refresh failed",
            ),
            "upd-syy" => (
                "Package databases force refreshed!",
                "Force database refresh failed.",
                "UPDATE: Force database refresh successful",
                "UPDATE: Force database refresh failed",
            ),
            "upd-syu" => (
                "System upgrade complete!",
                "System upgrade failed.",
                "UPDATE: System upgrade successful",
                "UPDATE: System upgrade failed",
            ),
            "upd-aur" => (
                "Full system upgrade complete!",
                "Upgrade failed.",
                "UPDATE: Full upgrade via AUR helper successful",
                "UPDATE: Full upgrade via AUR helper failed",
            ),
            _ => return,
        };
        if ok {
            app.toast(ok_msg, Sev::Success);
            app.log(ok_log);
        } else {
            app.toast(fail_msg, Sev::Error);
            app.log(fail_log);
        }
        self.refresh_updates();
    }
}

impl UpdateScreen {
    /// Route through CONFIRM_ACTIONS for the upgrade operations.
    fn start_with_confirm(&mut self, app: &mut App, op: UpdOp) {
        let needs_confirm =
            matches!(op, UpdOp::Syu | UpdOp::Aur) && app.settings().is_true("CONFIRM_ACTIONS");
        if needs_confirm {
            let prompt = match op {
                UpdOp::Syu => "Perform full system upgrade?",
                _ => match app.aur_helper() {
                    Some(h) => {
                        self.pending = Some(op);
                        app.confirm(
                            format!("Perform full upgrade (official + AUR) via {h}?"),
                            false,
                        );
                        return;
                    }
                    None => "",
                },
            };
            self.pending = Some(op);
            app.confirm(prompt, false);
        } else {
            self.run_op(app, op);
        }
    }
}
