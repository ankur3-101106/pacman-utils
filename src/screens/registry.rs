// ──────────────────────────────────────────────────────────────────────
// registry.rs — Declarative category → action table
//
// The whole user-facing surface of archman is described here, linutil
// style: categories hold flat lists of actions; an action either opens
// a focused screen or runs commands directly (optionally behind a
// confirmation prompt). Adding a feature = adding a row here plus its
// module.
// ──────────────────────────────────────────────────────────────────────

use crate::app::{App, ExtCmd, Screen};

/// What happens when the user presses enter on an action.
pub enum Launch {
    /// Opens an interactive flow.
    Screen(fn(&App) -> Box<dyn Screen>),
    /// Runs right away (after its confirmation prompt, if any).
    Run(RunSpec),
}

#[derive(Clone, Copy)]
pub struct RunSpec {
    /// Tag routed back through [`App`] for toasts/logs.
    pub tag: &'static str,
    /// Some(prompt) → ask before running; `danger` styles the dialog.
    pub confirm: fn(&App) -> Option<String>,
    pub danger: bool,
    /// Queues commands / performs the action.
    pub build: fn(&mut App),
    pub ok_msg: &'static str,
    pub fail_msg: &'static str,
    pub done_log: &'static str,
}

pub struct ActionDef {
    pub label: &'static str,
    /// One-liner shown in the description pane.
    pub desc: &'static str,
    pub launch: Launch,
}

pub struct CatDef {
    pub title: &'static str,
    pub actions: &'static [ActionDef],
}

// ── Confirmation helpers ────────────────────────────────────────────

fn no_confirm(_: &App) -> Option<String> {
    None
}

fn confirm_upgrade_pacman(app: &App) -> Option<String> {
    app.settings()
        .is_true("CONFIRM_ACTIONS")
        .then(|| "Perform full system upgrade?".to_string())
}

fn confirm_upgrade_aur(app: &App) -> Option<String> {
    app.settings()
        .is_true("CONFIRM_ACTIONS")
        .then(|| "Perform full upgrade (official + AUR) via your AUR helper?".to_string())
}

fn confirm_sc(app: &App) -> Option<String> {
    app.settings()
        .is_true("CONFIRM_ACTIONS")
        .then(|| "Remove all cached packages that are not currently installed?".into())
}

fn confirm_scc(app: &App) -> Option<String> {
    app.settings().is_true("CONFIRM_ACTIONS").then(|| {
        "This will remove ALL cached packages! \
         Are you absolutely sure? This cannot be undone."
            .into()
    })
}

fn confirm_paccache(app: &App) -> Option<String> {
    if !app.settings().is_true("CONFIRM_ACTIONS") {
        return None;
    }
    let keep = app.settings().get("PACCACHE_KEEP");
    Some(format!(
        "Remove all but the last {keep} versions of each package?"
    ))
}

// ── Run builders ────────────────────────────────────────────────────

fn build_db_sy(app: &mut App) {
    let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-Sy"]);
    app.queue_ext(ExtCmd::new("run-db-sy", "sudo", &cmd_args).note("Refreshing package databases"));
}

fn build_db_syy(app: &mut App) {
    let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-Syy"]);
    app.queue_ext(
        ExtCmd::new("run-db-syy", "sudo", &cmd_args).note("Force refreshing package databases"),
    );
}

fn build_upgrade_pacman(app: &mut App) {
    let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-Syu"]);
    app.queue_ext(ExtCmd::new("run-upgrade", "sudo", &cmd_args).note("Full system upgrade"));
}

fn build_upgrade_aur(app: &mut App) {
    match crate::sys::get_aur_helper(app.settings()) {
        Some(helper) => {
            let cmd_args = crate::sys::aur_args(app.settings(), &["-Syu"]);
            app.queue_ext(
                ExtCmd::new("run-upgrade-aur", &helper, &cmd_args)
                    .note(format!("Full upgrade via {helper}")),
            );
        }
        None => {
            app.toast(
                "No AUR helper found. Install yay or paru (Settings ▸ Dependency Check).",
                crate::widgets::Sev::Error,
            );
            app.log("UPDATE: skipped AUR upgrade — no helper");
        }
    }
}

fn build_cache_keep(app: &mut App) {
    if !app.caps().paccache {
        app.toast(
            "paccache not found — install pacman-contrib (Information ▸ Dependency Check).",
            crate::widgets::Sev::Warn,
        );
        app.log("CACHE: skipped — paccache missing");
        return;
    }
    let keep = app.settings().get("PACCACHE_KEEP").to_string();
    app.log(&format!("CACHE: Keeping last {keep} versions"));
    app.queue_ext(
        ExtCmd::new(
            "run-cache-keep",
            "sudo",
            &super::args(&["paccache", &format!("-rk{keep}")]),
        )
        .note(format!("Keep last {keep} package versions")),
    );
}

