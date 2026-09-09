// ──────────────────────────────────────────────────────────────────────
// remove.rs — Package removal (lib/remove.sh)
//
// Fuzzy-pick an explicitly installed package, choose one of three
// pacman removal strategies, verify the result.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Paragraph,
    Frame,
};
use std::time::Instant;

use crate::app::{App, Screen};
use crate::sys::{self, Job};
use crate::widgets::{self, FuzzyList, Menu, Sev};

use super::info_lines;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Loading,
    Pick,
    Confirm,
}

const METHODS: [&str; 4] = [
    "📦 Package only (pacman -R)",
    "🔗 Package + unused dependencies (pacman -Rs)",
    "🧹 Complete removal + configs (pacman -Rns)",
    "🔙 Cancel",
];

pub struct RemoveScreen {
    mode: Mode,
    pick: FuzzyList,
    confirm_pkg: String,
    confirm_menu: Menu,
    list_job: Option<Job<Vec<String>>>,
    /// (package, pacman flag) awaiting confirm resolution.
    #[allow(dead_code)]
    pending: Option<(String, &'static str)>,
    /// Package queued for the most recent removal attempt.
    last_remove: Option<String>,
}

impl RemoveScreen {
    pub fn new() -> Self {
        let mut s = Self {
            mode: Mode::Loading,
            pick: FuzzyList::new("Select package(s) to remove", Vec::new()),
            confirm_pkg: String::new(),
            confirm_menu: Menu::new("Removal method", METHODS.map(str::to_string).to_vec()),
            list_job: None,
            pending: None,
            last_remove: None,
        };
        s.reload_list();
        s
    }

    fn reload_list(&mut self) {
        self.mode = Mode::Loading;
        self.list_job = Some(Job::spawn(
            "Loading installed packages...",
            sys::explicit_rows,
        ));
    }

    fn enter_confirm(&mut self, pkg: String) {
        self.confirm_pkg = pkg;
        self.confirm_menu = Menu::new("Removal method", METHODS.map(str::to_string).to_vec());
        self.mode = Mode::Confirm;
    }

