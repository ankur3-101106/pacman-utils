// ──────────────────────────────────────────────────────────────────────
// app.rs — Application core
//
// Owns the screen stack, modal dialogs (confirm/input), toast messages,
// and the queue of interactive external commands, which run on a
// pseudo-terminal inside the dashboard's action pane.
// ──────────────────────────────────────────────────────────────────────

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::{
    settings::{log_action, Settings},
    sys::Caps,
    widgets::{self, centered_rect, help_bar, panel, spinner, Sev},
};

/// Anything renderable as a full-screen pane in the stack.
pub trait Screen {
    /// Handle a key press. Use `app.pop()` / `app.push()` / `app.toast()`…
    /// to change application state.
    fn handle_key(&mut self, app: &mut App, key: KeyEvent);

    /// Called every loop tick so background jobs can deliver results.
    fn poll(&mut self, _app: &mut App) {}

    /// Render into the content area.
    fn draw(&mut self, _f: &mut Frame<'_>, _area: Rect) {}

    /// While Some, a spinner overlay shows with this label/start time.
    fn busy(&self) -> Option<(String, Instant)> {
        None
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓ navigate", "enter select", "esc back"]
    }

    /// Area where an embedded command runner should render. Defaults to
    /// the full content area; the dashboard narrows it to its action pane.
    fn content_area(&self, full: Rect) -> Rect {
        full
    }

    /// Modal dialog resolutions route here.
    fn on_confirm(&mut self, _app: &mut App, _yes: bool) {}
    fn on_input(&mut self, _app: &mut App, _value: String) {}

    /// A queued external command finished; `ok` is its exit status.
    fn on_ext_done(&mut self, _app: &mut App, _tag: &str, _ok: bool) {}
}

// ── External command spec ───────────────────────────────────────────

#[allow(unused_imports)]
pub use crate::cmd::{
    CommandEngine, CommandError, CommandMode, CommandResult, CommandSpec, CommandStatus, ExtCmd,
};

// ── Modals ──────────────────────────────────────────────────────────

#[derive(Clone)]
enum Modal {
    Confirm { prompt: String, danger: bool },
    Input { prompt: String },
}

struct Toast {
    msg: String,
    sev: Sev,
    born: Instant,
}

const TOAST_TTL: Duration = Duration::from_secs(4);

// ── App ─────────────────────────────────────────────────────────────

pub struct App {
    screens: Vec<Box<dyn Screen>>,
    modal: Option<Modal>,
    /// Text typed into the current Input modal.
    input_buffer: String,
    toast: Option<Toast>,
    /// Queued external commands waiting for an embedded run pane.
    pending: VecDeque<ExtCmd>,
    /// The live embedded runner (rendered inside the dashboard pane).
    embedded: Option<Box<crate::screens::runpane::RunPane>>,
    /// Result feedback for the embedded command (ExtCmd::result).
    embedded_msgs: Option<(String, String, String)>,
    /// True while a run pane is executing a command.
    ext_active: bool,
    /// Result of the last finished external command, routed to the
    /// screen that queued it via [`Screen::on_ext_done`].
    last_ext_result: Option<(String, bool)>,
    popped_during_dispatch: bool,
    /// Stack depth as seen by the screen currently being dispatched
    /// (the top is temporarily lifted, so `screens.len()` lies).
    dispatch_depth: usize,
    /// Shortcut cheatsheet overlay (? key).
    help_open: bool,
    pub quit: bool,
    settings: Settings,
    caps: Caps,
    engine: CommandEngine,
    preflight_env: Option<crate::tx::PreflightEnv>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let settings = Settings::init();
        let caps = Caps::detect();
        let engine = CommandEngine::default();
        let mut app = Self {
            screens: Vec::new(),
            modal: None,
            input_buffer: String::new(),
            toast: None,
            pending: VecDeque::new(),
            embedded: None,
            embedded_msgs: None,
            ext_active: false,
            last_ext_result: None,
            popped_during_dispatch: false,
            dispatch_depth: 1,
            help_open: false,
            quit: false,
            settings,
            caps,
            engine,
            preflight_env: None,
        };
        log_action(
            &app.settings,
            &format!("SESSION: archman v{} started", crate::VERSION),
        );
        app.push(Box::new(crate::screens::HomeScreen::new()));
        app
    }

    // ── Accessors ───────────────────────────────────────────────────

    #[allow(dead_code)]
    pub fn engine(&self) -> &CommandEngine {
        &self.engine
    }

    #[allow(dead_code)]
    pub fn set_preflight_env(&mut self, env: crate::tx::PreflightEnv) {
        self.preflight_env = Some(env);
    }

