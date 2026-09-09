// ──────────────────────────────────────────────────────────────────────
// install.rs — Package installation
//
// Mirrors lib/install.sh: fuzzy-pick from all available packages,
// resolve official repos first, then AUR; optional dry-run preview;
// "not found" falls back to a similar-package search.
//
// State is kept FLAT (mode flag + sibling fields) rather than as a
// data-carrying enum so key handling never fights the borrow checker.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use std::time::Instant;

use crate::app::{App, ExtCmd, Screen};
use crate::sys::{self, Job};
use crate::widgets::{self, FuzzyList, Sev};

use super::info_lines;

const OFFICIAL_FIELDS: [&str; 5] = [
    "Name",
    "Version",
    "Repository",
    "Description",
    "Download Size",
];
const AUR_FIELDS: [&str; 5] = ["Name", "Version", "Description", "Maintainer", "Votes"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Fetching `pacman -Slq`.
    LoadingList,
    Choosing,
    Resolving,
    ShowingInfo,
    #[allow(dead_code)]
    DryRunPreview,
    PickingSimilar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Official,
    Aur,
}

/// Result of the background repo/AUR resolution.
#[allow(clippy::enum_variant_names)]
enum Found {
    Official {
        pkg: String,
        raw: String,
    },
    Aur {
        pkg: String,
        raw: String,
    },
    NotFound {
        official: Vec<String>,
        aur: Vec<String>,
    },
}

struct InfoView {
    pkg: String,
    source: Source,
    lines: Vec<Vec<Span<'static>>>,
}

#[allow(dead_code)]
enum Pending {
    Official(String),
    Aur(String),
}

pub struct InstallScreen {
    mode: Mode,
    aur_helper: Option<String>,

    choose: FuzzyList,
    similar: FuzzyList,
    info: Option<InfoView>,
    dryrun_lines: Vec<String>,

    list_job: Option<Job<Vec<String>>>,
    resolve_job: Option<Job<Found>>,

    pending: Option<Pending>,
    last_install_pkg: Option<String>,
    /// Preselected flows (search/groups) return to the previous screen.
    back_after_done: bool,
}

fn empty_choose() -> FuzzyList {
    FuzzyList::new("Select package to install...", Vec::new())
}

fn empty_similar() -> FuzzyList {
    FuzzyList::new("Similar packages — pick one to install", Vec::new())
}

impl InstallScreen {
    pub fn new(app: &App) -> Self {
        Self::with_loader(app.aur_helper(), false)
    }

    /// Jump straight to resolving a known package name.
    pub fn preselected(pkg: String, helper: Option<String>) -> Box<Self> {
        let mut s = Self::with_loader(helper, true);
        s.start_resolve(pkg);
        Box::new(s)
    }

    fn with_loader(helper: Option<String>, preselect: bool) -> Self {
        let list_job = if preselect {
            None
        } else {
            let h = helper.clone();
            Some(Job::spawn("Fetching available packages...", move || {
                sys::pkg_list_all(h.as_deref())
            }))
        };
        Self {
            mode: if preselect {
                Mode::Resolving
            } else {
                Mode::LoadingList
            },
            aur_helper: helper,
            choose: empty_choose(),
            similar: empty_similar(),
            info: None,
            dryrun_lines: Vec::new(),
            list_job,
            resolve_job: None,
            pending: None,
            last_install_pkg: None,
            back_after_done: preselect,
        }
    }

    fn start_resolve(&mut self, pkg: String) {
        let helper = self.aur_helper.clone();
        self.resolve_job = Some(Job::spawn(
            format!("Searching repositories for '{pkg}'..."),
            move || {
                if let Some(out) = sys::si(&[&pkg]) {
                    return Found::Official { pkg, raw: out };
                }
                if let Some(h) = &helper {
                    if let Some(out) = sys::capture(h, &["-Si", &pkg]) {
                        return Found::Aur { pkg, raw: out };
                    }
                }
                Found::NotFound {
                    official: sys::ss(&pkg),
                    aur: helper
                        .as_ref()
                        .map(|h| sys::aur_ss(h, &pkg))
                        .unwrap_or_default(),
                }
            },
        ));
        self.mode = Mode::Resolving;
    }

    fn finish(&mut self, app: &mut App) {
        if self.back_after_done {
            app.pop();
        } else {
            self.mode = Mode::Choosing;
        }
    }