    fn request_remove(
        &mut self,
        app: &mut App,
        pkg: String,
        flag: &'static str,
        _prompt: String,
        _danger: bool,
    ) {
        self.last_remove = Some(pkg.clone());
        let tx = crate::tx::TransactionSpec::remove(app.settings(), flag, &pkg);
        app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
    }
}

impl Default for RemoveScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for RemoveScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            Mode::Loading => {}
            Mode::Pick => {
                if self.pick.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if let Some(entry) = self.pick.take_selected() {
                            let pkg = entry.split_whitespace().next().unwrap_or("").to_string();
                            if !pkg.is_empty() {
                                self.enter_confirm(pkg);
                            }
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => app.pop(),
                    _ => {}
                }
            }
            Mode::Confirm => {
                if self.confirm_menu.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        let pkg = self.confirm_pkg.clone();
                        match self.confirm_menu.selected {
                            0 => self.request_remove(
                                app,
                                pkg.clone(),
                                "-R",
                                format!("Remove {pkg}?"),
                                false,
                            ),
                            1 => self.request_remove(
                                app,
                                pkg.clone(),
                                "-Rs",
                                format!("Remove {pkg} and unused dependencies?"),
                                false,
                            ),
                            2 => self.request_remove(
                                app,
                                pkg.clone(),
                                "-Rns",
                                format!("Completely remove {pkg} (including configs)?"),
                                true,
                            ),
                            _ => {
                                app.toast("Removal cancelled.", Sev::Info);
                                self.reload_list();
                            }
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.reload_list(),
                    _ => {}
                }
            }
        }
    }

    fn poll(&mut self, app: &mut App) {
        if let Some(mut job) = self.list_job.take() {
            match job.poll() {
                Some(rows) => {
                    if rows.is_empty() {
                        app.toast("No explicitly installed packages found.", Sev::Error);
                        app.pop();
                    } else {
                        self.pick = FuzzyList::new("Select package(s) to remove", rows);
                        self.mode = Mode::Pick;
                    }
                }
                None => self.list_job = Some(job),
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        match self.mode {
            Mode::Loading => {}
            Mode::Pick => self.pick.render(f, area),
            Mode::Confirm => {
                // Package summary above, method menu below.
                let info = sys::qi(&self.confirm_pkg).unwrap_or_default();
                const INFO_FIELDS: [&str; 5] = [
                    "Name",
                    "Version",
                    "Description",
                    "Installed Size",
                    "Depends On",
                ];
                let lines = info_lines(&info, &INFO_FIELDS);
                let rows =
                    Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)]).split(area);

                let block = widgets::panel(&format!("🗑  Remove Package — {}", self.confirm_pkg));
                let inner = block.inner(rows[0]);
                f.render_widget(block, rows[0]);
                let styled: Vec<Line> = lines.into_iter().map(Line::from).collect();
                f.render_widget(Paragraph::new(styled), inner);

                self.confirm_menu.render(f, rows[1]);
            }
        }
    }

    fn busy(&self) -> Option<(String, Instant)> {
        self.list_job.as_ref().map(|j| (j.label.clone(), j.started))
    }

    fn help_hints(&self) -> Vec<&'static str> {
        match self.mode {
            Mode::Pick => vec!["type to filter", "↑↓ navigate", "enter select", "esc back"],
            Mode::Confirm => vec!["↑↓ navigate", "enter select", "esc cancel"],
            Mode::Loading => vec![],
        }
    }

    fn on_confirm(&mut self, _app: &mut App, _yes: bool) {}

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        if tag != "remove" {
            return;
        }
        // Mirror _remove_result: verify the package is really gone.
        let still_installed = self
            .last_remove
            .as_ref()
            .map(|pkg| sys::installed_set().contains(pkg))
            .unwrap_or(false);
        if !still_installed {
            app.toast("Package removed successfully!", Sev::Success);
            app.log("REMOVE: removed successfully");
        } else {
            app.toast(
                "Removal may have failed. Package is still installed.",
                Sev::Error,
            );
            app.log("REMOVE: removal may have failed");
        }
        let _ = ok;
        self.last_remove = None;
        self.reload_list();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_test_screen(pkg: &str, method_index: usize) -> RemoveScreen {
        let mut confirm_menu = Menu::new("Removal method", METHODS.map(str::to_string).to_vec());
        confirm_menu.selected = method_index;
        RemoveScreen {
            mode: Mode::Confirm,
            pick: FuzzyList::new("Select package(s) to remove", Vec::new()),
            confirm_pkg: pkg.to_string(),
            confirm_menu,
            list_job: None,
            pending: None,
            last_remove: None,
        }
    }

    #[test]
    fn test_remove_confirm_actions_false_native_confirm_true() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "false");
        app.settings_mut().set("NATIVE_CONFIRM", "true");

        // method index 2 corresponds to "-Rns"
        let mut screen = make_test_screen("ripgrep", 2);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed
        app.on_key(enter);

        // Invariant: CONFIRM_ACTIONS=false skips modal
        assert!(!app.has_modal(), "Archman modal should not be shown");
        assert!(app.modal_prompt().is_none());

        // Invariant: NATIVE_CONFIRM=true does NOT append --noconfirm
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(queued.args, vec!["pacman", "-Rns", "ripgrep"]);
        assert!(!queued.args.contains(&"--noconfirm".to_string()));
    }

    #[test]
    fn test_remove_confirm_actions_false_native_confirm_false() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "false");
        app.settings_mut().set("NATIVE_CONFIRM", "false");

        let mut screen = make_test_screen("ripgrep", 2);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed
        app.on_key(enter);

        assert!(!app.has_modal());
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(
            queued.args,
            vec!["pacman", "-Rns", "ripgrep", "--noconfirm"]
        );
    }

    #[test]
    fn test_remove_confirm_actions_true_native_confirm_true() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "true");
        app.settings_mut().set("NATIVE_CONFIRM", "true");

        let mut screen = make_test_screen("ripgrep", 2);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed to confirmation
        app.on_key(enter);

        // Modal should be shown
        assert!(app.has_modal());
        assert_eq!(
            app.modal_prompt(),
            Some("Completely remove ripgrep (including configs)?")
        );
        assert!(app.pending_cmd().is_none());

        // Accepting confirmation executes without --noconfirm
        app.on_key(KeyEvent::from(KeyCode::Char('y')));
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(queued.args, vec!["pacman", "-Rns", "ripgrep"]);
        assert!(!queued.args.contains(&"--noconfirm".to_string()));
    }

    #[test]
    fn test_remove_confirm_actions_true_native_confirm_false() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "true");
        app.settings_mut().set("NATIVE_CONFIRM", "false");

        let mut screen = make_test_screen("ripgrep", 2);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed to confirmation
        app.on_key(enter);

        assert!(app.has_modal());
        assert_eq!(
            app.modal_prompt(),
            Some("Completely remove ripgrep (including configs)?")
        );
        assert!(app.pending_cmd().is_none());

        // Accepting confirmation executes WITH --noconfirm
        app.on_key(KeyEvent::from(KeyCode::Char('y')));
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(
            queued.args,
            vec!["pacman", "-Rns", "ripgrep", "--noconfirm"]
        );
    }
}