    pub fn preflight_env(&self) -> Option<&crate::tx::PreflightEnv> {
        self.preflight_env.as_ref()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    pub fn refresh_caps(&mut self) {
        self.caps = Caps::detect();
    }

    pub fn aur_helper(&self) -> Option<String> {
        crate::sys::get_aur_helper(&self.settings)
    }

    pub fn log(&self, msg: &str) {
        log_action(&self.settings, msg);
    }

    // ── Screen stack ────────────────────────────────────────────────

    pub fn push(&mut self, screen: Box<dyn Screen>) {
        self.screens.push(screen);
    }

    /// Close the top screen; quitting if it was the last one.
    pub fn pop(&mut self) {
        if self.dispatch_depth <= 1 {
            self.quit = true;
        } else {
            self.popped_during_dispatch = true;
        }
    }

    /// Temporarily lift the top screen, run `f`, then put it back in
    /// place — unless it removed itself via [`App::pop`]. Screens pushed
    /// during `f` land ON TOP of the restored one.
    fn dispatch(&mut self, f: impl FnOnce(&mut Box<dyn Screen>, &mut App)) {
        let depth_before = self.screens.len();
        self.dispatch_depth = depth_before;
        self.popped_during_dispatch = false;
        let Some(mut top) = self.screens.pop() else {
            return;
        };
        f(&mut top, self);
        if !self.popped_during_dispatch {
            // Insert back at its original position so anything pushed
            // during the call stays above it.
            let pos = depth_before - 1;
            self.screens.insert(pos, top);
        }
    }

    // ── Feedback & actions ──────────────────────────────────────────

    pub fn toast(&mut self, msg: impl Into<String>, sev: Sev) {
        self.toast = Some(Toast {
            msg: msg.into(),
            sev,
            born: Instant::now(),
        });
    }

    pub fn confirm(&mut self, prompt: impl Into<String>, danger: bool) {
        self.modal = Some(Modal::Confirm {
            prompt: prompt.into(),
            danger,
        });
    }

    pub fn ask_input(&mut self, prompt: impl Into<String>) {
        self.input_buffer.clear();
        self.modal = Some(Modal::Input {
            prompt: prompt.into(),
        });
    }

    #[allow(dead_code)]
    pub fn has_modal(&self) -> bool {
        self.modal.is_some()
    }

    pub fn modal_prompt(&self) -> Option<&str> {
        match &self.modal {
            Some(Modal::Confirm { prompt, .. }) | Some(Modal::Input { prompt }) => Some(prompt),
            None => None,
        }
    }

    #[allow(dead_code)]
    pub fn pending_cmd(&self) -> Option<&ExtCmd> {
        self.pending.front()
    }

    #[allow(dead_code)]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Queue an interactive external command. Executed right after the
    /// next frame with the terminal handed over to the child process.
    pub fn queue_ext(&mut self, cmd: ExtCmd) {
        self.pending.push_back(cmd);
    }

    pub fn toggle_help(&mut self) {
        self.help_open = !self.help_open;
    }

    /// Called by a run pane when its command finished (or was closed):
    /// unblocks the queue and schedules result delivery.
    pub fn complete_ext(&mut self, tag: &str, ok: bool) {
        self.refresh_caps();
        self.last_ext_result = Some((tag.to_string(), ok));
        self.ext_active = false;
    }

    pub fn quit_app(&mut self) {
        self.quit = true;
    }

    /// True while a confirm/input dialog is on screen.
    pub fn modal_open(&self) -> bool {
        self.modal.is_some()
    }

    // ── Input routing ───────────────────────────────────────────────

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        // The cheatsheet overlay swallows one key to close itself.
        if self.help_open {
            self.help_open = false;
            return;
        }

        // Embedded runner gets all keys while active; any key closes it
        // once the command finished, routing the result.
        if let Some(pane) = self.embedded.as_mut() {
            if pane.is_running() {
                pane.handle_key(key);
            } else {
                let tag = pane.tag().to_string();
                let ok = pane.result().unwrap_or(false);
                self.embedded = None;
                self.complete_ext(&tag, ok);
            }
            return;
        }

        if self.modal.is_some() {
            self.handle_modal_key(key);
            return;
        }

        self.dispatch(|screen, app| screen.handle_key(app, key));
    }