fn build_cache_sc(app: &mut App) {
    let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-Sc"]);
    app.queue_ext(
        ExtCmd::new("run-cache-sc", "sudo", &cmd_args)
            .note("Remove cached packages for uninstalled software"),
    );
}

fn build_cache_scc(app: &mut App) {
    let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-Scc"]);
    app.queue_ext(
        ExtCmd::new("run-cache-scc", "sudo", &cmd_args).note("Remove ALL cached packages"),
    );
}

fn write_list(path: &std::path::Path, names: Vec<String>) -> Result<usize, String> {
    let count = names.len();
    let body = if names.is_empty() {
        String::new()
    } else {
        names.join("\n") + "\n"
    };
    std::fs::write(path, body).map_err(|e| e.to_string())?;
    Ok(count)
}

fn build_export_explicit(app: &mut App) {
    let path = crate::sys::home_path("pkglist-explicit.txt");
    let names = crate::sys::explicit_names();
    match write_list(&path, names) {
        Ok(n) => {
            app.toast(
                format!("Exported {n} explicit packages → {}", path.display()),
                crate::widgets::Sev::Success,
            );
            app.log(&format!(
                "EXPORT: {n} explicit packages to {}",
                path.display()
            ));
        }
        Err(e) => app.toast(format!("Export failed: {e}"), crate::widgets::Sev::Error),
    }
}

fn build_export_aur(app: &mut App) {
    let path = crate::sys::home_path("pkglist-aur.txt");
    let names = crate::sys::foreign_names();
    if names.is_empty() {
        app.toast("No AUR packages found.", crate::widgets::Sev::Warn);
        return;
    }
    match write_list(&path, names) {
        Ok(n) => {
            app.toast(
                format!("Exported {n} AUR packages → {}", path.display()),
                crate::widgets::Sev::Success,
            );
            app.log(&format!("EXPORT: {n} AUR packages to {}", path.display()));
        }
        Err(e) => app.toast(format!("Export failed: {e}"), crate::widgets::Sev::Error),
    }
}

fn build_export_all(app: &mut App) {
    let exp = crate::sys::home_path("pkglist-explicit.txt");
    let aur = crate::sys::home_path("pkglist-aur.txt");
    let r1 = write_list(&exp, crate::sys::explicit_names());
    let r2 = write_list(&aur, crate::sys::foreign_names());
    match (r1, r2) {
        (Ok(a), Ok(b)) => {
            app.toast(
                format!(
                    "Exported: official {a} → {}; AUR {b} → {}",
                    exp.display(),
                    aur.display()
                ),
                crate::widgets::Sev::Success,
            );
            app.log(&format!("EXPORT: {a} official + {b} AUR packages"));
        }
        _ => app.toast("Export failed.", crate::widgets::Sev::Error),
    }
}

// ── Screen openers ──────────────────────────────────────────────────

mod open {
    use super::*;
    use crate::screens::*;

    pub fn install(app: &App) -> Box<dyn Screen> {
        Box::new(install::InstallScreen::new(app))
    }
    pub fn search(app: &App) -> Box<dyn Screen> {
        Box::new(search::SearchScreen::new(app))
    }
    pub fn remove(_: &App) -> Box<dyn Screen> {
        Box::new(remove::RemoveScreen::new())
    }
    pub fn browse_installed(_: &App) -> Box<dyn Screen> {
        packages::BrowsePackagesScreen::new(false)
    }
    pub fn browse_explicit(_: &App) -> Box<dyn Screen> {
        packages::BrowsePackagesScreen::new(true)
    }
    pub fn files_pick(_: &App) -> Box<dyn Screen> {
        packages::FilesPickScreen::new()
    }
    pub fn owner_query(_: &App) -> Box<dyn Screen> {
        packages::OwnerQueryScreen::new()
    }
    pub fn pkg_info(_: &App) -> Box<dyn Screen> {
        packages::InfoQueryScreen::new()
    }
    pub fn history(_: &App) -> Box<dyn Screen> {
        packages::history_viewer()
    }
    pub fn txn_log(_: &App) -> Box<dyn Screen> {
        packages::TransactionLogScreen::new()
    }
    pub fn reinstall(_: &App) -> Box<dyn Screen> {
        packages::ReinstallPickScreen::new()
    }
    pub fn orphans(_: &App) -> Box<dyn Screen> {
        packages::OrphansScreen::new()
    }
    pub fn import(_: &App) -> Box<dyn Screen> {
        exportimport::ImportScreen::new()
    }
    pub fn updates(_: &App) -> Box<dyn Screen> {
        Box::new(update::UpdateScreen::new())
    }
    pub fn lockfile(_: &App) -> Box<dyn Screen> {
        Box::new(lockfile::LockfileScreen::new())
    }
    pub fn mirrors(app: &App) -> Box<dyn Screen> {
        Box::new(mirrors::MirrorsScreen::new(app))
    }
    pub fn sysinfo(app: &App) -> Box<dyn Screen> {
        info::InfoScreen::new_with_app(app)
    }
    pub fn dep_check(app: &App) -> Box<dyn Screen> {
        settingsscr::DepsCheckScreen::new(app)
    }
    pub fn favorites(_: &App) -> Box<dyn Screen> {
        Box::new(favorites::FavoritesScreen::new())
    }
    pub fn groups(_: &App) -> Box<dyn Screen> {
        Box::new(groups::GroupsScreen::new())
    }
    pub fn settings(app: &App) -> Box<dyn Screen> {
        settingsscr::SettingsScreen::new(app)
    }
}

