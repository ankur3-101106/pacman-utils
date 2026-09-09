// ──────────────────────────────────────────────────────────────────────
// screens/preview.rs — Preflight checks and transaction preview screen
//
// Part of v2.1 Point 4: Preflight + Transaction Preview.
//
// Displays:
// 1. Transaction metadata (operation, manager, targets, command)
// 2. Preflight verification results (pass/warn/block)
// 3. Non-destructive package manager preview output (or limitations note)
// 4. Action status: BLOCKED, DRY RUN, or READY
//
// Ensures:
// - BLOCK always prevents execution, regardless of CONFIRM_ACTIONS.
// - DRY_RUN never executes the real transaction.
// - PreviewScreen itself never executes processes directly.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Frame,
};

use crate::{
    app::{App, Screen},
    cmd::CommandEngine,
    tx::{run_preflight, PreflightEnv, PreflightResult, PreflightStatus, TransactionSpec},
    widgets::{self, Sev},
};

pub struct PreviewScreen {
    tx: TransactionSpec,
    preflight: PreflightResult,
    preview_lines: Vec<String>,
    preview_note: Option<String>,
    scroll_offset: usize,
}

impl PreviewScreen {
    /// Construct a new PreviewScreen by evaluating preflight checks and generating
    /// non-destructive preview output using the centralized CommandEngine.
    pub fn new(tx: TransactionSpec, engine: &CommandEngine) -> Self {
        let env = PreflightEnv::system(engine);
        Self::with_env(tx, engine, &env)
    }

    /// Construct a PreviewScreen with a specific preflight environment.
    pub fn with_env(tx: TransactionSpec, engine: &CommandEngine, env: &PreflightEnv) -> Self {
        let preflight = run_preflight(&tx, env);
        let preview = tx.generate_preview(engine);

        Self {
            tx,
            preflight,
            preview_lines: preview.lines,
            preview_note: preview.note,
            scroll_offset: 0,
        }
    }

    /// Construct a PreviewScreen from an App, using the app's injected environment if set (e.g. in tests).
    pub fn from_app(tx: TransactionSpec, app: &App) -> Self {
        if let Some(env) = app.preflight_env() {
            Self::with_env(tx, app.engine(), env)
        } else {
            Self::new(tx, app.engine())
        }
    }

    #[cfg(test)]
    pub fn with_preflight(
        tx: TransactionSpec,
        preflight: PreflightResult,
        preview_lines: Vec<String>,
        preview_note: Option<String>,
    ) -> Self {
        Self {
            tx,
            preflight,
            preview_lines,
            preview_note,
            scroll_offset: 0,
        }
    }

    #[allow(dead_code)]
    pub fn transaction(&self) -> &TransactionSpec {
        &self.tx
    }

    #[allow(dead_code)]
    pub fn preflight(&self) -> &PreflightResult {
        &self.preflight
    }

    #[allow(dead_code)]
    pub fn preview_lines(&self) -> &[String] {
        &self.preview_lines
    }

    #[allow(dead_code)]
    pub fn is_blocked(&self) -> bool {
        self.preflight.is_blocked()
    }

