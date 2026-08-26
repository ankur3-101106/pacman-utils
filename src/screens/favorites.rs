// ──────────────────────────────────────────────────────────────────────
// favorites.rs — Favorite packages (lib/favorites.sh)
//
// ~/.config/archman/favorites.txt management plus bulk install flows.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::collections::HashSet;

use crate::app::{App, ExtCmd, Screen};
use crate::settings::favorites_file;
use crate::sys::{self};
use crate::widgets::{self, FuzzyList, Menu, Sev};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Root,
    RemovePick,
}

enum Await {
    Add,
    InstallBatch { official: Vec<String>, aur: Vec<String> },
}

pub struct FavoritesScreen {
    mode: Mode,
    root: Menu,
    remove_pick: FuzzyList,
    favs: Vec<String>,
    installed: HashSet<String>,
    await_kind: Option<Await>,
}

const MENU_ITEMS: [&str; 5] = [
    "➕ Add package to favorites",
    "➖ Remove package from favorites",
    "📦 Install all favorites",
    "📦 Install missing favorites",
    "🔙 Back to Main Menu",
];

fn root_menu() -> Menu {
    Menu::new("Favorite actions", MENU_ITEMS.map(str::to_string).to_vec())
}

fn read_favs() -> Vec<String> {
    std::fs::read_to_string(favorites_file())
        .map(|c| {
            c.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn write_favs(favs: &[String]) {
    if let Some(dir) = favorites_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = if favs.is_empty() { String::new() } else { favs.join("\n") + "\n" };
    let _ = std::fs::write(favorites_file(), body);
}

impl FavoritesScreen {
    pub fn new() -> Self {
        let favs = read_favs();
        let installed = sys::installed_set();
        Self {
            mode: Mode::Root,
            root: root_menu(),
            remove_pick: FuzzyList::new("Select package to remove...", Vec::new()),
            favs,
            installed,
            await_kind: None,
        }
    }

    fn persist(&self) {
        write_favs(&self.favs);
    }

    /// Split targets into official/AUR with one local `pacman -Si` call,
    /// then open the confirm modal.
    fn start_batch(&mut self, app: &mut App, targets: Vec<String>, what: &str) {
        app.log(&format!("FAVORITES: Installing {what}"));
        let (official, aur) = sys::filter_official(&targets);
        app.confirm(format!("Install {what}?"), false);
        self.await_kind = Some(Await::InstallBatch { official, aur });
    }
}

impl Default for FavoritesScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for FavoritesScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            Mode::Root => {
                if self.root.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => match self.root.selected {
                        0 => {
                            self.await_kind = Some(Await::Add);
                            app.ask_input("Package name");
                        }
                        1 => {
                            if self.favs.is_empty() {
                                app.toast("No favorites to remove.", Sev::Warn);
                            } else {
                                let items = self.favs.clone();
                                self.remove_pick =
                                    FuzzyList::new("Select package to remove...", items);
                                self.mode = Mode::RemovePick;
                            }
                        }
                        2 => {
                            if self.favs.is_empty() {
                                app.toast("No favorites to install.", Sev::Warn);
                            } else {
                                let n = self.favs.len();
                                self.start_batch(
                                    app,
                                    self.favs.clone(),
                                    &format!("all {n} favorite packages"),
                                );
                            }
                        }
                        3 => {
                            if self.favs.is_empty() {
                                app.toast("No favorites to install.", Sev::Warn);
                            } else {
                                let missing: Vec<String> = self
                                    .favs
                                    .iter()
                                    .filter(|p| !self.installed.contains(*p))
                                    .cloned()
                                    .collect();
                                if missing.is_empty() {
                                    app.toast(
                                        "All favorite packages are already installed!",
                                        Sev::Success,
                                    );
                                } else {
                                    let n = missing.len();
                                    self.start_batch(app, missing, &format!("{n} missing packages"));
                                }
                            }
                        }
                        _ => app.pop(),
                    },
                    KeyCode::Esc | KeyCode::Char('q') => app.pop(),
                    _ => {}
                }
            }
            Mode::RemovePick => {
                if self.remove_pick.handle_key(&key) {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if let Some(pkg) = self.remove_pick.take_selected() {
                            self.favs.retain(|p| *p != pkg);
                            self.persist();
                            app.toast(format!("Removed {pkg} from favorites."), Sev::Success);
                            app.log(&format!("FAVORITES: Removed {pkg}"));
                        }
                        self.mode = Mode::Root;
                    }
                    KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Root,
                    _ => {}
                }
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        if self.mode == Mode::RemovePick {
            self.remove_pick.render(f, area);
            return;
        }

        // Favorites overview + action menu.
        let rows =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(MENU_ITEMS.len() as u16 + 2)])
                .split(area);

        let title = format!("⭐  Favorite Packages ({})", self.favs.len());
        let block = widgets::panel(&title);
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let mut lines: Vec<Line> = Vec::new();
        if self.favs.is_empty() {
            lines.push(Line::from(widgets::span(
                "  No favorites yet. Add packages to get started!",
                widgets::dim(),
            )));
        } else {
            for pkg in &self.favs {
                let (tag, style) = if self.installed.contains(pkg) {
                    ("[installed]", widgets::success())
                } else {
                    ("[not installed]", widgets::dim())
                };
                lines.push(Line::from(vec![
                    widgets::span("★ ", widgets::warning()),
                    Span::styled(pkg.clone(), Style::new()),
                    widgets::span(format!(" {tag}"), style),
                ]));
            }
        }
        f.render_widget(Paragraph::new(lines), inner);

        self.root.render(f, rows[1]);
    }

    fn help_hints(&self) -> Vec<&'static str> {
        match self.mode {
            Mode::Root => vec!["↑↓ navigate", "enter select", "esc back"],
            Mode::RemovePick => vec!["type to filter", "enter remove", "esc back"],
        }
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let Some(await_kind) = self.await_kind.take() else { return };
        if !matches!(await_kind, Await::Add) {
            return;
        }
        let pkg = value.trim().to_string();
        if pkg.is_empty() {
            return;
        }
        if self.favs.iter().any(|p| *p == pkg) {
            app.toast(format!("{pkg} is already in favorites."), Sev::Warn);
            return;
        }
        self.favs.push(pkg.clone());
        self.persist();
        app.toast(format!("Added {pkg} to favorites!"), Sev::Success);
        app.log(&format!("FAVORITES: Added {pkg}"));
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(await_kind) = self.await_kind.take() else { return };
        let Await::InstallBatch { official, aur } = await_kind else { return };
        if !yes {
            return;
        }
        let has_official = !official.is_empty();
        let has_aur = !aur.is_empty();

        if has_official {
            let n = official.len();
            app.toast(format!("Installing {n} official packages..."), Sev::Info);
            let mut cmd_args = vec!["-S".to_string(), "--needed".to_string()];
            cmd_args.extend(official);
            app.queue_ext(
                ExtCmd::new("fav-official", "sudo", &cmd_args).result(
                    "Official favorites installed.",
                    "Official favorites installation failed.",
                    "FAVORITES: official batch finished",
                ),
            );
        }
        if has_aur {
            let n = aur.len();
            match app.aur_helper() {
                Some(helper) => {
                    app.toast(format!("Installing {n} AUR packages via {helper}..."), Sev::Info);
                    let mut cmd_args = vec!["-S".to_string(), "--needed".to_string()];
                    cmd_args.extend(aur);
                    app.queue_ext(
                    ExtCmd::new("fav-aur", &helper, &cmd_args).result(
                        "Favorites installation complete!",
                        "AUR favorites installation failed.",
                        "FAVORITES: batch finished",
                    ),
                );
                }
                None => {
                    app.toast(format!("Skipping {n} AUR packages — no AUR helper found."), Sev::Warn);
                    app.log("FAVORITES: skipped AUR packages (no helper)");
                }
            }
        }
        if !has_official && !has_aur {
            app.toast("Favorites installation complete!", Sev::Success);
            app.log("FAVORITES: Installation complete");
        }
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        match tag {
            "fav-official" => {
                if ok {
                    app.toast("Official favorites installed.", Sev::Success);
                } else {
                    app.toast("Official favorites installation failed.", Sev::Error);
                }
                self.installed = sys::installed_set();
            }
            "fav-aur" => {
                if ok {
                    app.toast("Favorites installation complete!", Sev::Success);
                    app.log("FAVORITES: Installation complete");
                } else {
                    app.toast("AUR favorites installation failed.", Sev::Error);
                    app.log("FAVORITES: AUR installation failed");
                }
                self.installed = sys::installed_set();
            }
            _ => {}
        }
    }
}