    fn execute_install(&mut self, app: &mut App, pending: Pending) {
        match pending {
            Pending::Official(pkg) => {
                self.last_install_pkg = Some(pkg.clone());
                app.log(&format!("INSTALL: Installing {pkg} from official repos"));
                let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-S", &pkg]);
                app.queue_ext(
                    ExtCmd::new("install-official", "sudo", &cmd_args)
                        .note(format!("Install {pkg}"))
                        .result(
                            format!("{pkg} installed successfully!"),
                            "Installation failed.",
                            "INSTALL: official install finished",
                        ),
                );
            }
            Pending::Aur(pkg) => {
                self.last_install_pkg = Some(pkg.clone());
                if let Some(helper) = app.aur_helper() {
                    app.log(&format!("INSTALL: Installing {pkg} from AUR via {helper}"));
                    let cmd_args = crate::sys::aur_args(app.settings(), &["-S", &pkg]);
                    app.queue_ext(
                        ExtCmd::new("install-aur", &helper, &cmd_args)
                            .note(format!("Install {pkg} from AUR via {helper}"))
                            .result(
                                format!("{pkg} installed successfully!"),
                                "Installation failed.",
                                "INSTALL: AUR install finished",
                            ),
                    );
                } else {
                    app.toast("No AUR helper found.", Sev::Error);
                    self.finish(app);
                }
            }
        }
    }
}

/// Shared with search/groups flows: queue an install for one package,
/// official repos first, then AUR.
pub fn install_specific_package(app: &mut App, pkg: &str) {
    if sys::si(&[pkg]).is_some() {
        let tx = crate::tx::TransactionSpec::install_official(app.settings(), pkg);
        app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
    } else if let Some(helper) = app.aur_helper() {
        let tx = crate::tx::TransactionSpec::install_aur(app.settings(), &helper, pkg);
        app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
    } else {
        app.toast(
            format!("Cannot install {pkg} — no AUR helper found."),
            Sev::Error,
        );
    }
}