// ── The table ───────────────────────────────────────────────────────

macro_rules! run_spec {
    ($tag:expr, $confirm:expr, $danger:expr, $build:expr, $ok:expr, $fail:expr, $log:expr) => {
        RunSpec {
            tag: $tag,
            confirm: $confirm,
            danger: $danger,
            build: $build,
            ok_msg: $ok,
            fail_msg: $fail,
            done_log: $log,
        }
    };
}

// The `run = …` arm MUST come first: otherwise `$open:expr` happily
// parses `run = run_spec!(…)` as an assignment expression.
macro_rules! action {
    ($label:expr, $desc:expr, run = $spec:expr) => {
        ActionDef {
            label: $label,
            desc: $desc,
            launch: Launch::Run($spec),
        }
    };
    ($label:expr, $desc:expr, $open:expr) => {
        ActionDef {
            label: $label,
            desc: $desc,
            launch: Launch::Screen($open),
        }
    };
}

pub static PACKAGES: [ActionDef; 15] = [
    action!(
        "Install Package",
        "Fuzzy-search every repository and AUR package, review details, install.",
        open::install
    ),
    action!(
        "Search Packages",
        "Search official repos and the AUR, then jump straight into installing.",
        open::search
    ),
    action!(
        "Remove Package",
        "Uninstall an explicitly installed package (-R, -Rs or -Rns).",
        open::remove
    ),
    action!(
        "Browse Installed Packages",
        "Browse everything installed and inspect full package details.",
        open::browse_installed
    ),
    action!(
        "Browse Explicit Packages",
        "Browse only the packages you deliberately installed.",
        open::browse_explicit
    ),
    action!(
        "Files Owned by Package",
        "List every file that belongs to an installed package.",
        open::files_pick
    ),
    action!(
        "Which Package Owns a File?",
        "Trace an absolute path back to the package that installed it.",
        open::owner_query
    ),
    action!(
        "Package Info Lookup",
        "Detailed information for any package — local database, repos or AUR.",
        open::pkg_info
    ),
    action!(
        "Installation History",
        "Recent installed / upgraded / removed events from pacman.log.",
        open::history
    ),
    action!(
        "Transaction Log Viewer",
        "Browse pacman.log filtered by installs, upgrades, removals or errors.",
        open::txn_log
    ),
    action!(
        "Reinstall Package",
        "Force-reinstall a package, overwriting any modified files.",
        open::reinstall
    ),
    action!(
        "Remove Orphan Packages",
        "Detect dependencies no longer needed and remove them all.",
        open::orphans
    ),
    action!(
        "Export Explicit List",
        "Write native package names to ~/pkglist-explicit.txt for backups.",
        run = run_spec!(
            "run-export-explicit",
            no_confirm,
            false,
            build_export_explicit,
            "Explicit list exported.",
            "Export failed.",
            "EXPORT: explicit list"
        )
    ),
    action!(
        "Export AUR List",
        "Write foreign (AUR) package names to ~/pkglist-aur.txt.",
        run = run_spec!(
            "run-export-aur",
            no_confirm,
            false,
            build_export_aur,
            "AUR list exported.",
            "Export failed.",
            "EXPORT: AUR list"
        )
    ),
    action!(
        "Export All Lists",
        "Write both backup lists (~/pkglist-explicit.txt and ~/pkglist-aur.txt).",
        run = run_spec!(
            "run-export-all",
            no_confirm,
            false,
            build_export_all,
            "Package lists exported.",
            "Export failed.",
            "EXPORT: all lists"
        )
    ),
];