    fn proceed(&mut self, app: &mut App) {
        if self.preflight.is_blocked() {
            app.toast(
                "Transaction is BLOCKED by preflight checks. Resolve issues before proceeding.",
                Sev::Error,
            );
            return;
        }

        if app.settings().is_true("DRY_RUN") {
            app.toast(
                "DRY_RUN mode active: preview simulation complete. No changes applied.",
                Sev::Info,
            );
            app.pop();
            return;
        }

        if app.settings().is_true("CONFIRM_ACTIONS") {
            let prompt = match self.tx.operation {
                crate::tx::OperationKind::Install => {
                    format!("Install {}?", self.tx.targets.join(", "))
                }
                crate::tx::OperationKind::Remove => {
                    if self.tx.command.args.iter().any(|a| a.contains('n')) {
                        format!(
                            "Completely remove {} (including configs)?",
                            self.tx.targets.join(", ")
                        )
                    } else if self.tx.command.args.iter().any(|a| a.contains('s')) {
                        format!(
                            "Remove {} with unused dependencies?",
                            self.tx.targets.join(", ")
                        )
                    } else {
                        format!("Remove {}?", self.tx.targets.join(", "))
                    }
                }
                crate::tx::OperationKind::Reinstall => {
                    format!("Reinstall {}?", self.tx.targets.join(", "))
                }
                crate::tx::OperationKind::RemoveOrphans => {
                    format!("Remove {} orphan package(s)?", self.tx.targets.len())
                }
                crate::tx::OperationKind::Upgrade => {
                    "Proceed with full system upgrade?".to_string()
                }
                crate::tx::OperationKind::RefreshDb => "Refresh package databases?".to_string(),
                crate::tx::OperationKind::CleanCache => "Clean package cache?".to_string(),
                _ => format!("Execute {}?", self.tx.command.note),
            };
            let danger = matches!(
                self.tx.operation,
                crate::tx::OperationKind::Remove | crate::tx::OperationKind::RemoveOrphans
            );
            app.confirm(prompt, danger);
        } else {
            let cmd = self.tx.command.clone();
            app.pop();
            app.queue_ext(cmd);
        }
    }
}