impl Screen for InstallScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            Mode::LoadingList | Mode::Resolving => {}
            Mode::Choosing => {
                if self.choose.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if let Some(pkg) = self.choose.take_selected() {
                            app.log(&format!("INSTALL: Selected '{pkg}'"));
                            self.start_resolve(pkg);
                        }
                    }
                    KeyCode::Esc => app.pop(),
                    _ => {}
                }
            }
            Mode::ShowingInfo => {
                let source = self.info.as_ref().map(|v| v.source);
                let pkg = self
                    .info
                    .as_ref()
                    .map(|v| v.pkg.clone())
                    .unwrap_or_default();
                match key.code {
                    KeyCode::Enter => match source {
                        Some(Source::Official) => {
                            self.last_install_pkg = Some(pkg.clone());
                            let tx =
                                crate::tx::TransactionSpec::install_official(app.settings(), &pkg);
                            app.push(Box::new(crate::screens::PreviewScreen::from_app(tx, app)));
                        }
                        Some(Source::Aur) => {
                            if let Some(helper) = app.aur_helper() {
                                self.last_install_pkg = Some(pkg.clone());
                                let tx = crate::tx::TransactionSpec::install_aur(
                                    app.settings(),
                                    &helper,
                                    &pkg,
                                );
                                app.push(Box::new(crate::screens::PreviewScreen::from_app(
                                    tx, app,
                                )));
                            } else {
                                app.toast("No AUR helper found.", Sev::Error);
                                self.finish(app);
                            }
                        }
                        None => {}
                    },
                    KeyCode::Esc | KeyCode::Char('q') => self.finish(app),
                    _ => {}
                }
            }
            Mode::DryRunPreview => {
                // Maintained for backward compatibility; previews now route to PreviewScreen
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.mode = Mode::ShowingInfo;
                }
            }
            Mode::PickingSimilar => {
                if self.similar.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if let Some(entry) = self.similar.take_selected() {
                            // Strip repo/ prefix and version tail → exact name.
                            let name = entry
                                .split_whitespace()
                                .next()
                                .unwrap_or("")
                                .rsplit('/')
                                .next()
                                .unwrap_or("")
                                .to_string();
                            if !name.is_empty() {
                                app.log(&format!("INSTALL: Selected '{name}'"));
                                self.start_resolve(name);
                            }
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.finish(app),
                    _ => {}
                }
            }
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
                        self.choose = FuzzyList::new("Select package to install...", items);
                        self.mode = Mode::Choosing;
                    }
                }
                None => self.list_job = Some(job),
            }
        }

        if let Some(mut job) = self.resolve_job.take() {
            match job.poll() {
                Some(found) => match found {
                    Found::Official { pkg, raw } => {
                        self.info = Some(InfoView {
                            pkg,
                            source: Source::Official,
                            lines: info_lines(&raw, &OFFICIAL_FIELDS),
                        });
                        self.mode = Mode::ShowingInfo;
                    }
                    Found::Aur { pkg, raw } => {
                        self.info = Some(InfoView {
                            pkg,
                            source: Source::Aur,
                            lines: info_lines(&raw, &AUR_FIELDS),
                        });
                        self.mode = Mode::ShowingInfo;
                    }
                    Found::NotFound { official, aur } => {
                        app.toast("Package not found in official repos or AUR.", Sev::Error);
                        let mut items = official;
                        items.extend(aur);
                        if items.is_empty() {
                            self.finish(app);
                        } else {
                            self.similar =
                                FuzzyList::new("Similar packages — pick one to install", items);
                            self.mode = Mode::PickingSimilar;
                        }
                    }
                },
                None => self.resolve_job = Some(job),
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        match self.mode {
            Mode::LoadingList | Mode::Resolving => {}
            Mode::Choosing => self.choose.render(f, area),
            Mode::PickingSimilar => self.similar.render(f, area),
            Mode::ShowingInfo => {
                if let Some(view) = &self.info {
                    let title = match view.source {
                        Source::Official => format!("📦 Install Package — {}", view.pkg),
                        Source::Aur => format!("🌟 Install from AUR — {}", view.pkg),
                    };
                    let hint = match view.source {
                        Source::Official => "enter install · esc back",
                        Source::Aur => "enter install from AUR · esc back",
                    };
                    render_info_panel(f, area, &title, &view.lines, hint);
                }
            }
            Mode::DryRunPreview => {
                let pkg = self
                    .info
                    .as_ref()
                    .map(|v| v.pkg.clone())
                    .unwrap_or_default();
                let title = format!("🧪 Dry-run preview — would install {pkg}");
                let text = self.dryrun_lines.clone();
                render_dry_run(f, area, &title, &text);
            }
        }
    }

    fn busy(&self) -> Option<(String, Instant)> {
        if let Some(job) = &self.list_job {
            return Some((job.label.clone(), job.started));
        }
        self.resolve_job
            .as_ref()
            .map(|j| (j.label.clone(), j.started))
    }

    fn help_hints(&self) -> Vec<&'static str> {
        match self.mode {
            Mode::Choosing | Mode::PickingSimilar => {
                vec!["type to filter", "↑↓ navigate", "enter select", "esc back"]
            }
            Mode::ShowingInfo | Mode::DryRunPreview => vec!["enter continue", "esc back"],
            _ => vec![],
        }
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if !yes {
            app.toast("Installation cancelled.", Sev::Info);
            self.finish(app);
            return;
        }
        self.execute_install(app, pending);
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        match tag {
            "install-official" | "install-aur" => {
                let pkg = self.last_install_pkg.take().unwrap_or_default();
                if ok {
                    app.toast(format!("{pkg} installed successfully!"), Sev::Success);
                    app.log(&format!("INSTALL: {pkg} installed successfully"));
                } else {
                    app.toast("Installation failed.", Sev::Error);
                    app.log(&format!("INSTALL: {pkg} installation failed"));
                }
                self.finish(app);
            }
            "install-specific-official" | "install-specific-aur" => {
                let _ = (ok, app); // context toasts handled by the calling screen
            }
            _ => {}
        }
    }
}