pub static SYSTEM: [ActionDef; 5] = [
    action!(
        "Check for Updates",
        "List available updates and pick a database refresh or full upgrade.",
        open::updates
    ),
    action!(
        "Refresh Databases (-Sy)",
        "Sync package databases from the mirrors.",
        run = run_spec!(
            "run-db-sy",
            no_confirm,
            false,
            build_db_sy,
            "Package databases refreshed!",
            "Database refresh failed.",
            "UPDATE: database refresh finished"
        )
    ),
    action!(
        "Force Refresh Databases (-Syy)",
        "Re-download databases even if they look up to date.",
        run = run_spec!(
            "run-db-syy",
            no_confirm,
            false,
            build_db_syy,
            "Package databases force refreshed!",
            "Force refresh failed.",
            "UPDATE: force refresh finished"
        )
    ),
    action!(
        "Full System Upgrade (pacman)",
        "pacman -Syu — every official package brought up to date.",
        run = run_spec!(
            "run-upgrade",
            confirm_upgrade_pacman,
            false,
            build_upgrade_pacman,
            "System upgrade complete!",
            "System upgrade failed.",
            "UPDATE: pacman upgrade finished"
        )
    ),
    action!(
        "Full Upgrade incl. AUR (yay/paru)",
        "Upgrade official repos and AUR packages in one go.",
        run = run_spec!(
            "run-upgrade-aur",
            confirm_upgrade_aur,
            false,
            build_upgrade_aur,
            "Full upgrade complete!",
            "Upgrade failed.",
            "UPDATE: AUR upgrade finished"
        )
    ),
];

pub static MAINTENANCE: [ActionDef; 4] = [
    action!(
        "Clean Package Cache (keep recent)",
        "Keep only the newest N versions of each package (paccache -rkN).",
        run = run_spec!(
            "run-cache-keep",
            confirm_paccache,
            false,
            build_cache_keep,
            "Cache cleaned!",
            "Cache cleaning failed.",
            "CACHE: paccache finished"
        )
    ),
    action!(
        "Remove Uninstalled Caches (-Sc)",
        "Drop cached packages that are no longer installed.",
        run = run_spec!(
            "run-cache-sc",
            confirm_sc,
            false,
            build_cache_sc,
            "Cache cleaned!",
            "Cache cleaning failed.",
            "CACHE: -Sc finished"
        )
    ),
    action!(
        "Wipe ENTIRE Cache (-Scc)",
        "Delete every cached package archive. Frees the most space.",
        run = run_spec!(
            "run-cache-scc",
            confirm_scc,
            true,
            build_cache_scc,
            "Cache wiped!",
            "Cache wipe failed.",
            "CACHE: -Scc finished"
        )
    ),
    action!(
        "Remove Pacman Lockfile",
        "Inspect /var/lib/pacman/db.lck and remove it safely.",
        open::lockfile
    ),
];

pub static MIRRORS: [ActionDef; 1] = [action!(
    "Mirror Management",
    "Auto-update or rank mirrors with reflector, back up and restore the mirrorlist.",
    open::mirrors
)];

pub static INFORMATION: [ActionDef; 2] = [
    action!(
        "System Dashboard",
        "Hostname, kernel, uptime, package counts, cache size, disk usage.",
        open::sysinfo
    ),
    action!(
        "Dependency Check",
        "Report optional tools (reflector, pacman-contrib, AUR helper) and offer installs.",
        open::dep_check
    ),
];

pub static EXTRAS: [ActionDef; 3] = [
    action!(
        "Favorite Packages",
        "Manage your favorite list and bulk-install it on fresh systems.",
        open::favorites
    ),
    action!(
        "Package Groups",
        "Curated setups: GNOME, KDE, Hyprland, dev tools, gaming, fonts…",
        open::groups
    ),
    action!(
        "Import Package List",
        "Install packages from a pkglist text file via pacman or your AUR helper.",
        open::import
    ),
];

pub static SETTINGS: [ActionDef; 1] = [action!(
    "Settings",
    "AUR helper, dry-run mode, confirmations, logging, cache retention.",
    open::settings
)];

pub static CATEGORIES: [CatDef; 7] = [
    CatDef {
        title: "📦 Packages",
        actions: &PACKAGES,
    },
    CatDef {
        title: "⬆  System",
        actions: &SYSTEM,
    },
    CatDef {
        title: "🧹 Maintenance",
        actions: &MAINTENANCE,
    },
    CatDef {
        title: "🌍 Mirrors",
        actions: &MIRRORS,
    },
    CatDef {
        title: "📊 Information",
        actions: &INFORMATION,
    },
    CatDef {
        title: "⭐ Extras",
        actions: &EXTRAS,
    },
    CatDef {
        title: "⚙  Settings",
        actions: &SETTINGS,
    },
];