    fn handle_modal_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        let Some(modal) = &self.modal else { return };
        match modal {
            Modal::Confirm { .. } => {
                let yes = match key.code {
                    Char('y') | Char('Y') | Enter | Left => true,
                    Char('n') | Char('N') | Esc | Char('q') | Right => false,
                    _ => return,
                };
                self.modal = None;
                self.dispatch(|screen, app| screen.on_confirm(app, yes));
            }
            Modal::Input { .. } => {
                // Empty value mirrors bash ui_input semantics: cancel.
                let mut resolved: Option<String> = None;
                match key.code {
                    Esc => resolved = Some(String::new()),
                    Enter => resolved = Some(self.input_buffer.clone()),
                    Backspace => {
                        self.input_buffer.pop();
                    }
                    Char(c) => self.input_buffer.push(c),
                    _ => {}
                }
                if let Some(v) = resolved {
                    self.modal = None;
                    self.dispatch(|screen, app| screen.on_input(app, v));
                }
            }
        }
    }

    // ── Frame ───────────────────────────────────────────────────────

    fn draw(&mut self, f: &mut Frame<'_>) {
        let rows = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1), // help bar
        ])
        .split(f.area());

        // The dashboard (bottom screen) is the permanent shell: always
        // paint it, then render the active screen — and any embedded
        // command runner — inside its content pane (linutil style).
        let base_area = {
            let shell = &mut self.screens[0];
            shell.draw(f, rows[0]);
            shell.content_area(rows[0])
        };
        if self.screens.len() > 1 {
            // Erase the dashboard's action rows first — overlays draw
            // partial content and must not bleed through.
            f.render_widget(Clear, base_area);
            if let Some(top) = self.screens.last_mut() {
                top.draw(f, base_area);
            }
        }

        // Embedded command runner shares the same pane.
        if let Some(pane) = self.embedded.as_mut() {
            pane.draw(f, base_area);
        }

        // Toasts print inside the pane (bottom row), per user request —
        // not at the bottom of the window.
        if let Some(t) = &self.toast {
            let row = Rect {
                x: base_area.x,
                y: base_area.bottom().saturating_sub(1),
                width: base_area.width,
                height: 1,
            };
            f.render_widget(Clear, row);
            let spans = Line::from(vec![
                Span::styled(format!(" {} ", t.sev.icon()), t.sev.style()),
                Span::styled(t.msg.clone(), t.sev.style()),
            ]);
            f.render_widget(Paragraph::new(spans), row);
        }

        let mut hints: Vec<&'static str> = Vec::new();
        self.dispatch(|screen, _| hints = screen.help_hints());
        help_bar(f, rows[1], &hints);

        let mut busy: Option<(String, Instant)> = None;
        self.dispatch(|screen, _| busy = screen.busy());
        if let Some((label, start)) = busy {
            const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let ms = start.elapsed().as_millis() / 100;
            spinner(f, FRAMES[(ms % 10) as usize], &label);
        }

        self.draw_modal(f);

        // Shortcuts cheatsheet on top of everything.
        if self.help_open {
            widgets::render_help_overlay(f);
        }
    }

    fn draw_modal(&mut self, f: &mut Frame<'_>) {
        match self.modal.clone() {
            Some(Modal::Confirm { prompt, danger }) => {
                let area = centered_rect(60, 9, f.area());
                f.render_widget(Clear, area);
                let style = if danger {
                    widgets::danger()
                } else {
                    widgets::warning()
                };
                let block = Block::new()
                    .borders(Borders::ALL)
                    .border_style(style)
                    .title(Span::styled(" Confirm ", style));
                let inner = block.inner(area);
                f.render_widget(block, area);

                let rows =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(inner);
                f.render_widget(
                    Paragraph::new(Line::from(prompt)).wrap(Wrap { trim: true }),
                    rows[0],
                );
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled("[y] Yes   [n] No", widgets::dim())))
                        .alignment(Alignment::Center),
                    rows[1],
                );
            }
            Some(Modal::Input { .. }) => {
                let area = centered_rect(70, 9, f.area());
                f.render_widget(Clear, area);
                let block = panel(" Input ");
                let inner = block.inner(area);
                f.render_widget(block, area);

                let rows = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ])
                .split(inner);

                let prompt = self.modal_prompt().unwrap_or_default();
                let value = self.modal_value();
                f.render_widget(Paragraph::new(Line::from(prompt)), rows[0]);
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!("{value}▏"),
                        widgets::accent_bold(),
                    ))),
                    rows[1],
                );
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "enter confirm · esc cancel",
                        widgets::dim(),
                    )))
                    .alignment(Alignment::Center),
                    rows[2],
                );
            }
            None => {}
        }
    }

    fn modal_value(&self) -> String {
        self.input_buffer.clone()
    }

    // ── Main Loop ───────────────────────────────────────────────────

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;

            // Launch the next queued command inside an embedded pane.
            // Serialized: one run pane at a time.
            if !self.ext_active {
                if let Some(cmd) = self.pending.pop_front() {
                    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
                    // Commands always run on the dashboard's side pane:
                    // return there no matter which screen queued them.
                    self.screens.truncate(1);
                    self.embedded_msgs = match (&cmd.ok_msg, &cmd.fail_msg, &cmd.done_log) {
                        (Some(o), Some(f), Some(l)) => Some((o.clone(), f.clone(), l.clone())),
                        _ => None,
                    };
                    match crate::screens::runpane::RunPane::with_engine(
                        &self.engine,
                        &cmd,
                        rows,
                        cols,
                    ) {
                        Ok(pane) => {
                            self.ext_active = true;
                            self.embedded = Some(pane);
                        }
                        Err(e) => {
                            let tag = cmd.tag.clone();
                            self.toast(
                                format!("Failed to run {}: {e}", cmd.program),
                                widgets::Sev::Error,
                            );
                            self.dispatch(move |screen, app| screen.on_ext_done(app, &tag, false));
                        }
                    }
                }
            }

            // Deliver a finished command's result: ExtCmd::result
            // messages are handled app-level; everything else routes to
            // the queuing screen (dashboard RunSpecs).
            if let Some((tag, ok)) = self.last_ext_result.take() {
                if let Some((ok_msg, fail_msg, done_log)) = self.embedded_msgs.take() {
                    if ok {
                        self.toast(ok_msg, widgets::Sev::Success);
                    } else {
                        self.toast(fail_msg, widgets::Sev::Error);
                    }
                    self.log(&done_log);
                } else {
                    let tag2 = tag.clone();
                    self.dispatch(move |screen, app| screen.on_ext_done(app, &tag2, ok));
                    let _ = tag;
                }
            }

            if self.quit {
                break;
            }

            if let Some(pane) = self.embedded.as_mut() {
                pane.poll();
            }

            self.dispatch(|screen, app| screen.poll(app));

            if crossterm::event::poll(Duration::from_millis(40))? {
                match crossterm::event::read()? {
                    Event::Key(k) if k.kind == crossterm::event::KeyEventKind::Press => {
                        self.on_key(k)
                    }
                    _ => {}
                }
            }

            if self
                .toast
                .as_ref()
                .is_some_and(|t| t.born.elapsed() > TOAST_TTL)
            {
                self.toast = None;
            }
        }

        log_action(&self.settings, "SESSION: archman exited");
        Ok(())
    }
}

