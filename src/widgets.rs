// ──────────────────────────────────────────────────────────────────────
// widgets.rs — Shared TUI components
//
// The ratatui equivalents of lib/ui.sh: menus (ui_choose), fuzzy lists
// (ui_filter), scrollable text panes (ui_pager), key-value tables
// (ui_table), status messages, spinner overlay and help bar.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

// ── Theme ───────────────────────────────────────────────────────────
//
// One place owns every color so the whole utility stays coherent.
// Honor NO_COLOR (https://no-color.org): when set, every helper returns
// an unstyled Style so the UI degrades to plain monochrome text while
// keeping icons/glyphs as non-color cues.

/// Global accent — cyan reads well on both dark and light terminals.
pub const ACCENT: Color = Color::Cyan;

/// Respect https://no-color.org.
fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn styled(fg: Color) -> Style {
    if color_enabled() {
        Style::new().fg(fg)
    } else {
        Style::new()
    }
}

pub fn accent() -> Style {
    styled(ACCENT)
}
pub fn accent_bold() -> Style {
    if color_enabled() {
        Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    }
}
pub fn success() -> Style {
    styled(Color::Green).add_modifier(Modifier::BOLD)
}
pub fn warning() -> Style {
    styled(Color::Yellow).add_modifier(Modifier::BOLD)
}
pub fn danger() -> Style {
    styled(Color::Red).add_modifier(Modifier::BOLD)
}
/// Secondary text — mid-gray, readable on dark terminals (not DarkGray).
pub fn dim() -> Style {
    styled(Color::Indexed(244))
}
/// Help-bar / hint text — bright enough for low-contrast environments.
pub fn hint() -> Style {
    styled(Color::Indexed(250))
}

/// Per-category accent colors. Chosen to differ in hue AND lightness so
/// they stay distinguishable with common color-vision deficiencies;
/// the numbered sidebar (1–7) is the non-color identifier.
pub fn cat_color(idx: usize) -> Color {
    const PALETTE: [Color; 7] = [
        Color::Cyan,      // 1 📦 Packages
        Color::Green,     // 2 ⬆ System
        Color::Yellow,    // 3 🧹 Maintenance
        Color::Blue,      // 4 🌍 Mirrors
        Color::Magenta,   // 5 📊 Information
        Color::LightRed,  // 6 ⭐ Extras
        Color::Indexed(250), // 7 ⚙ Settings (near-white)
    ];
    PALETTE[idx % PALETTE.len()]
}

/// Category-colored style helpers.
pub fn cat_style(idx: usize) -> Style {
    styled(cat_color(idx))
}

/// Selection style: bold + italic in the accent color — noticeable but
/// not a full reverse-video block. The ▸ cursor remains the primary cue.
pub fn selected_style(color: Color) -> Style {
    if color_enabled() {
        Style::new()
            .fg(color)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC)
    } else {
        Style::new()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC)
    }
}

/// Severity for toast messages — mirrors ui_success/ui_warn/ui_error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    Info,
    Success,
    Warn,
    Error,
}

impl Sev {
    pub fn icon(self) -> &'static str {
        match self {
            Sev::Info => "ℹ",
            Sev::Success => "✔",
            Sev::Warn => "⚠",
            Sev::Error => "✘",
        }
    }
    pub fn style(self) -> Style {
        match self {
            Sev::Info => accent(),
            Sev::Success => success(),
            Sev::Warn => warning(),
            Sev::Error => danger(),
        }
    }
}

// ── Small text helpers ──────────────────────────────────────────────