impl Screen for PreviewScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                app.pop();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
            }
            KeyCode::End => {
                self.scroll_offset = self.preview_lines.len().saturating_sub(5);
            }
            KeyCode::Enter | KeyCode::Char('c') => {
                self.proceed(app);
            }
            _ => {}
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        if yes {
            let cmd = self.tx.command.clone();
            app.pop();
            app.queue_ext(cmd);
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        f.render_widget(Clear, area);

        // Divide area into:
        // 1. Transaction Overview panel (height 6)
        // 2. Preflight Checks panel (height: checks count + 2, min 6, max 10)
        // 3. Preview Output panel (remaining space)
        let check_count = self.preflight.checks.len() as u16;
        let checks_height = (check_count + 2).clamp(6, 11);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Length(checks_height),
                Constraint::Min(8),
            ])
            .split(area);

        // 1. Transaction Overview
        let is_blocked = self.preflight.is_blocked();
        let has_warn = self.preflight.has_warnings();

        let status_badge = if is_blocked {
            "✖ BLOCKED BY PREFLIGHT"
        } else if has_warn {
            "⚠ READY WITH WARNINGS"
        } else {
            "✔ PREFLIGHT PASSED"
        };

        let overview_title = format!(" Transaction Overview — [{status_badge}] ");
        let border_color = if is_blocked {
            Color::Red
        } else if has_warn {
            Color::Yellow
        } else {
            widgets::ACCENT
        };

        let overview_block = widgets::panel_color(&overview_title, border_color);
        let inner_overview = overview_block.inner(chunks[0]);
        f.render_widget(overview_block, chunks[0]);

        let targets_str = if self.tx.targets.is_empty() {
            "(system-wide)".to_string()
        } else {
            self.tx.targets.join(", ")
        };

        let cmd_str = format!(
            "{} {}",
            self.tx.command.program,
            self.tx.command.args.join(" ")
        );

        let overview_lines = vec![
            Line::from(vec![
                Span::styled("  Operation:     ", widgets::accent_bold()),
                Span::styled(
                    format!("{}", self.tx.operation),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::styled("    Manager: ", widgets::accent_bold()),
                Span::styled(format!("{}", self.tx.manager), Style::new()),
                Span::styled("    Requires Root: ", widgets::accent_bold()),
                Span::styled(
                    if self.tx.requires_root { "Yes" } else { "No" },
                    Style::new(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Targets:       ", widgets::accent_bold()),
                Span::styled(targets_str, Style::new().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("  Command:       ", widgets::accent_bold()),
                Span::styled(cmd_str, widgets::dim()),
            ]),
            Line::from(vec![
                Span::styled("  Description:   ", widgets::accent_bold()),
                Span::styled(self.tx.command.note.clone(), Style::new().fg(Color::Cyan)),
            ]),
        ];
        f.render_widget(Paragraph::new(overview_lines), inner_overview);

        // 2. Preflight Checks Panel
        let checks_block = widgets::panel(" Preflight Verification ");
        let inner_checks = checks_block.inner(chunks[1]);
        f.render_widget(checks_block, chunks[1]);

        let mut check_lines: Vec<Line> = Vec::new();
        for check in &self.preflight.checks {
            let (icon, style) = match check.status {
                PreflightStatus::Pass => (" ✔ PASS ", widgets::success()),
                PreflightStatus::Warning => (" ⚠ WARN ", widgets::warning()),
                PreflightStatus::Block => (" ✖ BLOCK", widgets::danger()),
            };

            check_lines.push(Line::from(vec![
                Span::styled(icon, style),
                Span::styled(format!(" {:<18} ", check.name), widgets::accent_bold()),
                Span::styled(&check.message, widgets::dim()),
            ]));
        }
        f.render_widget(Paragraph::new(check_lines), inner_checks);

        // 3. Preview Output Panel
        let preview_title = " Package Manager Preview (Non-Destructive) ";
        let preview_block = widgets::panel(preview_title);
        let inner_preview = preview_block.inner(chunks[2]);
        f.render_widget(preview_block, chunks[2]);

        let mut out_lines: Vec<Line> = Vec::new();

        if let Some(note) = &self.preview_note {
            out_lines.push(Line::from(vec![
                Span::styled("ℹ Note: ", widgets::warning()),
                Span::styled(note, widgets::warning()),
            ]));
            out_lines.push(Line::from(""));
        }

        if self.preview_lines.is_empty() {
            out_lines.push(Line::from(vec![Span::styled(
                "  (No preview output returned by package manager)",
                widgets::dim(),
            )]));
        } else {
            for (idx, line) in self.preview_lines.iter().enumerate() {
                out_lines.push(Line::from(vec![
                    Span::styled(format!("  {:>3} │ ", idx + 1), widgets::dim()),
                    Span::styled(line.clone(), Style::new().fg(Color::Indexed(252))),
                ]));
            }
        }

        let max_scroll = out_lines
            .len()
            .saturating_sub(inner_preview.height as usize);
        let effective_scroll = self.scroll_offset.min(max_scroll);

        f.render_widget(
            Paragraph::new(out_lines).scroll((effective_scroll as u16, 0)),
            inner_preview,
        );
    }

    fn help_hints(&self) -> Vec<&'static str> {
        if self.preflight.is_blocked() {
            vec!["↑↓ scroll preview", "esc cancel", "✖ execution blocked"]
        } else {
            vec!["↑↓ scroll preview", "enter execute", "esc cancel"]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::CommandSpec;
    use crate::tx::{OperationKind, PackageManager, PreflightCheck, PreflightStatus};

    fn dummy_tx() -> TransactionSpec {
        TransactionSpec {
            operation: OperationKind::Install,
            manager: PackageManager::Pacman,
            command: CommandSpec::new(
                "test-install",
                "sudo",
                &[
                    "pacman".to_string(),
                    "-S".to_string(),
                    "ripgrep".to_string(),
                ],
            ),
            targets: vec!["ripgrep".to_string()],
            requires_root: true,
            requires_network: true,
            preview_cmd: None,
            preview_note: None,
        }
    }

    #[test]
    fn test_preview_screen_blocked_prevents_proceed() {
        let mut app = App::new();
        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "Database Lock".to_string(),
                status: PreflightStatus::Block,
                message: "Lock active — /var/lib/pacman/db.lck exists".to_string(),
            }],
        };

        let mut screen = PreviewScreen::with_preflight(tx, preflight, vec![], None);
        assert!(screen.is_blocked());

        // Pressing Enter should not queue any command
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.pending_len(), 0);
        assert!(!app.has_modal());
    }

    #[test]
    fn test_preview_screen_dry_run_does_not_execute() {
        let mut app = App::new();
        app.settings_mut().set("DRY_RUN", "true");
        assert!(app.settings().is_true("DRY_RUN"));

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "All checks".to_string(),
                status: PreflightStatus::Pass,
                message: "OK".to_string(),
            }],
        };

        let mut screen =
            PreviewScreen::with_preflight(tx, preflight, vec!["dry run test".to_string()], None);
        assert!(!screen.is_blocked());

        // Pressing Enter with DRY_RUN should pop without queueing
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.pending_len(), 0);
        assert!(!app.has_modal());
    }

    #[test]
    fn test_preview_screen_confirm_actions_routes_modal() {
        let mut app = App::new();
        app.settings_mut().set("CONFIRM_ACTIONS", "true");
        app.settings_mut().set("DRY_RUN", "false");

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "All checks".to_string(),
                status: PreflightStatus::Pass,
                message: "OK".to_string(),
            }],
        };

        let mut screen = PreviewScreen::with_preflight(tx, preflight, vec![], None);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));

        // Should open confirmation modal without queueing yet
        assert!(app.has_modal());
        assert_eq!(app.pending_len(), 0);

        // Confirming yes should queue the command
        screen.on_confirm(&mut app, true);
        assert_eq!(app.pending_len(), 1);
        assert_eq!(
            app.pending_cmd().map(|c| c.tag.as_str()),
            Some("test-install")
        );
    }

    #[test]
    fn test_preview_screen_no_confirm_queues_immediately() {
        let mut app = App::new();
        app.settings_mut().set("CONFIRM_ACTIONS", "false");
        app.settings_mut().set("DRY_RUN", "false");

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "All checks".to_string(),
                status: PreflightStatus::Pass,
                message: "OK".to_string(),
            }],
        };

        let mut screen = PreviewScreen::with_preflight(tx, preflight, vec![], None);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));

        // Should NOT open modal, but immediately queue command
        assert!(!app.has_modal());
        assert_eq!(app.pending_len(), 1);
        assert_eq!(
            app.pending_cmd().map(|c| c.tag.as_str()),
            Some("test-install")
        );
    }

    #[test]
    fn test_preview_screen_scroll_keys() {
        let tx = dummy_tx();
        let preflight = PreflightResult { checks: vec![] };
        let lines: Vec<String> = (0..50).map(|i| format!("preview line {i}")).collect();

        let mut screen = PreviewScreen::with_preflight(tx, preflight, lines, None);
        let mut app = App::new();

        assert_eq!(screen.scroll_offset, 0);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Down));
        assert_eq!(screen.scroll_offset, 1);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::PageDown));
        assert_eq!(screen.scroll_offset, 11);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Up));
        assert_eq!(screen.scroll_offset, 10);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Home));
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn test_preview_screen_blocked_prevents_proceed_even_when_confirm_actions_false() {
        let mut app = App::new();
        // Explicitly disable confirmations
        app.settings_mut().set("CONFIRM_ACTIONS", "false");

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "Database Lock".to_string(),
                status: PreflightStatus::Block,
                message: "Lock active".to_string(),
            }],
        };

        let mut screen = PreviewScreen::with_preflight(tx, preflight, vec![], None);
        assert!(screen.is_blocked());

        // Pressing Enter must NOT queue command, even when CONFIRM_ACTIONS=false
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.pending_len(), 0);
        assert!(!app.has_modal());
    }

    #[test]
    fn test_preview_screen_modal_cancellation_preserves_screen_and_queues_nothing() {
        let mut app = App::new();
        app.settings_mut().set("CONFIRM_ACTIONS", "true");

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "Disk Space".to_string(),
                status: PreflightStatus::Pass,
                message: "OK".to_string(),
            }],
        };

        let mut screen = PreviewScreen::with_preflight(tx, preflight, vec![], None);
        // Press enter to trigger modal
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));
        assert!(app.has_modal());

        // Cancel modal by rejecting with false
        screen.on_confirm(&mut app, false);
        assert_eq!(app.pending_len(), 0);
    }

    #[test]
    fn test_preview_screen_small_terminal_area_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let tx = dummy_tx();
        let preflight = PreflightResult {
            checks: vec![PreflightCheck {
                name: "Check".to_string(),
                status: PreflightStatus::Pass,
                message: "OK".to_string(),
            }],
        };
        let mut screen =
            PreviewScreen::with_preflight(tx, preflight, vec!["line1".to_string()], None);

        terminal
            .draw(|f| {
                screen.draw(f, f.area());
            })
            .expect("draw must not panic on tiny areas");
    }
}
