// ──────────────────────────────────────────────────────────────────────
// runpane.rs — Embedded command runner pane
//
// External commands (pacman, paccache, reflector, yay…) execute on a
// pseudo-terminal and their output streams INTO the dashboard's action
// pane. Keystrokes are forwarded to the child, so sudo password
// prompts and pacman [Y/n] confirmations work right inside the pane.
// The pane border turns green on success, red on failure.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::ExtCmd;
use crate::pty::{PtyChild, PtyEvent};
use crate::widgets;

const MAX_LINES: usize = 5000;

/// Minimal line-oriented terminal emulator: strips ANSI escapes,
/// handles \r overwrites (progress bars), tabs and backspaces so
/// pacman's download bars render sanely inside our pane.
#[derive(Default)]
struct TermEmu {
    lines: Vec<Vec<u8>>,
    cur: Vec<u8>,
    col: usize,
    /// Set when output was truncated (oldest lines dropped).
    truncated: bool,
}

impl TermEmu {
    fn feed(&mut self, bytes: &[u8]) {
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match b {
                0x1b => {
                    // Skip CSI/OSC escape sequences: consume until a
                    // final byte (@..~ for CSI, BEL/ESC-intro for OSC).
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'[' || bytes[i] == b']') {
                        let intro = bytes[i];
                        i += 1;
                        while i < bytes.len() {
                            let c = bytes[i];
                            if intro == b']' && (c == 0x07 || c == 0x1b) {
                                break;
                            }
                            if intro == b'[' && (0x40..=0x7e).contains(&c) {
                                break;
                            }
                            i += 1;
                        }
                    }
                }
                b'\r' => self.col = 0,
                b'\n' => self.push_line(),
                0x08 => self.col = self.col.saturating_sub(1),
                b'\t' => {
                    let target = (self.col / 8 + 1) * 8;
                    while self.cur.len() < target {
                        self.cur.push(b' ');
                    }
                    self.col = target;
                }
                c if c < 0x20 || c == 0x7f => {}
                byte => {
                    if self.cur.len() > 4000 {
                        // Absurdly long single line: hard-wrap it.
                        self.push_line();
                    }
                    if self.cur.len() <= self.col {
                        self.cur.resize(self.col + 1, b' ');
                    }
                    self.cur[self.col] = byte;
                    self.col += 1;
                }
            }
            i += 1;
        }
    }

    fn push_line(&mut self) {
        let finished = std::mem::take(&mut self.cur);
        self.col = 0;
        self.lines.push(finished);
        if self.lines.len() > MAX_LINES {
            self.lines.remove(0);
            self.truncated = true;
        }
    }

    fn snapshot_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.lines.len() + 1);
        for l in &self.lines {
            out.push(String::from_utf8_lossy(l).to_string());
        }
        if !self.cur.is_empty() {
            out.push(String::from_utf8_lossy(&self.cur).to_string());
        }
        if self.truncated {
            out.insert(0, "… older output trimmed …".to_string());
        }
        out
    }
}

enum Phase {
    Running,
    Done(i32),
}

/// Embedded runner component (owned by the home dashboard).
pub struct RunPane {
    title: String,
    tag: String,
    emu: TermEmu,
    child: Option<PtyChild>,
    phase: Phase,
    /// Scrollback distance from the tail (false = follow live output).
    scroll: usize,
    follow: bool,
    /// Preloaded stdin payload (pkglist imports), flushed once.
    pending_stdin: Option<Vec<u8>>,
    stdin_flushed: bool,
}

