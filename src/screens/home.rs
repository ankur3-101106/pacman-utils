// ──────────────────────────────────────────────────────────────────────
// home.rs — linutil-style dashboard
//
// Left column: textual ARCHMAN logo + version, category list, live
// system info. Right column: search bar over a flat action list; the
// embedded command runner renders here too. Bottom: full-width command
// list. Selecting an action shows its description before you run it.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, Screen};
use crate::sys::{self, Job, SysBrief};
use crate::widgets;

use super::registry::{Launch, RunSpec, CATEGORIES};

/// One searchable entry per action across all categories.
struct IndexEntry {
    cat: usize,
    idx: usize,
    label: String,
    desc: &'static str,
}

pub struct HomeScreen {
    cat: usize,
    action: usize,
    cat_offset: usize,
    act_offset: usize,
    sidebar_view: usize,
    action_view: usize,
    /// Run spec awaiting confirm-modal resolution.
    pending_run: Option<RunSpec>,
    /// Run spec queued most recently (for result routing).
    last_run: Option<RunSpec>,

    // Search (/)
    search_active: bool,
    query: String,
    index: Vec<IndexEntry>,
    labels: Vec<String>,
    matches: Vec<usize>,
    search_sel: usize,

    // Sidebar system summary
    sys_job: Option<Job<SysBrief>>,
    sys: Option<SysBrief>,
}

/// Rectangles for every dashboard region.
struct DashRects {
    logo: Rect,
    cats: Rect,
    sys: Rect,
    search: Rect,
    actions: Rect,
    cmdlist: Rect,
}

fn dashboard_layout(area: Rect) -> DashRects {
    let outer = Layout::vertical([Constraint::Min(3), Constraint::Length(8)]).split(area);
    let cols = Layout::horizontal([Constraint::Length(32), Constraint::Fill(1)]).split(outer[0]);
    let sidebar = Layout::vertical([
        Constraint::Length(5),
        Constraint::Fill(1),
        Constraint::Length(6),
    ])
    .split(cols[0]);
    let main = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).split(cols[1]);
    DashRects {
        logo: sidebar[0],
        cats: sidebar[1],
        sys: sidebar[2],
        search: main[0],
        actions: main[1],
        cmdlist: outer[1],
    }
}

impl HomeScreen {
    pub fn new() -> Self {
        let mut index = Vec::new();
        for (ci, cat) in CATEGORIES.iter().enumerate() {
            for (ai, a) in cat.actions.iter().enumerate() {
                index.push(IndexEntry {
                    cat: ci,
                    idx: ai,
                    label: format!("[{}] {}", ci + 1, a.label),
                    desc: a.desc,
                });
            }
        }
        let labels: Vec<String> = index.iter().map(|e| e.label.clone()).collect();
        Self {
            cat: 0,
            action: 0,
            cat_offset: 0,
            act_offset: 0,
            sidebar_view: 10,
            action_view: 15,
            pending_run: None,
            last_run: None,
            search_active: false,
            query: String::new(),
            matches: Vec::new(),
            search_sel: 0,
            index,
            labels,
            sys_job: Some(Job::spawn("system info", sys::sys_brief)),
            sys: None,
        }
    }

    fn clamp(&mut self) {
        let cats = CATEGORIES.len();
        self.cat = self.cat.min(cats - 1);
        let actions = CATEGORIES[self.cat].actions.len();
        self.action = self.action.min(actions.saturating_sub(1));

        if self.cat < self.cat_offset {
            self.cat_offset = self.cat;
        } else if self.sidebar_view > 0 && self.cat >= self.cat_offset + self.sidebar_view {
            self.cat_offset = self.cat + 1 - self.sidebar_view;
        }
        if self.action < self.act_offset {
            self.act_offset = self.action;
        } else if self.action_view > 0 && self.action >= self.act_offset + self.action_view {
            self.act_offset = self.action + 1 - self.action_view;
        }
    }

    fn switch_cat(&mut self, delta: isize) {
        let n = CATEGORIES.len() as isize;
        self.cat = (self.cat as isize + delta).rem_euclid(n) as usize;
        self.action = 0;
        self.clamp();
    }

    // ── Search ──────────────────────────────────────────────────────

    fn refilter(&mut self) {
        self.matches = crate::fuzzy::filter_indices(&self.labels, &self.query);
        self.search_sel = 0;
    }

    fn search_selected(&self) -> Option<(usize, usize)> {
        self.matches
            .get(self.search_sel)
            .map(|&i| (self.index[i].cat, self.index[i].idx))
    }

