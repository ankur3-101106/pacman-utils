// ──────────────────────────────────────────────────────────────────────
// viewer.rs — generic scrollable read-only screen (replaces ui_pager)
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, text::Span, Frame};

use crate::app::{App, Screen};
use crate::widgets::TextViewer;

pub struct ViewerScreen {
    viewer: TextViewer,
}

impl ViewerScreen {
    pub fn new(title: impl Into<String>, lines: Vec<Vec<Span<'static>>>) -> Box<Self> {
        Box::new(Self {
            viewer: TextViewer::new_styled(title, lines),
        })
    }

    pub fn from_text(title: impl Into<String>, text: &str) -> Box<Self> {
        Box::new(Self {
            viewer: TextViewer::from_text(title, text),
        })
    }
}

impl Screen for ViewerScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        if self.viewer.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        self.viewer.render(f, area);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓/pgup/pgdn scroll", "g/G top/bottom", "esc close"]
    }
}