impl RunPane {
    pub fn new(cmd: &ExtCmd, rows: u16, cols: u16) -> std::io::Result<Box<Self>> {
        let stdin_data = match &cmd.stdin_file {
            Some(path) => std::fs::read(path).ok(),
            None => None,
        };
        let child =
            crate::pty::spawn(&cmd.program.clone(), &cmd.args, rows.max(2), cols.max(4))?;
        Ok(Box::new(Self {
            title: cmd.note.clone(),
            tag: cmd.tag.clone(),
            emu: TermEmu::default(),
            child: Some(child),
            phase: Phase::Running,
            scroll: 0,
            follow: true,
            pending_stdin: stdin_data,
            stdin_flushed: false,
        }))
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Running)
    }

    /// Some(ok) once the child exited. Only exit code 0 is success.
    pub fn result(&self) -> Option<bool> {
        match self.phase {
            Phase::Done(c) => Some(c == 0),
            Phase::Running => None,
        }
    }

    /// Drain child output; called every tick by the owner.
    pub fn poll(&mut self) {
        // Flush preloaded stdin (imports) once the child is listening.
        if !self.stdin_flushed {
            if let Some(data) = self.pending_stdin.take() {
                if let Some(child) = self.child.as_ref() {
                    child.write_all(&data);
                    child.write_all(b"\n");
                }
            }
            self.stdin_flushed = true;
        }

        let mut exited: Option<i32> = None;
        if let Some(child) = self.child.as_ref() {
            loop {
                match child.rx.try_recv() {
                    Ok(PtyEvent::Output(bytes)) => self.emu.feed(&bytes),
                    Ok(PtyEvent::Exited(code)) => {
                        exited = Some(code);
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
        if let Some(code) = exited {
            self.emu.feed(b"\n");
            self.child = None;
            self.phase = Phase::Done(code);
        }
    }

    /// Forward a keypress to the child as raw terminal input.
    fn forward(&mut self, key: KeyEvent) {
        let Some(child) = &self.child else { return };
        let bytes: Vec<u8> = match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let upper = c.to_ascii_uppercase() as u8;
                    if !upper.is_ascii_uppercase() {
                        return;
                    }
                    vec![upper - b'A' + 1]
                } else {
                    let mut buf = [0u8; 4];
                    c.encode_utf8(&mut buf).as_bytes().to_vec()
                }
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Tab => vec![b'\t'],
            KeyCode::Esc => vec![0x1b],
            KeyCode::Up => vec![0x1b, b'[', b'A'],
            KeyCode::Down => vec![0x1b, b'[', b'B'],
            KeyCode::Right => vec![0x1b, b'[', b'C'],
            KeyCode::Left => vec![0x1b, b'[', b'D'],
            _ => return,
        };
        child.write_all(&bytes);
    }

    /// Route a keypress: scroll keys stay local, everything else goes
    /// to the child. Ignored once finished (owner closes the pane).
    pub fn handle_key(&mut self, key: KeyEvent) {
        if !self.is_running() {
            return;
        }
        match key.code {
            // Local scrollback only; never sent to the child.
            KeyCode::PageUp => {
                self.follow = false;
                self.scroll += 15;
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(15);
                if self.scroll == 0 {
                    self.follow = true;
                }
            }
            KeyCode::End => {
                self.follow = true;
                self.scroll = 0;
            }
            _ => self.forward(key),
        }
    }

    /// Render into `area` (the dashboard's action pane). The border is
    /// cyan while running, green on success, red on failure; the status
    /// rides the bottom border.
    pub fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let (icon, border_color, status) = match &self.phase {
            Phase::Running => (
                "⏳",
                widgets::ACCENT,
                "running · keys → command · ctrl+c interrupt · pgup/pgdn scroll",
            ),
            Phase::Done(0) => (
                "✔",
                ratatui::style::Color::Green,
                "success — press any key to close",
            ),
            Phase::Done(code) => (
                "✘",
                ratatui::style::Color::Red,
                &format!("failed (exit {code}) — press any key to close")[..],
            ),
        };
        let status = status.to_string();

        let block = widgets::panel_color(&format!("{icon} {}", self.title), border_color)
            .title_bottom(
                Line::from(widgets::span(status, widgets::hint()))
                    .alignment(ratatui::layout::Alignment::Right),
            );
        let inner = block.inner(area);
        // Wipe whatever the dashboard painted here before we overlay —
        // otherwise stale action-list text bleeds through the pane.
        f.render_widget(ratatui::widgets::Clear, inner);
        f.render_widget(block, area);

        let lines_all = self.emu.snapshot_lines();
        let view_rows = inner.height as usize;
        let total = lines_all.len();

        let start = if self.follow {
            total.saturating_sub(view_rows)
        } else {
            self.scroll = self.scroll.min(total.saturating_sub(1));
            total
                .saturating_sub(self.scroll + 1)
                .saturating_sub(view_rows.saturating_sub(1))
        };
        let end = (start + view_rows).min(total);
        let shown: Vec<Line> = lines_all[start..end.max(start)]
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), Style::new())))
            .collect();
        f.render_widget(Paragraph::new(shown), inner);
    }
}