#[cfg(test)]
mod bleed_tests {
    use super::*;
    use crate::screens::viewer::ViewerScreen;
    use ratatui::backend::TestBackend;

    fn text_of(term: &ratatui::Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    /// Regression: a pushed screen must erase the dashboard's action
    /// rows in the shared pane — no label tails bleeding through.
    #[test]
    fn pushed_screen_clears_dashboard_pane() {
        let mut app = App::new();
        let mut term = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();

        // Dashboard alone: action labels visible.
        term.draw(|f| app.draw(f)).unwrap();
        assert!(
            text_of(&term).contains("Install Package"),
            "dashboard should list actions"
        );

        // Push an overlay with distinctive content.
        app.push(ViewerScreen::from_text(
            "OVERLAY",
            "XYZOVERLAY-CONTENT\nsecond line",
        ));
        term.draw(|f| app.draw(f)).unwrap();
        let text = text_of(&term);
        assert!(text.contains("XYZOVERLAY-CONTENT"), "overlay not drawn");
        assert!(
            !text.contains("Install Package"),
            "dashboard action rows bled through the overlay"
        );
        // The shell (sidebar) must persist around the overlay.
        assert!(text.contains("Categories"), "shell lost");
    }

    /// Toasts render inside the pane's bottom row, not the window edge.
    #[test]
    fn toast_renders_inside_content_pane() {
        let mut app = App::new();
        app.toast("XYZTOAST-MESSAGE", widgets::Sev::Error);
        let mut term = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("XYZTOAST-MESSAGE"), "toast missing");

        // The toast row must sit INSIDE the content area (above the
        // command list), not on the terminal's last row.
        let row = buf
            .content
            .iter()
            .position(|c| c.symbol().contains("X"))
            .map(|i| i / buf.area.width as usize)
            .unwrap();
        assert!(
            row < buf.area.height as usize - 1,
            "toast on window bottom row"
        );
    }
}