pub fn span(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

pub fn line(spans: Vec<Span<'static>>) -> Line<'static> {
    Line::from(spans)
}

/// A bordered block in the archman style.
pub fn panel(title: &str) -> Block<'static> {
    panel_color(title, ACCENT)
}

/// Bordered panel in an arbitrary accent (used for per-category theming).
pub fn panel_color(title: &str, color: Color) -> Block<'static> {
    let title_style = if color_enabled() {
        Style::new().fg(color).add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    Block::new()
        .borders(Borders::ALL)
        .border_style(styled(color))
        .title(Span::styled(format!(" {title} "), title_style))
}

// ── Menu (ui_choose) ────────────────────────────────────────────────

/// A vertical list menu with highlight navigation.
pub struct Menu {
    pub title: String,
    pub items: Vec<String>,
    pub selected: usize,
    offset: usize,
    /// Visible rows, updated each render.
    view: usize,
}

impl Menu {
    pub fn new(title: impl Into<String>, items: Vec<String>) -> Self {
        Self { title: title.into(), items, selected: 0, offset: 0, view: 20 }
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.clamp_scroll();
    }

    pub fn down(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1).min(self.items.len() - 1);
        }
        self.clamp_scroll();
    }

    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }

    fn clamp_scroll(&mut self) {
        let view = self.view.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + view {
            self.offset = self.selected + 1 - view;
        }
    }

    /// Handle movement keys; returns true when consumed.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.up();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.down();
                true
            }
            _ => false,
        }
    }

    pub fn render(&mut self, f: &mut Frame<'_>, area: Rect) {
        let inner_height = area.height.saturating_sub(2) as usize;
        if inner_height > 0 {
            self.view = inner_height;
            self.clamp_scroll();
        }
        let visible: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(inner_height)
            .map(|(i, item)| {
                let cursor = if i == self.selected { "▸ " } else { "  " };
                let style = if i == self.selected {
                    selected_style(ACCENT)
                } else {
                    Style::new()
                };
                line(vec![span(cursor.to_string(), accent_bold()), span(item.clone(), style)])
            })
            .collect();

        let block = panel(&self.title);
        f.render_widget(Paragraph::new(visible).block(block), area);
    }
}

// ── Fuzzy List (ui_filter) ──────────────────────────────────────────

/// Fuzzy-searchable list: type to filter, arrows to pick.
pub struct FuzzyList {
    pub title: String,
    items: Vec<String>,
    filtered: Vec<usize>,
    query: String,
    pub selected: usize,
    offset: usize,
    /// Visible rows, updated each render.
    view: usize,
}

impl FuzzyList {
    pub fn new(title: impl Into<String>, items: Vec<String>) -> Self {
        let mut l = Self {
            title: title.into(),
            items,
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
            offset: 0,
            view: 20,
        };
        l.refilter();
        l
    }

    fn refilter(&mut self) {
        self.filtered = crate::fuzzy::filter_indices(&self.items, &self.query);
        self.selected = 0;
        self.offset = 0;
    }

    pub fn item_count(&self) -> usize {
        self.filtered.len()
    }

    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// The currently highlighted item string, if any.
    pub fn take_selected(&self) -> Option<String> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.items.get(i).cloned())
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.ensure_visible();
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1).min(self.filtered.len() - 1);
        }
        self.ensure_visible();
    }

    fn ensure_visible(&mut self) {
        let view = self.view.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + view {
            self.offset = self.selected + 1 - view;
        }
    }

    /// Consume typing/navigation keys; returns true when handled.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                    return false;
                }
                self.query.push(c);
                self.refilter();
                true
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
                true
            }
            KeyCode::Up => {
                self.move_up();
                true
            }
            KeyCode::Down => {
                self.move_down();
                true
            }
            KeyCode::Esc => {
                // First Esc clears an active filter; once the query is
                // empty, let the screen see Esc so it can go back.
                if self.query.is_empty() {
                    return false;
                }
                self.query.clear();
                self.refilter();
                true
            }
            _ => false,
        }
    }

    pub fn render(&mut self, f: &mut Frame<'_>, area: Rect) {
        let rows = area.height.saturating_sub(2) as usize;
        let inner_height = rows.saturating_sub(1); // one row for the query bar
        if inner_height > 0 {
            self.view = inner_height;
            self.ensure_visible();
        }

        let mut lines: Vec<Line> = Vec::with_capacity(inner_height + 1);
        lines.push(line(vec![
            span("search: ", dim()),
            span(format!("{}▏", self.query), accent_bold()),
        ]));

        for i in self.offset..self.offset + inner_height {
            let Some(&item_idx) = self.filtered.get(i) else { break };
            let item = &self.items[item_idx];
            let selected_row = i == self.selected;
            let style = if selected_row {
                selected_style(ACCENT)
            } else {
                Style::new()
            };
            lines.push(line(vec![span(item.clone(), style)]));
        }

        let title = format!(" {} ({}/{} matches) ", self.title, self.item_count(), self.total_count());
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(accent())
            .title(Span::styled(title, accent_bold()));
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

// ── Text Viewer (ui_pager) ──────────────────────────────────────────

/// Scrollable read-only pane; styled lines supported.
pub struct TextViewer {
    pub title: String,
    pub lines: Vec<Vec<Span<'static>>>,
    pub top: usize,
}

impl TextViewer {
    pub fn from_text(title: impl Into<String>, text: &str) -> Self {
        let lines: Vec<Vec<Span>> = if text.is_empty() {
            vec![vec![span("(empty)", dim())]]
        } else {
            text.lines().map(|l| vec![span(l.to_string(), Style::new())]).collect()
        };
        Self { title: title.into(), lines, top: 0 }
    }

    pub fn new_styled(title: impl Into<String>, lines: Vec<Vec<Span<'static>>>) -> Self {
        Self { title: title.into(), lines, top: 0 }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.top = self.top.saturating_sub(1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.lines.is_empty() {
                    self.top = (self.top + 1).min(self.lines.len() - 1);
                }
                true
            }
            KeyCode::PageUp => {
                self.top = self.top.saturating_sub(15);
                true
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                if !self.lines.is_empty() {
                    self.top = (self.top + 15).min(self.lines.len() - 1);
                }
                true
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.top = 0;
                true
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.lines.is_empty() {
                    self.top = self.lines.len() - 1;
                }
                true
            }
            _ => false,
        }
    }

    pub fn render(&self, f: &mut Frame<'_>, area: Rect) {
        let rows = area.height.saturating_sub(2) as usize;
        let shown: Vec<Line> = self
            .lines
            .iter()
            .skip(self.top)
            .take(rows)
            .map(|l| Line::from(l.clone()))
            .collect();
        let pos = format!(" {}–{}/{} ", self.top + 1, (self.top + rows).min(self.lines.len()), self.lines.len());
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(accent())
            .title(Span::styled(format!(" {} ", self.title), accent_bold()))
            .title(Line::from(Span::styled(pos, dim())).alignment(Alignment::Right));
        f.render_widget(Paragraph::new(shown).block(block), area);
    }
}