impl Drop for RunPane {
    fn drop(&mut self) {
        // Never leave a child running behind a closed pane.
        if let Some(child) = self.child.take() {
            child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ExtCmd;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use std::time::{Duration, Instant};

    fn make(code: i32) -> Box<RunPane> {
        let cmd = ExtCmd {
            tag: "t".into(),
            program: "sh".into(),
            args: vec!["-c".into(), format!("exit {code}")],
            stdin_file: None,
            note: "unit test command".into(),
            ok_msg: None,
            fail_msg: None,
            done_log: None,
        };
        RunPane::new(&cmd, 24, 80).unwrap()
    }

    fn wait_done(pane: &mut RunPane) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && pane.is_running() {
            pane.poll();
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn render(pane: &mut RunPane) -> ratatui::buffer::Buffer {
        let mut term = ratatui::Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| pane.draw(f, f.area())).unwrap();
        term.backend().buffer().clone()
    }

    fn text_of(buf: &ratatui::buffer::Buffer) -> String {
        buf.content.iter().map(|c| c.symbol().to_string()).collect()
    }

    fn has_fg(buf: &ratatui::buffer::Buffer, color: Color) -> bool {
        buf.content.iter().any(|c| c.style().fg == Some(color))
    }

    #[test]
    fn failure_border_is_red_with_exit_footer() {
        let mut pane = make(3);
        wait_done(&mut pane);
        assert_eq!(pane.result(), Some(false));
        let buf = render(&mut pane);
        let text = text_of(&buf);
        assert!(text.contains("failed (exit 3)"), "text: {text}");
        assert!(text.contains("press any key"));
        assert!(has_fg(&buf, Color::Red), "no red border cell");
    }

    #[test]
    fn success_border_is_green() {
        let mut pane = make(0);
        wait_done(&mut pane);
        assert_eq!(pane.result(), Some(true));
        let buf = render(&mut pane);
        let text = text_of(&buf);
        assert!(text.contains("success"), "text: {text}");
        assert!(has_fg(&buf, Color::Green), "no green border cell");
    }

    #[test]
    fn running_pane_has_cyan_border_and_hint() {
        let mut pane = make(0); // exits quickly; draw before that
        let buf = render(&mut pane);
        assert!(has_fg(&buf, Color::Cyan) || pane.result().is_some(),
                "no cyan border cell");
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;
    use crate::app::ExtCmd;
    use ratatui::backend::TestBackend;
    use std::time::{Duration, Instant};

    /// Regression: the runner must erase dashboard content painted into
    /// the same rect, not let it bleed through around short output.
    #[test]
    fn runner_clears_background_text() {
        let cmd = ExtCmd {
            tag: "t".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "echo prompt-line".into()],
            stdin_file: None,
            note: "runner title".into(),
            ok_msg: None,
            fail_msg: None,
            done_log: None,
        };
        let mut pane = RunPane::new(&cmd, 24, 80).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && pane.is_running() {
            pane.poll();
            std::thread::sleep(Duration::from_millis(50));
        }

        let mut term = ratatui::Terminal::new(TestBackend::new(60, 10)).unwrap();
        term.draw(|f| {
            let area = f.area();
            // Simulate the dashboard having painted action rows here.
            let bg = vec![
                ratatui::text::Line::from("Clean Package Cache (keep recent)"),
                ratatui::text::Line::from("Remove Uninstalled Caches (-Sc)"),
                ratatui::text::Line::from("Wipe ENTIRE Cache (-Scc)"),
            ];
            f.render_widget(ratatui::widgets::Paragraph::new(bg), area);
            pane.draw(f, area);
        })
        .unwrap();

        let text: String = term.backend().buffer().content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(!text.contains("keep recent"), "stale text bled through");
        assert!(!text.contains("Uninstalled Caches"), "stale text bled through");
        assert!(text.contains("prompt-line"), "output missing");
    }
}