    // ── Launching ───────────────────────────────────────────────────

    fn launch_at(&mut self, app: &mut App, cat: usize, idx: usize) {
        let Some(def) = CATEGORIES[cat].actions.get(idx) else {
            return;
        };
        match &def.launch {
            Launch::Screen(open) => app.push(open(app)),
            Launch::Run(spec) => match (spec.confirm)(app) {
                Some(prompt) => {
                    self.pending_run = Some(*spec);
                    app.confirm(prompt, spec.danger);
                }
                None => self.run_spec(app, *spec),
            },
        }
    }

    fn run_spec(&mut self, app: &mut App, spec: RunSpec) {
        self.last_run = Some(spec);
        (spec.build)(app);
    }
}

impl Default for HomeScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for HomeScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        use KeyCode::*;

        // ── Search mode captures typing first ──
        if self.search_active {
            match key.code {
                Char(c)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    self.query.push(c);
                    self.refilter();
                    self.sync_search_offset();
                }
                Backspace => {
                    self.query.pop();
                    self.refilter();
                    self.sync_search_offset();
                }
                Esc => {
                    self.query.clear();
                    self.matches.clear();
                    self.search_active = false;
                }
                Up => {
                    self.search_sel = self.search_sel.saturating_sub(1);
                    self.sync_search_offset();
                }
                Down => {
                    if self.search_sel + 1 < self.matches.len() {
                        self.search_sel += 1;
                    }
                    self.sync_search_offset();
                }
                Tab => self.search_active = false,
                Enter => {
                    if let Some((c, i)) = self.search_selected() {
                        self.launch_at(app, c, i);
                    }
                }
                _ => {}
            }
            return;
        }

        let actions_len = CATEGORIES[self.cat].actions.len();

        match key.code {
            Char('?') => {
                app.toggle_help();
            }
            Char('q') => app.quit_app(),
            Char('/') => {
                self.search_active = true;
            }
            Up | Char('k') => {
                self.action = self.action.saturating_sub(1);
                self.clamp();
            }
            Down | Char('j') => {
                if self.action + 1 < actions_len {
                    self.action += 1;
                }
                self.clamp();
            }
            BackTab => self.switch_cat(-1),
            Tab => self.switch_cat(1),
            Char('g') => {
                self.action = 0;
                self.clamp();
            }
            Char('G') => {
                self.action = actions_len.saturating_sub(1);
                self.clamp();
            }
            Char(c) if c.is_ascii_digit() => {
                let idx = c.to_digit(10).unwrap_or(0) as usize;
                if idx >= 1 && idx <= CATEGORIES.len() {
                    self.cat = idx - 1;
                    self.action = 0;
                    self.clamp();
                }
            }
            Enter => self.launch_at(app, self.cat, self.action),
            _ => {}
        }
    }

    fn poll(&mut self, _app: &mut App) {
        if let Some(mut job) = self.sys_job.take() {
            match job.poll() {
                Some(brief) => self.sys = Some(brief),
                None => self.sys_job = Some(job),
            }
        }
    }

    fn content_area(&self, full: Rect) -> Rect {
        dashboard_layout(full).actions
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let r = dashboard_layout(area);
        widgets::render_logo_pane(f, r.logo, crate::VERSION);

        // ── Categories ──
        let sb_rows = r.cats.height.saturating_sub(2) as usize;
        if sb_rows > 0 {
            self.sidebar_view = sb_rows;
        }
        let mut sb_lines: Vec<Line> = Vec::new();
        for (i, c) in CATEGORIES.iter().enumerate().skip(self.cat_offset) {
            if sb_lines.len() >= self.sidebar_view {
                break;
            }
            let num = format!("{:>2} ", i + 1);
            let style = if i == self.cat {
                widgets::selected_style(widgets::cat_color(i))
            } else {
                widgets::cat_style(i)
            };
            let cursor = if i == self.cat { "▸ " } else { "  " };
            sb_lines.push(Line::from(vec![
                widgets::span(format!("{cursor}{num}"), {
                    if i == self.cat {
                        style
                    } else {
                        widgets::hint()
                    }
                }),
                widgets::span(c.title.to_string(), style),
            ]));
        }
        let sb_block = widgets::panel_color("Categories", widgets::ACCENT);
        f.render_widget(Paragraph::new(sb_lines).block(sb_block), r.cats);

        // ── System info ──
        let sys_rows: Vec<(&str, String)> = match &self.sys {
            Some(b) => vec![
                ("CPU", b.cpu.clone()),
                ("RAM", b.ram.clone()),
                ("DISK", b.disk.clone()),
                ("GPU", b.gpu.clone()),
            ],
            None => vec![
                ("CPU", "…".into()),
                ("RAM", "…".into()),
                ("DISK", "…".into()),
                ("GPU", "…".into()),
            ],
        };
        widgets::render_sysinfo_pane(f, r.sys, &sys_rows);

        // ── Search bar ──
        widgets::render_search_pane(f, r.search, &self.query, self.search_active);

        // ── Actions pane (or search results) ──
        self.clamp();
        let searching = !self.query.is_empty();
        let act_rows = r.actions.height.saturating_sub(2) as usize;

        let desc: &str;
        let mut act_lines: Vec<Line> = Vec::new();

        if searching {
            desc = self
                .matches
                .get(self.search_sel)
                .map(|&i| self.index[i].desc)
                .unwrap_or("No matching actions.");
            // Render window with its own offset (reuse act_offset).
            let start = self.act_offset.min(self.matches.len());
            for &mi in self.matches.iter().skip(start).take(act_rows) {
                let e = &self.index[mi];
                let sel = mi
                    == self
                        .matches
                        .get(self.search_sel)
                        .copied()
                        .unwrap_or(usize::MAX);
                let style = if sel {
                    widgets::selected_style(widgets::cat_color(e.cat))
                } else {
                    Style::new()
                };
                let cursor = if sel { "▸ " } else { "  " };
                act_lines.push(Line::from(vec![
                    widgets::span(cursor.to_string(), widgets::cat_style(e.cat)),
                    Span::styled(e.label.clone(), style),
                    widgets::span(format!("  {}", CATEGORIES[e.cat].title), widgets::dim()),
                ]));
            }
        } else {
            let acts = CATEGORIES[self.cat].actions;
            desc = acts
                .get(self.action)
                .map(|a| a.desc)
                .unwrap_or("No actions in this category.");
            for (i, a) in acts.iter().enumerate().skip(self.act_offset) {
                if act_lines.len() >= act_rows {
                    break;
                }
                let sel = i == self.action;
                let style = if sel {
                    widgets::selected_style(widgets::cat_color(self.cat))
                } else {
                    Style::new()
                };
                let cursor = if sel { "▸ " } else { "  " };
                act_lines.push(Line::from(vec![
                    widgets::span(
                        cursor.to_string(),
                        widgets::selected_style(widgets::cat_color(self.cat)),
                    ),
                    Span::styled(a.label.to_string(), style),
                ]));
            }
        }

        // Actions pane with the description on its bottom border.
        let title = if searching {
            format!("Search: {} ({} matches)", self.query, self.matches.len())
        } else {
            CATEGORIES[self.cat].title.to_string()
        };
        let desc_style = widgets::dim();
        let block = widgets::panel_color(&title, widgets::cat_color(self.cat)).title_bottom(
            Line::from(widgets::span(format!(" {} ", desc), desc_style))
                .alignment(ratatui::layout::Alignment::Right),
        );
        f.render_widget(Paragraph::new(act_lines).block(block), r.actions);

        // ── Command list footer ──
        widgets::render_command_list(f, r.cmdlist);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec![]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(spec) = self.pending_run.take() else {
            return;
        };
        if !yes {
            app.toast("Cancelled.", widgets::Sev::Info);
            app.log(&format!("{} cancelled", spec.tag));
            return;
        }
        self.run_spec(app, spec);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if let Some(spec) = self.last_run.take() {
            if spec.tag == tag {
                if ok {
                    app.toast(spec.ok_msg, widgets::Sev::Success);
                    app.log(spec.done_log);
                } else {
                    app.toast(spec.fail_msg, widgets::Sev::Error);
                    app.log(spec.done_log);
                }
                return;
            }
            self.last_run = Some(spec);
        }
    }
}

impl HomeScreen {
    /// Keep the search-result window in sync while scrolling.
    pub fn sync_search_offset(&mut self) {
        let total = self.matches.len();
        if total == 0 {
            self.act_offset = 0;
            return;
        }
        if self.search_sel < self.act_offset {
            self.act_offset = self.search_sel;
        } else if self.action_view > 0 && self.search_sel >= self.act_offset + self.action_view {
            self.act_offset = self.search_sel + 1 - self.action_view;
        }
    }
}
