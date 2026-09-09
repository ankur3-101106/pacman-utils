// ──────────────────────────────────────────────────────────────────────
// search.rs — Package search (lib/search.sh)
//
// Fuzzy-search all available packages; picking one hands off to the
// install flow, which shows official-repo or AUR info.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};
use std::time::Instant;

use crate::app::{App, Screen};
use crate::sys::{self, Job};
use crate::widgets::{FuzzyList, Sev};

pub struct SearchScreen {
    list_job: Option<Job<Vec<String>>>,
    choose: Option<FuzzyList>,
}

impl SearchScreen {
    pub fn new(app: &App) -> Self {
        let helper = app.aur_helper();
        Self {
            list_job: Some(Job::spawn("Fetching available packages...", move || {
                sys::pkg_list_all(helper.as_deref())
            })),
            choose: None,
        }
    }
}

impl Screen for SearchScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        let Some(list) = &mut self.choose else { return };
        if list.handle_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Enter => {
                if let Some(pkg) = list.take_selected() {
                    app.log(&format!("SEARCH: Selected '{pkg}'"));
                    app.push(crate::screens::install::InstallScreen::preselected(
                        pkg,
                        app.aur_helper(),
                    ));
                }
            }
            KeyCode::Esc => app.pop(),
            _ => {}
        }
    }

    fn poll(&mut self, app: &mut App) {
        if let Some(mut job) = self.list_job.take() {
            match job.poll() {
                Some(items) => {
                    if items.is_empty() {
                        app.toast("No packages found in repositories.", Sev::Warn);
                        app.pop();
                    } else {
                        self.choose = Some(FuzzyList::new("Search packages...", items));
                    }
                }
                None => self.list_job = Some(job),
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        if let Some(list) = &mut self.choose {
            list.render(f, area);
        }
    }

    fn busy(&self) -> Option<(String, Instant)> {
        self.list_job.as_ref().map(|j| (j.label.clone(), j.started))
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec![
            "type to filter",
            "↑↓ navigate",
            "enter info/install",
            "esc back",
        ]
    }
}
