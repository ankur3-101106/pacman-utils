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

use super::{args, info_lines};

const OFFICIAL_FIELDS: [&str; 5] =
    ["Name", "Version", "Repository", "Description", "Download Size"];
const AUR_FIELDS: [&str; 5] = ["Name", "Version", "Description", "Maintainer", "Votes"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Fetching `pacman -Slq`.
    LoadingList,
    Choosing,
    Resolving,
    ShowingInfo,
    DryRunPreview,
    PickingSimilar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Official,
    Aur,
}

/// Result of the background repo/AUR resolution.
enum Found {
    Official { pkg: String, raw: String },
    Aur { pkg: String, raw: String },
    NotFound { official: Vec<String>, aur: Vec<String> },
}

struct InfoView {
    pkg: String,
    source: Source,
    lines: Vec<Vec<Span<'static>>>,
}

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
            mode: if preselect { Mode::Resolving } else { Mode::LoadingList },
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
                    aur: helper.as_ref().map(|h| sys::aur_ss(h, &pkg)).unwrap_or_default(),
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
}

/// Shared with search/groups flows: queue an install for one package,
/// official repos first, then AUR.
pub fn install_specific_package(app: &mut App, pkg: &str) {
    if sys::si(&[pkg]).is_some() {
        app.log(&format!("INSTALL: Installing {pkg} from official repos"));
        app.queue_ext(
            ExtCmd::new("install-specific-official", "sudo", &args(&["pacman", "-S", pkg]))
                .note(format!("Install {pkg}"))
                .result(
                    format!("{pkg} installed successfully!"),
                    "Installation failed.",
                    "INSTALL: official install finished",
                ),
        );
    } else if let Some(helper) = app.aur_helper() {
        app.log(&format!("INSTALL: Installing {pkg} from AUR via {helper}"));
        app.queue_ext(
            ExtCmd::new("install-specific-aur", &helper, &args(&["-S", pkg]))
                .note(format!("Install {pkg} from AUR via {helper}"))
                .result(
                    format!("{pkg} installed successfully!"),
                    "Installation failed.",
                    "INSTALL: AUR install finished",
                ),
        );
    } else {
        app.toast(format!("Cannot install {pkg} — no AUR helper found."), Sev::Error);
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
                let pkg = self.info.as_ref().map(|v| v.pkg.clone()).unwrap_or_default();
                match key.code {
                    KeyCode::Enter => match source {
                        Some(Source::Official) => {
                            if app.settings().is_true("DRY_RUN") {
                                self.dryrun_lines =
                                    sys::capture("pacman", &["-S", "--print", &pkg])
                                        .map(|out| out.lines().map(str::to_string).collect())
                                        .unwrap_or_else(|| {
                                            vec!["(dry-run preview unavailable)".to_string()]
                                        });
                                self.mode = Mode::DryRunPreview;
                            } else {
                                app.confirm(format!("Install {pkg}?"), false);
                                self.pending = Some(Pending::Official(pkg));
                            }
                        }
                        Some(Source::Aur) => {
                            app.confirm(format!("Install {pkg} from AUR?"), false);
                            self.pending = Some(Pending::Aur(pkg));
                        }
                        None => {}
                    },
                    KeyCode::Esc | KeyCode::Char('q') => self.finish(app),
                    _ => {}
                }
            }
            Mode::DryRunPreview => match key.code {
                KeyCode::Enter => {
                    let pkg = self.info.as_ref().map(|v| v.pkg.clone()).unwrap_or_default();
                    app.confirm("Proceed with actual install?", false);
                    self.pending = Some(Pending::Official(pkg));
                }
                KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::ShowingInfo,
                _ => {}
            },
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
                        self.choose =
                            FuzzyList::new("Select package to install...", items);
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
                let pkg = self.info.as_ref().map(|v| v.pkg.clone()).unwrap_or_default();
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
        self.resolve_job.as_ref().map(|j| (j.label.clone(), j.started))
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
        let Some(pending) = self.pending.take() else { return };
        if !yes {
            app.toast("Installation cancelled.", Sev::Info);
            self.finish(app);
            return;
        }
        match pending {
            Pending::Official(pkg) => {
                self.last_install_pkg = Some(pkg.clone());
                app.log(&format!("INSTALL: Installing {pkg} from official repos"));
                app.queue_ext(
                    ExtCmd::new("install-official", "sudo", &args(&["pacman", "-S", &pkg]))
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
                    app.queue_ext(
                        ExtCmd::new("install-aur", &helper, &args(&["-S", &pkg]))
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