// ── Key-Value Table (ui_table) ──────────────────────────────────────

/// Render aligned key/value rows like `ui_table`.
pub fn kv_table(f: &mut Frame<'_>, area: Rect, title: &str, rows: &[(&str, String)]) {
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            line(vec![
                span(format!("  {k:<width$} │ ", width = width), accent_bold()),
                span(v.clone(), Style::new()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(panel(title)), area);
}

// ── Chrome ──────────────────────────────────────────────────────────

/// Full ASCII logo shown atop the shortcuts overlay.
const BIG_LOGO: [&str; 6] = [
    " █████╗ ██████╗  ██████╗██╗  ██╗███╗   ███╗ █████╗ ███╗   ██╗",
    "██╔══██╗██╔══██╗██╔════╝██║  ██║████╗ ████║██╔══██║████╗  ██║",
    "███████║██████╔╝██║     ███████║██╔████╔██║███████║██╔██╗ ██║",
    "██╔══██║██╔══██╗██║     ██╔══██║██║╚██╔╝██║██╔══██║██║╚██╗██║",
    "██║  ██║██║  ██║╚██████╗██║  ██║██║ ╚═╝ ██║██║  ██║██║ ╚████║",
    "╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝",
];

/// Header bar: wordmark left, version right.
/// Compact textual logo for the sidebar (3 rows, 27 cols).
pub const LOGO_SMALL: [&str; 3] = [
    "▄▀█ █▀█ █▀▀ █░█ ▄▀▄ ▄▀█ █▄█",
    "█▀█ █▀▄ █░░ █▀█ █░█ █▀█ █░█",
    "▀ ▀ ▀ ▀ ▀▀▀ ▀░▀ ▀░▀ ▀ ▀ ▀░▀",
];

/// Sidebar logo pane: colored ASCII name + version beneath.
pub fn render_logo_pane(f: &mut Frame<'_>, area: Rect, version: &str) {
    let rows =
        Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).split(area);
    let logo: Vec<Line> = LOGO_SMALL
        .iter()
        .enumerate()
        .map(|(i, l)| Line::from(span(l.to_string(), styled(cat_color(i + 1)))))
        .collect();
    f.render_widget(Paragraph::new(logo).alignment(Alignment::Center), rows[0]);
    f.render_widget(
        Paragraph::new(Line::from(span(
            format!("archman v{version}"),
            styled(cat_color(5)),
        )))
        .alignment(Alignment::Center),
        rows[1],
    );
}

/// Sidebar system summary pane (CPU/RAM/DISK/GPU).
pub fn render_sysinfo_pane(f: &mut Frame<'_>, area: Rect, rows_kv: &[(&str, String)]) {
    let block = panel_color("SYSTEM", cat_color(2));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let lines: Vec<Line> = rows_kv
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                span(format!("{k:>5}: "), styled(cat_color(2))),
                span(v.clone(), Style::new()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// Top search bar pane (linutil style).
pub fn render_search_pane(f: &mut Frame<'_>, area: Rect, query: &str, active: bool) {
    let style = if active { styled(cat_color(1)) } else { dim() };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(if active { styled(cat_color(1)) } else { styled(Color::Indexed(244)) })
        .title(Span::styled(" SEARCH ", styled(cat_color(1)).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = if query.is_empty() {
        Line::from(span(
            if active { "" } else { "Type to search (/)" },
            dim(),
        ))
    } else {
        Line::from(vec![
            span(query.to_string(), accent_bold()),
            span("▏", style),
        ])
    };
    f.render_widget(Paragraph::new(text), inner);
}

/// Full-width command-list footer (linutil style), two columns.
pub fn render_command_list(f: &mut Frame<'_>, area: Rect) {
    let block = panel_color("Command list", cat_color(4));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let left: [(&str, &str); 6] = [
        ("[q] [ctrl+c]", "Exit archman"),
        ("[tab]", "Next category"),
        ("[shift+tab]", "Previous category"),
        ("[1-7]", "Jump to category"),
        ("[/]", "Search actions"),
        ("[enter]", "Run action"),
    ];
    let right: [(&str, &str); 6] = [
        ("[↑/k] [↓/j]", "Select item"),
        ("[g] [G]", "First / last item"),
        ("[esc]", "Back · clear filter"),
        ("[?]", "All shortcuts"),
        ("[pgup/pgdn]", "Scroll output"),
        ("[ctrl+c]", "Interrupt command"),
    ];

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    for (col_idx, entries) in [left, right].into_iter().enumerate() {
        let lines: Vec<Line> = entries
            .iter()
            .map(|(k, d)| {
                Line::from(vec![
                    span(format!("{k:<14}"), styled(cat_color(1))),
                    span(d.to_string(), dim()),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), cols[col_idx]);
    }
}

/// Global shortcut cheatsheet overlay (toggled with ?).
pub fn render_help_overlay(f: &mut Frame<'_>) {
    let area = centered_rect(70, 78, f.area());
    f.render_widget(Clear, area);
    let block = panel(" ⌨  Keyboard Shortcuts ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Rainbow logo — one accent per category color.
    for (i, l) in BIG_LOGO.iter().enumerate() {
        lines.push(Line::from(span(
            l.to_string(),
            styled(cat_color(i)),
        )));
    }
    lines.push(Line::from(""));
    let mut row = |key: &str, desc: &str| {
        lines.push(line(vec![
            span(format!("  {:<16}", key), accent_bold()),
            span(desc.to_string(), Style::new()),
        ]));
    };
    row("↑ / k", "move selection up");
    row("↓ / j", "move selection down");
    row("tab", "next category");
    row("shift+tab", "previous category");
    row("1 … 7", "jump straight to a category");
    row("g / G", "first / last action");
    row("enter", "run or open the selected action");
    row("esc", "go back · clear search filter");
    row("q", "quit archman");
    row("?", "toggle this cheatsheet");
    row("ctrl+c", "quit immediately");
    lines.push(Line::from(""));
    lines.push(Line::from(span("  press any key to close", dim())));
    f.render_widget(Paragraph::new(lines), inner);
}

/// Bottom help bar: "↑↓ navigate · enter select · esc back".
pub fn help_bar(f: &mut Frame<'_>, area: Rect, hints: &[&str]) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(span(" · ", dim()));
        }
        spans.push(span(h.to_string(), hint()));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

/// Centered modal box helper.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup[1])[1]
}

/// Spinner overlay while a background job runs.
pub fn spinner(f: &mut Frame<'_>, frame_char: &str, label: &str) {
    let area = centered_rect(50, 7, f.area());
    let block = panel(label);
    let inner = block.inner(area);
    // Clear the whole overlay region so nothing bleeds through.
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let row = Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).split(inner);
    f.render_widget(
        Paragraph::new(line(vec![span(frame_char.to_string(), accent_bold()), span(format!(" {label}"), accent())]))
            .alignment(Alignment::Center),
        row[1],
    );
}