fn render_info_panel(
    f: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: &[Vec<Span<'static>>],
    hint: &str,
) {
    let block = widgets::panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let styled: Vec<Line> = lines.iter().map(|l| Line::from(l.clone())).collect();
    f.render_widget(Paragraph::new(styled), rows[0]);
    f.render_widget(
        Paragraph::new(Line::from(widgets::span(hint, widgets::accent())))
            .alignment(Alignment::Center),
        rows[1],
    );
}

fn render_dry_run(f: &mut Frame<'_>, area: Rect, title: &str, lines: &[String]) {
    let block = widgets::panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let spans: Vec<Line> = lines.iter().map(|l| Line::from(l.clone())).collect();
    f.render_widget(Paragraph::new(spans).wrap(Wrap { trim: false }), rows[0]);
    f.render_widget(
        Paragraph::new(Line::from(widgets::span(
            "enter proceed with actual install · esc back",
            widgets::warning(),
        )))
        .alignment(Alignment::Center),
        rows[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_test_screen(pkg: &str) -> InstallScreen {
        InstallScreen {
            mode: Mode::ShowingInfo,
            aur_helper: None,
            choose: empty_choose(),
            similar: empty_similar(),
            info: Some(InfoView {
                pkg: pkg.to_string(),
                source: Source::Official,
                lines: Vec::new(),
            }),
            dryrun_lines: Vec::new(),
            list_job: None,
            resolve_job: None,
            pending: None,
            last_install_pkg: None,
            back_after_done: false,
        }
    }

    #[test]
    fn test_install_confirm_actions_false_native_confirm_true() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "false");
        app.settings_mut().set("NATIVE_CONFIRM", "true");

        let mut screen = make_test_screen("ripgrep");
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
        assert_eq!(queued.args, vec!["pacman", "-S", "ripgrep"]);
        assert!(!queued.args.contains(&"--noconfirm".to_string()));
    }

    #[test]
    fn test_install_confirm_actions_false_native_confirm_false() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "false");
        app.settings_mut().set("NATIVE_CONFIRM", "false");

        let mut screen = make_test_screen("ripgrep");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed
        app.on_key(enter);

        assert!(!app.has_modal());
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(queued.args, vec!["pacman", "-S", "ripgrep", "--noconfirm"]);
    }

    #[test]
    fn test_install_confirm_actions_true_native_confirm_true() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "true");
        app.settings_mut().set("NATIVE_CONFIRM", "true");

        let mut screen = make_test_screen("ripgrep");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed to confirmation
        app.on_key(enter);

        // Modal should be shown
        assert!(app.has_modal());
        assert_eq!(app.modal_prompt(), Some("Install ripgrep?"));
        assert!(app.pending_cmd().is_none());

        // Accepting confirmation executes without --noconfirm
        app.on_key(KeyEvent::from(KeyCode::Char('y')));
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(queued.args, vec!["pacman", "-S", "ripgrep"]);
        assert!(!queued.args.contains(&"--noconfirm".to_string()));
    }

    #[test]
    fn test_install_confirm_actions_true_native_confirm_false() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "true");
        app.settings_mut().set("NATIVE_CONFIRM", "false");

        let mut screen = make_test_screen("ripgrep");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed onto stack; press enter to proceed to confirmation
        app.on_key(enter);

        // Modal should be shown
        assert!(app.has_modal());
        assert_eq!(app.modal_prompt(), Some("Install ripgrep?"));
        assert!(app.pending_cmd().is_none());

        // Accepting confirmation executes WITH --noconfirm because NATIVE_CONFIRM=false
        app.on_key(KeyEvent::from(KeyCode::Char('y')));
        let queued = app.pending_cmd().expect("command should be queued");
        assert_eq!(queued.program, "sudo");
        assert_eq!(queued.args, vec!["pacman", "-S", "ripgrep", "--noconfirm"]);
    }

    #[test]
    fn test_install_modal_cancellation() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("CONFIRM_ACTIONS", "true");

        let mut screen = make_test_screen("ripgrep");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);
        app.on_key(enter);

        assert!(app.has_modal());

        // Reject confirmation with 'n'
        app.on_key(KeyEvent::from(KeyCode::Char('n')));
        assert!(!app.has_modal());
        assert_eq!(app.pending_len(), 0);
    }

    #[test]
    fn test_install_dry_run_full_flow() {
        let mut app = App::new();
        app.set_preflight_env(crate::tx::PreflightEnv::mock());
        app.settings_mut().set("DRY_RUN", "true");

        let mut screen = make_test_screen("ripgrep");
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        screen.handle_key(&mut app, enter);

        // PreviewScreen is pushed; press Enter
        app.on_key(enter);

        // DRY_RUN pops without queueing any command
        assert_eq!(app.pending_len(), 0);
    }
}
