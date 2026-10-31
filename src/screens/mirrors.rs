// ──────────────────────────────────────────────────────────────────────
// mirrors.rs — Provider-Aware Multi-Distribution Mirror Management
//
// Integrated provider discovery, per-provider mirrorlist resolution,
// Reflector & rate-mirrors ranking, real-time benchmarking, timestamped
// backup & restore, bulk ranking, and custom provider configuration.
// ──────────────────────────────────────────────────────────────────────

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, ExtCmd, Screen};
use crate::mirrors::{
    add_custom_provider, build_backup_command, build_delete_backup_command, build_restore_command,
    create_backup, create_ranking_plan, current_timestamp, delete_backup, discover_providers,
    list_backups, load_custom_providers, remove_custom_provider, spawn_benchmark_job,
    validate_candidate_file, BackupEntry, BenchmarkProgress, BenchmarkResult, CustomProvider,
    DetectionStatus, MirrorProvider, ProviderId, RankingBackend, RankingPlan,
};
#[allow(unused_imports)]
pub use crate::sys::{MIRRORLIST, MIRRORLIST_BACKUP};
use crate::widgets::{
    self, accent, accent_bold, cat_color, danger, dim, hint, kv_table, panel, panel_color,
    selected_style, span, styled, success, warning, Sev,
};

use super::{args, viewer::ViewerScreen};

/// Display mode discriminant for the mirror manager screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MirrorViewMode {
    /// Main table listing all discovered and supported providers.
    List,
    /// Detailed inspection view for the selected provider.
    Detail,
    /// Bulk selection and ranking workflow.
    BulkRank,
    /// Live performance benchmarking view.
    Benchmark,
    /// Timestamped backup history and restore selector.
    Restore,
    /// Custom provider management view.
    Custom,
}

/// What an open confirmation or input modal refers to.
#[derive(Clone, PartialEq, Eq, Debug)]
enum AwaitKind {
    InstallReflector,
    ApplyRanking,
    RestoreBackup(PathBuf, PathBuf),
    DeleteBackup(PathBuf),
    StartBulkRank,
    DeleteCustomProvider(String),
    EnableRepo(ProviderId),
    RemoveRepo(ProviderId),
}

/// Input prompt stage when registering a custom provider.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CustomAddStep {
    Id,
    Name,
    MirrorlistPath,
    Backend,
}

pub struct MirrorsScreen {
    mode: MirrorViewMode,
    providers: Vec<MirrorProvider>,
    selected_idx: usize,
    list_offset: usize,

    // Detail view state
    detail_mirror_idx: usize,
    detail_scroll: usize,

    // Bulk selection state
    bulk_selected: Vec<bool>,
    bulk_idx: usize,

    // Benchmark state
    bench_rx: Option<Receiver<BenchmarkProgress>>,
    bench_results: Vec<BenchmarkResult>,
    bench_cancel: Option<Arc<AtomicBool>>,
    bench_progress_msg: String,
    bench_finished: bool,
    bench_scroll: usize,

    // Backup & Restore state
    backups: Vec<BackupEntry>,
    selected_backup_idx: usize,

    // Custom provider state
    custom_providers: Vec<CustomProvider>,
    selected_custom_idx: usize,
    custom_add_step: Option<CustomAddStep>,
    custom_add_id: String,
    custom_add_name: String,
    custom_add_path: String,
    custom_add_backend: String,

    // Ranking pending plan
    pending_plan: Option<RankingPlan>,

    // Modal await kind
    await_kind: Option<AwaitKind>,

    // Capability snapshots
    reflector_installed: bool,
    rate_mirrors_installed: bool,
}

impl MirrorsScreen {
    pub fn new(app: &App) -> Self {
        let mut s = Self::new_embedded();
        s.reflector_installed = app.caps().reflector;
        s
    }

    /// Construct screen initialized with system-detected binaries (usable standalone or embedded).
    pub fn new_embedded() -> Self {
        let reflector_installed = crate::sys::has_bin("reflector");
        let rate_mirrors_installed = crate::sys::has_bin("rate-mirrors");
        let providers = discover_providers();
        let bulk_selected = vec![true; providers.len()];
        let custom_providers = load_custom_providers();

        Self {
            mode: MirrorViewMode::List,
            providers,
            selected_idx: 0,
            list_offset: 0,

            detail_mirror_idx: 0,
            detail_scroll: 0,

            bulk_selected,
            bulk_idx: 0,

            bench_rx: None,
            bench_results: Vec::new(),
            bench_cancel: None,
            bench_progress_msg: String::new(),
            bench_finished: false,
            bench_scroll: 0,

            backups: Vec::new(),
            selected_backup_idx: 0,

            custom_providers,
            selected_custom_idx: 0,
            custom_add_step: None,
            custom_add_id: String::new(),
            custom_add_name: String::new(),
            custom_add_path: String::new(),
            custom_add_backend: String::new(),

            pending_plan: None,
            await_kind: None,

            reflector_installed,
            rate_mirrors_installed,
        }
    }

    /// Refresh discovered providers from pacman configuration.
    pub fn refresh_providers(&mut self) {
        self.providers = discover_providers();
        if self.bulk_selected.len() != self.providers.len() {
            self.bulk_selected = vec![true; self.providers.len()];
        }
        if self.selected_idx >= self.providers.len() && !self.providers.is_empty() {
            self.selected_idx = self.providers.len() - 1;
        }
        self.custom_providers = load_custom_providers();
        self.reflector_installed = crate::sys::has_bin("reflector");
        self.rate_mirrors_installed = crate::sys::has_bin("rate-mirrors");
    }

    #[allow(dead_code)]
    pub fn selected_index(&self) -> usize {
        self.selected_idx
    }

    pub fn selected_provider(&self) -> Option<&MirrorProvider> {
        self.providers.get(self.selected_idx)
    }

    pub fn selected_provider_mut(&mut self) -> Option<&mut MirrorProvider> {
        self.providers.get_mut(self.selected_idx)
    }

    /// Switch to detailed inspection of the currently selected provider.
    pub fn open_detail(&mut self) {
        if self.selected_provider().is_some() {
            self.mode = MirrorViewMode::Detail;
            self.detail_mirror_idx = 0;
            self.detail_scroll = 0;
            self.refresh_backups_for_selected();
        }
    }

    /// Refresh backups for the currently selected provider's primary target.
    fn refresh_backups_for_selected(&mut self) {
        if let Some(p) = self.selected_provider() {
            if let Some(target) = p.primary_mirrorlist() {
                self.backups = list_backups(target);
                if self.backups.is_empty() {
                    self.selected_backup_idx = 0;
                } else if self.selected_backup_idx >= self.backups.len() {
                    self.selected_backup_idx = self.backups.len() - 1;
                }
            } else {
                self.backups.clear();
                self.selected_backup_idx = 0;
            }
        }
    }

    /// Start mirror ranking for the currently selected provider.
    pub fn start_rank_selected(&mut self, app: &mut App) {
        let Some(provider) = self.selected_provider().cloned() else {
            return;
        };

        if !provider.is_supported() {
            app.toast(
                format!(
                    "Provider '{}' does not have a supported ranking backend",
                    provider.name
                ),
                Sev::Warn,
            );
            return;
        }

        match &provider.backend {
            RankingBackend::Reflector { .. } => {
                if !self.reflector_installed && !self.rate_mirrors_installed {
                    app.toast(
                        "Neither reflector nor rate-mirrors is installed.",
                        Sev::Warn,
                    );
                    if app.settings().is_true("CONFIRM_ACTIONS") {
                        app.confirm("Install reflector?", false);
                        self.await_kind = Some(AwaitKind::InstallReflector);
                    } else {
                        self.execute_install_reflector(app);
                    }
                    return;
                }
                // Reflector can ask for country filter
                app.ask_input("Country name or code for Reflector (leave blank for global):");
            }
            RankingBackend::RateMirrors { .. } => {
                if !self.rate_mirrors_installed {
                    app.toast("rate-mirrors executable is not installed.", Sev::Error);
                    return;
                }
                self.execute_ranking_for_provider(app, &provider, None);
            }
            RankingBackend::Native { executable, .. } => {
                if !crate::sys::has_bin(executable) {
                    app.toast(format!("{executable} is not installed."), Sev::Error);
                    return;
                }
                self.execute_ranking_for_provider(app, &provider, None);
            }
            RankingBackend::Unsupported => {
                app.toast("Unsupported ranking backend", Sev::Warn);
            }
        }
    }

    fn execute_install_reflector(&self, app: &mut App) {
        let cmd_args = crate::sys::sudo_pacman_args(app.settings(), &["-S", "reflector"]);
        app.queue_ext(
            ExtCmd::new("mirrors-dep", "sudo", &cmd_args)
                .note("Install reflector")
                .result(
                    "reflector installed.",
                    "Failed to install reflector.",
                    "MIRRORS: reflector install finished",
                ),
        );
    }

    /// Build ranking plan and queue candidate generation.
    fn execute_ranking_for_provider(
        &mut self,
        app: &mut App,
        provider: &MirrorProvider,
        country: Option<String>,
    ) {
        let reflector_opts = country.map(|c| crate::mirrors::ReflectorOptions {
            country: Some(c),
            ..Default::default()
        });

        match create_ranking_plan(provider, None, reflector_opts.as_ref(), None) {
            Ok(plan) => {
                app.log(&format!(
                    "MIRRORS: Ranking mirrors for {} via {}",
                    provider.name,
                    provider.backend.name()
                ));
                app.toast(
                    format!("Testing & ranking mirrors for {}...", provider.name),
                    Sev::Info,
                );

                self.pending_plan = Some(plan.clone());
                app.queue_ext(plan.generate_cmd.result(
                    "Candidate mirrors generated. Validating...",
                    "Mirror ranking failed.",
                    "MIRRORS: candidate generation finished",
                ));
            }
            Err(e) => {
                app.toast(format!("Ranking setup failed: {e}"), Sev::Error);
            }
        }
    }

    /// Start real-time benchmarking for the selected provider's active mirrors.
    pub fn start_benchmark_selected(&mut self, app: &mut App) {
        let Some(p) = self.selected_provider() else {
            return;
        };

        let mut urls = Vec::new();
        for target in &p.mirrorlists {
            for mirror in &target.active_mirrors {
                if !urls.contains(&mirror.url) {
                    urls.push(mirror.url.clone());
                }
            }
        }

        if urls.is_empty() {
            app.toast(
                format!("No active mirrors found for provider '{}'", p.name),
                Sev::Warn,
            );
            return;
        }

        app.toast(
            format!("Starting benchmark for {} mirrors...", urls.len()),
            Sev::Info,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_benchmark_job(urls, Arc::clone(&cancel));

        self.bench_rx = Some(rx);
        self.bench_cancel = Some(cancel);
        self.bench_results.clear();
        self.bench_finished = false;
        self.bench_scroll = 0;
        self.bench_progress_msg = "Initializing mirror benchmark...".to_string();
        self.mode = MirrorViewMode::Benchmark;
    }

    /// Start benchmarking all detected providers.
    pub fn start_benchmark_all_detected(&mut self, app: &mut App) {
        let mut urls = Vec::new();
        for p in &self.providers {
            if p.detection.is_detected() {
                for target in &p.mirrorlists {
                    for mirror in &target.active_mirrors {
                        if !urls.contains(&mirror.url) {
                            urls.push(mirror.url.clone());
                        }
                    }
                }
            }
        }

        if urls.is_empty() {
            app.toast(
                "No active mirrors found across detected providers",
                Sev::Warn,
            );
            return;
        }

        app.toast(
            format!(
                "Benchmarking all {} mirrors across detected providers...",
                urls.len()
            ),
            Sev::Info,
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_benchmark_job(urls, Arc::clone(&cancel));

        self.bench_rx = Some(rx);
        self.bench_cancel = Some(cancel);
        self.bench_results.clear();
        self.bench_finished = false;
        self.bench_scroll = 0;
        self.bench_progress_msg = "Benchmarking all detected providers...".to_string();
        self.mode = MirrorViewMode::Benchmark;
    }

    /// Create manual backup for the selected provider.
    pub fn backup_selected(&mut self, app: &mut App) {
        let Some(p) = self.selected_provider() else {
            return;
        };
        let Some(target) = p.primary_mirrorlist() else {
            app.toast("No mirrorlist file resolved for provider", Sev::Warn);
            return;
        };

        let backup_path = crate::mirrors::generate_backup_path(target);
        app.log(&format!(
            "MIRRORS: Backing up {} to {}",
            target.display(),
            backup_path.display()
        ));

        let cmd = build_backup_command(target, &backup_path);
        app.queue_ext(cmd);
    }

    /// Create manual backup for the selected provider in live time if possible,
    /// falling back to external command runner if elevated privileges are needed.
    pub fn backup_selected_live(&mut self, app: &mut App) {
        let Some(p) = self.selected_provider() else {
            return;
        };
        let Some(target) = p.primary_mirrorlist() else {
            app.toast("No mirrorlist file resolved for provider", Sev::Warn);
            return;
        };

        match create_backup(target) {
            Ok(entry) => {
                let fname = entry.filename();
                self.backups.insert(0, entry);
                self.selected_backup_idx = 0;
                app.toast(format!("Backup '{fname}' created."), Sev::Success);
                app.log(&format!("MIRRORS: Created backup {fname} in live time"));
            }
            Err(_) => {
                self.backup_selected(app);
            }
        }
    }

    /// Open backup history restoration selector.
    pub fn open_restore_selector(&mut self, app: &mut App) {
        self.refresh_backups_for_selected();
        if self.backups.is_empty() {
            app.toast("No existing backups found for this mirrorlist", Sev::Warn);
            return;
        }
        self.mode = MirrorViewMode::Restore;
        self.selected_backup_idx = 0;
    }

    /// Execute bulk ranking across all selected providers.
    pub fn execute_bulk_rank(&mut self, app: &mut App) {
        let mut to_rank = Vec::new();
        for (i, &selected) in self.bulk_selected.iter().enumerate() {
            if selected && i < self.providers.len() {
                let p = &self.providers[i];
                if p.is_supported() && (p.detection.is_detected() || p.is_custom) {
                    to_rank.push(p.clone());
                }
            }
        }

        if to_rank.is_empty() {
            app.toast(
                "No supported providers selected for bulk ranking",
                Sev::Warn,
            );
            return;
        }

        app.toast(
            format!("Starting bulk ranking for {} providers...", to_rank.len()),
            Sev::Info,
        );
        app.log(&format!(
            "MIRRORS: Starting bulk ranking of {} providers",
            to_rank.len()
        ));

        // Queue ranking operations
        for provider in to_rank {
            self.execute_ranking_for_provider(app, &provider, None);
        }
    }

    /// Enable third-party repository if supported for the selected provider.
    pub fn enable_selected_repo(&mut self, app: &mut App) {
        let Some(p) = self.selected_provider() else {
            return;
        };
        let id = p.id.as_str();
        if !matches!(id, "chaotic-aur" | "cachyos" | "blackarch") {
            app.toast(
                format!("Direct repository setup not supported for '{}'", p.name),
                Sev::Warn,
            );
            return;
        }

        let is_enabled = match id {
            "chaotic-aur" => crate::sys::is_chaotic_aur_enabled(),
            "cachyos" => crate::sys::is_cachyos_repo_enabled(),
            "blackarch" => crate::sys::is_blackarch_enabled(),
            _ => false,
        };

        if is_enabled {
            app.toast(format!("{} is already enabled.", p.name), Sev::Info);
            return;
        }

        let prompt = match id {
            "chaotic-aur" => "Enable Chaotic-AUR repository (automated pre-built AUR packages)?",
            "cachyos" => "Enable CachyOS repositories (CPU-optimized x86-64-v3/v4/zen4 packages)?",
            "blackarch" => "Enable BlackArch penetration testing repository?",
            _ => return,
        };

        if app.settings().is_true("CONFIRM_ACTIONS") {
            app.confirm(prompt, false);
            self.await_kind = Some(AwaitKind::EnableRepo(p.id.clone()));
        } else {
            match id {
                "chaotic-aur" => enable_chaotic_aur(app),
                "cachyos" => enable_cachyos(app),
                "blackarch" => enable_blackarch(app),
                _ => {}
            }
        }
    }

    /// Remove third-party repository if supported for the selected provider.
    pub fn remove_selected_repo(&mut self, app: &mut App) {
        let Some(p) = self.selected_provider() else {
            return;
        };
        let id = p.id.as_str();
        if !matches!(id, "chaotic-aur" | "cachyos" | "blackarch") {
            app.toast(
                format!("Direct repository removal not supported for '{}'", p.name),
                Sev::Warn,
            );
            return;
        }

        let is_enabled = match id {
            "chaotic-aur" => crate::sys::is_chaotic_aur_enabled(),
            "cachyos" => crate::sys::is_cachyos_repo_enabled(),
            "blackarch" => crate::sys::is_blackarch_enabled(),
            _ => false,
        };

        if !is_enabled {
            app.toast(format!("{} is not currently enabled.", p.name), Sev::Warn);
            return;
        }

        let prompt = match id {
            "chaotic-aur" => "Remove Chaotic-AUR repository, mirrorlist, and keyrings?",
            "cachyos" => "Remove CachyOS repositories, mirrorlist, and keyrings?",
            "blackarch" => "Remove BlackArch repository, mirrorlist, and keyrings?",
            _ => return,
        };

        if app.settings().is_true("CONFIRM_ACTIONS") {
            app.confirm(prompt, false);
            self.await_kind = Some(AwaitKind::RemoveRepo(p.id.clone()));
        } else {
            match id {
                "chaotic-aur" => remove_chaotic_aur(app),
                "cachyos" => remove_cachyos(app),
                "blackarch" => remove_blackarch(app),
                _ => {}
            }
        }
    }
}

impl Screen for MirrorsScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match self.mode {
            MirrorViewMode::List => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_idx = self.selected_idx.saturating_sub(1);
                    if self.selected_idx < self.list_offset {
                        self.list_offset = self.selected_idx;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.providers.is_empty() {
                        self.selected_idx = (self.selected_idx + 1).min(self.providers.len() - 1);
                    }
                }
                KeyCode::Enter | KeyCode::Char('v') => {
                    self.open_detail();
                }
                KeyCode::Char('R') => {
                    self.start_rank_selected(app);
                }
                KeyCode::Char('b') => {
                    self.start_benchmark_selected(app);
                }
                KeyCode::Char('B') => {
                    self.backup_selected(app);
                }
                KeyCode::Char('U') => {
                    self.open_restore_selector(app);
                }
                KeyCode::Char('A') => {
                    self.mode = MirrorViewMode::BulkRank;
                    self.bulk_idx = 0;
                }
                KeyCode::Char('M') => {
                    self.start_benchmark_all_detected(app);
                }
                KeyCode::Char('c') => {
                    self.custom_providers = load_custom_providers();
                    self.selected_custom_idx = 0;
                    self.mode = MirrorViewMode::Custom;
                }
                KeyCode::Char('r') => {
                    self.refresh_providers();
                    app.toast("Refreshed mirror providers and configuration", Sev::Info);
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    self.enable_selected_repo(app);
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    self.remove_selected_repo(app);
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.pop();
                }
                _ => {}
            },

            MirrorViewMode::Detail => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.detail_mirror_idx = self.detail_mirror_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let total = self
                        .selected_provider()
                        .map(|p| p.total_active_mirrors())
                        .unwrap_or(0);
                    if total > 0 {
                        self.detail_mirror_idx = (self.detail_mirror_idx + 1).min(total - 1);
                    }
                }
                KeyCode::Char('R') => {
                    self.start_rank_selected(app);
                }
                KeyCode::Char('b') => {
                    self.start_benchmark_selected(app);
                }
                KeyCode::Char('B') => {
                    self.backup_selected(app);
                }
                KeyCode::Char('U') => {
                    self.open_restore_selector(app);
                }
                KeyCode::Enter | KeyCode::Char('v') => {
                    // View raw mirrorlist contents in viewer screen
                    if let Some(p) = self.selected_provider() {
                        if let Some(target) = p.primary_mirrorlist() {
                            if let Ok(content) = std::fs::read_to_string(target) {
                                let lines: Vec<Vec<Span>> = content
                                    .lines()
                                    .map(|l| vec![Span::styled(l.to_string(), Style::new())])
                                    .collect();
                                app.push(ViewerScreen::new(
                                    format!("📋 {}", target.display()),
                                    lines,
                                ));
                            }
                        }
                    }
                }
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    self.enable_selected_repo(app);
                }
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    self.remove_selected_repo(app);
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.mode = MirrorViewMode::List;
                }
                _ => {}
            },

            MirrorViewMode::BulkRank => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.bulk_idx = self.bulk_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.providers.is_empty() {
                        self.bulk_idx = (self.bulk_idx + 1).min(self.providers.len() - 1);
                    }
                }
                KeyCode::Char(' ') => {
                    if self.bulk_idx < self.bulk_selected.len() {
                        self.bulk_selected[self.bulk_idx] = !self.bulk_selected[self.bulk_idx];
                    }
                }
                KeyCode::Char('a') => {
                    let all_set = self.bulk_selected.iter().all(|&s| s);
                    self.bulk_selected.fill(!all_set);
                }
                KeyCode::Enter => {
                    let selected_count = self.bulk_selected.iter().filter(|&&s| s).count();
                    if selected_count == 0 {
                        app.toast("No providers selected for bulk ranking", Sev::Warn);
                    } else if app.settings().is_true("CONFIRM_ACTIONS") {
                        app.confirm(
                            format!("Rank {selected_count} selected providers? Backups will be created first."),
                            false,
                        );
                        self.await_kind = Some(AwaitKind::StartBulkRank);
                    } else {
                        self.execute_bulk_rank(app);
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.mode = MirrorViewMode::List;
                }
                _ => {}
            },

            MirrorViewMode::Benchmark => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.bench_scroll = self.bench_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.bench_results.is_empty() {
                        self.bench_scroll =
                            (self.bench_scroll + 1).min(self.bench_results.len().saturating_sub(1));
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    if let Some(ref cancel) = self.bench_cancel {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    self.bench_rx = None;
                    self.mode = MirrorViewMode::List;
                }
                _ => {}
            },

            MirrorViewMode::Restore => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_backup_idx = self.selected_backup_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.backups.is_empty() {
                        self.selected_backup_idx =
                            (self.selected_backup_idx + 1).min(self.backups.len() - 1);
                    }
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    self.selected_backup_idx = 0;
                }
                KeyCode::End | KeyCode::Char('G') => {
                    if !self.backups.is_empty() {
                        self.selected_backup_idx = self.backups.len() - 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char('r') => {
                    if let Some(backup) = self.backups.get(self.selected_backup_idx).cloned() {
                        let prompt = format!(
                            "Restore backup from {} ({}) to {}?",
                            backup.timestamp,
                            backup.filename(),
                            backup.mirrorlist_path.display()
                        );
                        app.confirm(&prompt, false);
                        self.await_kind = Some(AwaitKind::RestoreBackup(
                            backup.backup_path,
                            backup.mirrorlist_path,
                        ));
                    }
                }
                KeyCode::Char('p') | KeyCode::Char('v') => {
                    // Preview backup content with syntax highlighting
                    if let Some(backup) = self.backups.get(self.selected_backup_idx) {
                        if let Ok(content) = std::fs::read_to_string(&backup.backup_path) {
                            let lines: Vec<Vec<Span<'static>>> = content
                                .lines()
                                .map(|l| {
                                    let trimmed = l.trim();
                                    let style = if trimmed.starts_with("Server =")
                                        || trimmed.starts_with("Server=")
                                    {
                                        success()
                                    } else if trimmed.starts_with('#') {
                                        dim()
                                    } else {
                                        Style::new()
                                    };
                                    vec![Span::styled(l.to_string(), style)]
                                })
                                .collect();
                            app.push(ViewerScreen::new(
                                format!("💾 Backup: {} ({})", backup.filename(), backup.timestamp),
                                lines,
                            ));
                        } else {
                            app.toast("Failed to read backup file.", Sev::Error);
                        }
                    }
                }
                KeyCode::Char('c') => {
                    // Compare / Diff selected backup against active mirrorlist
                    if let Some(backup) = self.backups.get(self.selected_backup_idx) {
                        let active_path = &backup.mirrorlist_path;
                        let bkp_path = &backup.backup_path;

                        let active_content =
                            std::fs::read_to_string(active_path).unwrap_or_default();
                        let bkp_content = std::fs::read_to_string(bkp_path).unwrap_or_default();

                        let (active_mirrors, _) =
                            crate::mirrors::parse_mirrorlist_content(&active_content);
                        let (bkp_mirrors, _) =
                            crate::mirrors::parse_mirrorlist_content(&bkp_content);

                        let active_urls: std::collections::HashSet<String> =
                            active_mirrors.iter().map(|m| m.url.clone()).collect();
                        let bkp_urls: std::collections::HashSet<String> =
                            bkp_mirrors.iter().map(|m| m.url.clone()).collect();

                        let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
                        lines.push(vec![
                            span("Backup comparison: ", accent_bold()),
                            span(backup.filename(), styled(cat_color(4))),
                            span(format!(" ({})", backup.timestamp), dim()),
                        ]);
                        lines.push(vec![
                            span("Active target:     ", accent_bold()),
                            span(active_path.display().to_string(), dim()),
                        ]);
                        lines.push(vec![span(
                            format!(
                                "Summary:           {} in backup | {} in active target",
                                bkp_mirrors.len(),
                                active_mirrors.len()
                            ),
                            hint(),
                        )]);
                        lines.push(vec![span("────────────────────────────────────────────────────────────────────────────", dim())]);
                        lines.push(vec![]);

                        let mut diff_count = 0;

                        // Mirrors only in current active file (added since backup)
                        for url in &active_urls {
                            if !bkp_urls.contains(url) {
                                lines.push(vec![
                                    span("+ [active only]  ", success()),
                                    span(format!("Server = {url}"), success()),
                                ]);
                                diff_count += 1;
                            }
                        }

                        // Mirrors only in backup (removed since backup)
                        for url in &bkp_urls {
                            if !active_urls.contains(url) {
                                lines.push(vec![
                                    span("- [backup only]  ", danger()),
                                    span(format!("Server = {url}"), danger()),
                                ]);
                                diff_count += 1;
                            }
                        }

                        // Common mirrors (unchanged)
                        for url in &active_urls {
                            if bkp_urls.contains(url) {
                                lines.push(vec![
                                    span("  [in both]      ", dim()),
                                    span(format!("Server = {url}"), dim()),
                                ]);
                            }
                        }

                        if diff_count == 0 {
                            lines.push(vec![
                                span("✔ Identical: All mirrors in this backup match the current active mirrorlist exactly.", success()),
                            ]);
                        }

                        app.push(ViewerScreen::new(
                            format!("🔍 Diff: {} vs Current", backup.filename()),
                            lines,
                        ));
                    }
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    self.backup_selected_live(app);
                }
                KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete => {
                    if let Some(backup) = self.backups.get(self.selected_backup_idx).cloned() {
                        let prompt = format!(
                            "Delete backup {} ({})?",
                            backup.filename(),
                            backup.timestamp
                        );
                        app.confirm(&prompt, false);
                        self.await_kind = Some(AwaitKind::DeleteBackup(backup.backup_path));
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.mode = MirrorViewMode::List;
                }
                _ => {}
            },

            MirrorViewMode::Custom => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected_custom_idx = self.selected_custom_idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.custom_providers.is_empty() {
                        self.selected_custom_idx =
                            (self.selected_custom_idx + 1).min(self.custom_providers.len() - 1);
                    }
                }
                KeyCode::Char('a') => {
                    // Start registration wizard
                    self.custom_add_step = Some(CustomAddStep::Id);
                    app.ask_input("Enter Custom Provider ID (e.g. 'myrepo'):");
                }
                KeyCode::Char('d') => {
                    if let Some(p) = self.custom_providers.get(self.selected_custom_idx).cloned() {
                        let prompt = format!("Remove custom provider definition '{}'?", p.id);
                        app.confirm(&prompt, false);
                        self.await_kind = Some(AwaitKind::DeleteCustomProvider(p.id.to_string()));
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.refresh_providers();
                    self.mode = MirrorViewMode::List;
                }
                _ => {}
            },
        }
    }

    fn poll(&mut self, app: &mut App) {
        if let Some(ref mut rx) = self.bench_rx {
            while let Ok(msg) = rx.try_recv() {
                self.bench_progress_msg = format!(
                    "Testing mirror {} of {} — {}",
                    msg.completed, msg.total, msg.current_mirror
                );
                if let Some(res) = msg.result {
                    self.bench_results.push(res);
                }
                if msg.finished {
                    self.bench_finished = true;
                    self.bench_progress_msg = format!(
                        "Benchmark finished! {} mirrors tested.",
                        self.bench_results.len()
                    );
                    app.toast("Mirror benchmarking complete!", Sev::Success);
                    break;
                }
            }
        }
    }

    fn busy(&self) -> Option<(String, Instant)> {
        None
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let total_active: usize = self
            .providers
            .iter()
            .map(|p| p.total_active_mirrors())
            .sum();
        let detected_count = self
            .providers
            .iter()
            .filter(|p| p.detection.is_detected())
            .count();

        let shortcuts_items = self.shortcuts_for_mode();
        let inner_width = area.width.saturating_sub(2);
        let num_cols = if inner_width >= 105 {
            3
        } else if inner_width >= 60 {
            2
        } else {
            1
        };
        let needed_rows = shortcuts_items.len().div_ceil(num_cols);
        let needed_height = (needed_rows + 2) as u16;

        let shortcuts_height = if area.height < 14 {
            1
        } else {
            let max_shortcuts = area.height.saturating_sub(10);
            needed_height.min(max_shortcuts).max(3)
        };

        let chunks = Layout::vertical([
            Constraint::Length(6),                // Header stat table
            Constraint::Fill(1),                  // Main content pane
            Constraint::Length(shortcuts_height), // Proper shortcuts pane / compact fallback
        ])
        .split(area);

        // Header table
        kv_table(
            f,
            chunks[0],
            "🌍 Provider-Aware Mirror Management",
            &[
                (
                    "Providers",
                    format!(
                        "{} total ({} detected, {} configured)",
                        self.providers.len(),
                        detected_count,
                        self.providers
                            .iter()
                            .filter(|p| !p.repos.is_empty())
                            .count()
                    ),
                ),
                (
                    "Total Mirrors",
                    format!("{total_active} active across all providers"),
                ),
                (
                    "Backends",
                    format!(
                        "Reflector: {} · rate-mirrors: {}",
                        if self.reflector_installed {
                            "installed"
                        } else {
                            "not installed"
                        },
                        if self.rate_mirrors_installed {
                            "installed"
                        } else {
                            "not installed"
                        }
                    ),
                ),
            ],
        );

        // Content pane based on mode
        match self.mode {
            MirrorViewMode::List => self.draw_provider_list(f, chunks[1]),
            MirrorViewMode::Detail => self.draw_provider_detail(f, chunks[1]),
            MirrorViewMode::BulkRank => self.draw_bulk_rank(f, chunks[1]),
            MirrorViewMode::Benchmark => self.draw_benchmark(f, chunks[1]),
            MirrorViewMode::Restore => self.draw_restore(f, chunks[1]),
            MirrorViewMode::Custom => self.draw_custom(f, chunks[1]),
        }

        // Shortcuts pane / help footer
        if shortcuts_height > 1 {
            self.draw_shortcuts_pane(f, chunks[2], &shortcuts_items);
        } else {
            let hints: Vec<&str> = match self.mode {
                MirrorViewMode::List => vec![
                    "↑↓ navigate",
                    "enter/v view",
                    "R rank",
                    "b bench",
                    "B backup",
                    "U restore",
                    "esc back",
                ],
                MirrorViewMode::Detail => vec![
                    "↑↓ scroll",
                    "R rank",
                    "b bench",
                    "B backup",
                    "U restore",
                    "v raw",
                    "esc back",
                ],
                MirrorViewMode::BulkRank => {
                    vec!["space toggle", "a all", "enter rank", "esc cancel"]
                }
                MirrorViewMode::Benchmark => vec!["esc cancel"],
                MirrorViewMode::Restore => vec!["enter restore", "p preview", "esc cancel"],
                MirrorViewMode::Custom => vec!["a add", "d delete", "esc back"],
            };
            widgets::help_bar(f, chunks[2], &hints);
        }
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["↑↓ navigate", "enter select", "esc back"]
    }

    fn on_confirm(&mut self, app: &mut App, yes: bool) {
        let Some(await_kind) = self.await_kind.take() else {
            return;
        };

        if !yes {
            return;
        }

        match await_kind {
            AwaitKind::InstallReflector => {
                self.execute_install_reflector(app);
            }
            AwaitKind::ApplyRanking => {
                if let Some(plan) = self.pending_plan.take() {
                    app.queue_ext(plan.build_apply_command());
                }
            }
            AwaitKind::RestoreBackup(backup_path, target_path) => {
                app.log(&format!(
                    "MIRRORS: Restoring {} from {}",
                    target_path.display(),
                    backup_path.display()
                ));
                let cmd = build_restore_command(&backup_path, &target_path);
                app.queue_ext(cmd);
            }
            AwaitKind::DeleteBackup(backup_path) => {
                let filename = backup_path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| backup_path.to_string_lossy().into_owned());

                app.log(&format!(
                    "MIRRORS: Deleting backup {}",
                    backup_path.display()
                ));

                match delete_backup(&backup_path) {
                    Ok(()) => {
                        // Clear from list in live time
                        self.backups.retain(|b| b.backup_path != backup_path);
                        if self.backups.is_empty() {
                            self.selected_backup_idx = 0;
                        } else if self.selected_backup_idx >= self.backups.len() {
                            self.selected_backup_idx = self.backups.len() - 1;
                        }
                        app.toast(format!("Backup '{filename}' deleted."), Sev::Success);
                        app.log(&format!("MIRRORS: Backup {filename} deleted in live time"));
                    }
                    Err(e) => {
                        app.log(&format!(
                            "MIRRORS: Direct delete failed ({e}), queuing sudo command"
                        ));
                        let cmd = build_delete_backup_command(&backup_path);
                        app.queue_ext(cmd);
                    }
                }
            }
            AwaitKind::StartBulkRank => {
                self.execute_bulk_rank(app);
            }
            AwaitKind::DeleteCustomProvider(id) => match remove_custom_provider(&id) {
                Ok(true) => {
                    app.toast(format!("Custom provider '{id}' removed."), Sev::Success);
                    self.refresh_providers();
                }
                Ok(false) => {
                    app.toast(format!("Provider '{id}' not found."), Sev::Warn);
                }
                Err(e) => {
                    app.toast(format!("Failed to remove provider: {e}"), Sev::Error);
                }
            },
            AwaitKind::EnableRepo(provider_id) => match provider_id.as_str() {
                "chaotic-aur" => enable_chaotic_aur(app),
                "cachyos" => enable_cachyos(app),
                "blackarch" => enable_blackarch(app),
                _ => {}
            },
            AwaitKind::RemoveRepo(provider_id) => match provider_id.as_str() {
                "chaotic-aur" => remove_chaotic_aur(app),
                "cachyos" => remove_cachyos(app),
                "blackarch" => remove_blackarch(app),
                _ => {}
            },
        }
    }

    fn on_input(&mut self, app: &mut App, value: String) {
        let input = value.trim().to_string();

        // If in custom provider wizard
        if let Some(step) = self.custom_add_step.take() {
            match step {
                CustomAddStep::Id => {
                    if input.is_empty() {
                        app.toast("Registration cancelled (empty ID)", Sev::Info);
                        return;
                    }
                    self.custom_add_id = input;
                    self.custom_add_step = Some(CustomAddStep::Name);
                    app.ask_input("Enter Display Name (e.g. 'My Custom Repository'):");
                }
                CustomAddStep::Name => {
                    self.custom_add_name = if input.is_empty() {
                        self.custom_add_id.clone()
                    } else {
                        input
                    };
                    self.custom_add_step = Some(CustomAddStep::MirrorlistPath);
                    app.ask_input(
                        "Enter Absolute Mirrorlist Path (e.g. /etc/pacman.d/myrepo-mirrorlist):",
                    );
                }
                CustomAddStep::MirrorlistPath => {
                    if input.is_empty() {
                        app.toast("Registration cancelled (empty path)", Sev::Info);
                        return;
                    }
                    self.custom_add_path = input;
                    self.custom_add_step = Some(CustomAddStep::Backend);
                    app.ask_input(
                        "Enter Backend (rate-mirrors / reflector / native / unsupported):",
                    );
                }
                CustomAddStep::Backend => {
                    self.custom_add_backend = if input.is_empty() {
                        "rate-mirrors".to_string()
                    } else {
                        input
                    };

                    let cp = CustomProvider {
                        id: ProviderId::new(&self.custom_add_id),
                        name: self.custom_add_name.clone(),
                        mirrorlist: PathBuf::from(&self.custom_add_path),
                        backend_type: self.custom_add_backend.clone(),
                        backend_arg: self.custom_add_id.clone(),
                        repos: vec![self.custom_add_id.clone()],
                    };

                    match add_custom_provider(cp) {
                        Ok(()) => {
                            app.toast("Custom mirror provider registered!", Sev::Success);
                            self.refresh_providers();
                        }
                        Err(e) => {
                            app.toast(format!("Registration error: {e}"), Sev::Error);
                        }
                    }
                }
            }
            return;
        }

        // Reflector country input
        let country = if input.is_empty() { None } else { Some(input) };
        if let Some(p) = self.selected_provider().cloned() {
            self.execute_ranking_for_provider(app, &p, country);
        }
    }

    fn on_ext_done(&mut self, app: &mut App, tag: &str, ok: bool) {
        match tag {
            "mirrors-dep" => {
                app.refresh_caps();
                self.reflector_installed = app.caps().reflector;
                if self.reflector_installed {
                    app.toast("reflector installed successfully.", Sev::Success);
                } else {
                    app.toast("Failed to install reflector.", Sev::Error);
                }
            }
            "mirrors-rank-generate" => {
                if ok {
                    if let Some(ref plan) = self.pending_plan {
                        match validate_candidate_file(&plan.candidate_temp_file) {
                            Ok(mirrors) => {
                                app.toast(
                                    format!(
                                        "Validation passed: {} fast mirrors verified.",
                                        mirrors.len()
                                    ),
                                    Sev::Success,
                                );

                                if app.settings().is_true("CONFIRM_ACTIONS") {
                                    let prompt = format!(
                                        "Apply {} ranked mirrors to {}?",
                                        mirrors.len(),
                                        plan.target_mirrorlist.display()
                                    );
                                    app.confirm(&prompt, false);
                                    self.await_kind = Some(AwaitKind::ApplyRanking);
                                } else {
                                    app.queue_ext(plan.build_apply_command());
                                }
                            }
                            Err(e) => {
                                app.toast(format!("Candidate validation failed: {e}"), Sev::Error);
                                app.log(&format!("MIRRORS: Candidate validation error: {e}"));
                                let _ = std::fs::remove_file(&plan.candidate_temp_file);
                                self.pending_plan = None;
                            }
                        }
                    }
                } else {
                    app.toast(
                        "Ranking command failed. Original mirrorlist untouched.",
                        Sev::Error,
                    );
                    if let Some(ref plan) = self.pending_plan {
                        let _ = std::fs::remove_file(&plan.candidate_temp_file);
                    }
                    self.pending_plan = None;
                }
            }
            "mirrors-apply-rank" => {
                if ok {
                    app.toast("Mirrorlist updated and verified!", Sev::Success);
                    app.log("MIRRORS: Ranking applied successfully");
                    let ts = current_timestamp();
                    if let Some(idx) = self.selected_provider_mut() {
                        idx.last_ranked = Some(ts);
                    }
                    self.refresh_providers();
                } else {
                    app.toast("Failed to apply ranked mirrorlist.", Sev::Error);
                }
                self.pending_plan = None;
            }
            "mirrors-backup" => {
                if ok {
                    app.toast("Mirrorlist backed up.", Sev::Success);
                    app.log("MIRRORS: Backup successful");
                    self.refresh_backups_for_selected();
                } else {
                    app.toast("Backup failed.", Sev::Error);
                }
            }
            "mirrors-restore" => {
                if ok {
                    app.toast("Mirrorlist restored from backup!", Sev::Success);
                    app.log("MIRRORS: Mirrorlist restored");
                    self.refresh_providers();
                    self.refresh_backups_for_selected();
                } else {
                    app.toast("Restore failed.", Sev::Error);
                }
            }
            "mirrors-delete-backup" => {
                if ok {
                    app.toast("Backup deleted.", Sev::Success);
                    app.log("MIRRORS: Backup deleted");
                    self.refresh_backups_for_selected();
                } else {
                    app.toast("Failed to delete backup.", Sev::Error);
                }
            }
            "mirrors-chaotic-aur" | "run-enable-chaotic-aur" => {
                app.refresh_caps();
                if ok {
                    app.toast("Chaotic-AUR repository enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable Chaotic-AUR repository.", Sev::Error);
                }
                self.refresh_providers();
            }
            "mirrors-chaotic-aur-remove" | "run-remove-chaotic-aur" => {
                app.refresh_caps();
                if ok {
                    app.toast("Chaotic-AUR repository removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove Chaotic-AUR repository.", Sev::Error);
                }
                self.refresh_providers();
            }
            "mirrors-cachyos" | "run-enable-cachyos-repo" => {
                app.refresh_caps();
                if ok {
                    app.toast("CachyOS repositories enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable CachyOS repositories.", Sev::Error);
                }
                self.refresh_providers();
            }
            "mirrors-cachyos-remove" | "run-remove-cachyos-repo" => {
                app.refresh_caps();
                if ok {
                    app.toast("CachyOS repositories removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove CachyOS repositories.", Sev::Error);
                }
                self.refresh_providers();
            }
            "mirrors-blackarch" | "run-enable-blackarch" => {
                app.refresh_caps();
                if ok {
                    app.toast("BlackArch repository enabled!", Sev::Success);
                } else {
                    app.toast("Failed to enable BlackArch repository.", Sev::Error);
                }
                self.refresh_providers();
            }
            "mirrors-blackarch-remove" | "run-remove-blackarch" => {
                app.refresh_caps();
                if ok {
                    app.toast("BlackArch repository removed!", Sev::Success);
                } else {
                    app.toast("Failed to remove BlackArch repository.", Sev::Error);
                }
                self.refresh_providers();
            }
            _ => {}
        }
    }
}

// ── Rendering Implementations ───────────────────────────────────────

impl MirrorsScreen {
    /// Render Mirror Management directly inside an embedded container (such as the HomeScreen main pane).
    pub fn draw_embedded(&mut self, f: &mut Frame<'_>, area: Rect) {
        let shortcuts_items = self.shortcuts_for_mode();
        let inner_width = area.width.saturating_sub(2);
        let num_cols = if inner_width >= 105 {
            3
        } else if inner_width >= 60 {
            2
        } else {
            1
        };
        let needed_rows = shortcuts_items.len().div_ceil(num_cols);
        let needed_height = (needed_rows + 2) as u16;

        let (content_area, shortcuts_area) = if area.height >= 14 {
            let max_shortcuts = area.height.saturating_sub(7);
            let s_height = needed_height.min(max_shortcuts).max(3);
            let chunks =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(s_height)]).split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        match self.mode {
            MirrorViewMode::List => self.draw_provider_list(f, content_area),
            MirrorViewMode::Detail => self.draw_provider_detail(f, content_area),
            MirrorViewMode::BulkRank => self.draw_bulk_rank(f, content_area),
            MirrorViewMode::Benchmark => self.draw_benchmark(f, content_area),
            MirrorViewMode::Restore => self.draw_restore(f, content_area),
            MirrorViewMode::Custom => self.draw_custom(f, content_area),
        }

        if let Some(s_area) = shortcuts_area {
            self.draw_shortcuts_pane(f, s_area, &shortcuts_items);
        }
    }

    /// Render contextual shortcuts into a dedicated area (such as the HomeScreen command list footer).
    #[allow(dead_code)]
    pub fn draw_shortcuts(&self, f: &mut Frame<'_>, area: Rect) {
        let items = self.shortcuts_for_mode();
        self.draw_shortcuts_pane(f, area, &items);
    }

    /// Check if currently viewing a submode (Detail, BulkRank, Benchmark, Restore, Custom).
    pub fn is_in_submode(&self) -> bool {
        self.mode != MirrorViewMode::List
    }

    /// Return from any submode back to the main provider overview.
    pub fn exit_submode(&mut self) {
        self.mode = MirrorViewMode::List;
    }

    fn draw_provider_list(&mut self, f: &mut Frame<'_>, area: Rect) {
        let total_active: usize = self
            .providers
            .iter()
            .map(|p| p.total_active_mirrors())
            .sum();
        let status_desc = format!(
            " {} active mirrors · reflector: {} · rate-mirrors: {} ",
            total_active,
            if self.reflector_installed {
                "installed"
            } else {
                "missing"
            },
            if self.rate_mirrors_installed {
                "installed"
            } else {
                "missing"
            }
        );
        let block = panel_color("Repository Providers & Mirrorlists", cat_color(3)).title_bottom(
            Line::from(span(status_desc, dim())).alignment(ratatui::layout::Alignment::Right),
        );
        let inner = block.inner(area);
        f.render_widget(block, area);

        let visible_height = inner.height.saturating_sub(2) as usize;
        if self.selected_idx >= self.list_offset + visible_height && visible_height > 0 {
            self.list_offset = self.selected_idx + 1 - visible_height;
        }

        let mut lines = Vec::new();

        // Table column headers
        lines.push(Line::from(vec![
            span("  ", Style::new()),
            span(format!("{:<18}", "Provider"), accent_bold()),
            span(format!("{:<20}", "Status"), accent_bold()),
            span(format!("{:<9}", "Targets"), accent_bold()),
            span(format!("{:<10}", "Mirrors"), accent_bold()),
            span(format!("{:<14}", "Backend"), accent_bold()),
            span(format!("{:<15}", "Last Ranked"), accent_bold()),
        ]));
        lines.push(Line::from(span(
            "  ─────────────────────────────────────────────────────────────────────────────",
            dim(),
        )));

        for (i, p) in self
            .providers
            .iter()
            .enumerate()
            .skip(self.list_offset)
            .take(visible_height)
        {
            let is_selected = i == self.selected_idx;
            let cursor = if is_selected { "▸ " } else { "  " };

            let (status_label, status_style) = match &p.detection {
                DetectionStatus::Detected => ("✔ Detected", success()),
                DetectionStatus::NotConfigured => ("· Not Configured", dim()),
                DetectionStatus::PartiallyConfigured(_) => ("⚠ Partially Config", warning()),
                DetectionStatus::MissingMirrorlist(_) => ("✘ Missing Target", danger()),
                DetectionStatus::BackendUnavailable(_) => ("⚠ Backend Missing", warning()),
                DetectionStatus::Unsupported(_) => ("· Unsupported", dim()),
            };

            let active_count = p.total_active_mirrors();
            let targets_count = p.total_targets();
            let backend_name = p.backend.name();
            let last_ranked = p.last_ranked.as_deref().unwrap_or("Never");

            let row_style = if is_selected {
                selected_style(cat_color(3))
            } else {
                Style::new()
            };

            lines.push(Line::from(vec![
                span(cursor.to_string(), accent_bold()),
                span(format!("{:<18}", truncate_str(&p.name, 17)), row_style),
                span(format!("{:<20}", status_label), status_style),
                span(
                    format!("{:<9}", format!("{targets_count} file(s)")),
                    row_style,
                ),
                span(
                    format!("{:<10}", format!("{active_count} active")),
                    row_style,
                ),
                span(format!("{:<14}", backend_name), row_style),
                span(format!("{:<15}", last_ranked), dim()),
            ]));
        }

        f.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_provider_detail(&mut self, f: &mut Frame<'_>, area: Rect) {
        let Some(p) = self.selected_provider() else {
            return;
        };

        let block = panel_color(&format!("Provider Detail: {}", p.name), cat_color(3));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let cols = Layout::horizontal([
            Constraint::Percentage(45), // Left: Configuration and Target info
            Constraint::Percentage(55), // Right: Mirrorlist URLs & Backups
        ])
        .split(inner);

        // Left pane: KV summary
        let repos_str = if p.repos.is_empty() {
            "(none configured)".to_string()
        } else {
            p.repos.join(", ")
        };

        let targets_str = if p.mirrorlists.is_empty() {
            "(no mirrorlist target)".to_string()
        } else {
            p.mirrorlists
                .iter()
                .map(|t| t.filename())
                .collect::<Vec<_>>()
                .join(", ")
        };

        let backend_avail = match &p.detection {
            DetectionStatus::BackendUnavailable(reason) => format!("Unavailable: {reason}"),
            _ => "Available".to_string(),
        };

        let mut rows = vec![
            ("Provider ID", p.id.to_string()),
            ("Display Name", p.name.clone()),
            ("Distribution", p.dist_type.clone()),
            ("Status", p.detection.label().to_string()),
            ("Repositories", repos_str),
            ("Mirrorlists", targets_str),
            ("Ranking Tool", p.backend.name().to_string()),
            ("Backend Status", backend_avail),
            ("Active Mirrors", p.total_active_mirrors().to_string()),
            ("Backups Stored", self.backups.len().to_string()),
        ];
        if matches!(p.id.as_str(), "chaotic-aur" | "cachyos" | "blackarch") {
            let is_enabled = match p.id.as_str() {
                "chaotic-aur" => crate::sys::is_chaotic_aur_enabled(),
                "cachyos" => crate::sys::is_cachyos_repo_enabled(),
                "blackarch" => crate::sys::is_blackarch_enabled(),
                _ => false,
            };
            rows.push((
                "Repo Action",
                if is_enabled {
                    "[X] Remove repository".to_string()
                } else {
                    "[E] Enable repository".to_string()
                },
            ));
        }
        kv_table(f, cols[0], "Configuration", &rows);

        // Right pane: Active mirrors
        let mut right_lines = Vec::new();
        right_lines.push(Line::from(vec![
            span("Active Mirrors inside Target: ", accent_bold()),
            span(format!("({} total)", p.total_active_mirrors()), dim()),
        ]));
        right_lines.push(Line::from(span(
            "──────────────────────────────────────────────────",
            dim(),
        )));

        let mut idx = 1;
        for target in &p.mirrorlists {
            right_lines.push(Line::from(vec![
                span("File: ", accent_bold()),
                span(target.path.display().to_string(), hint()),
            ]));

            for mirror in &target.active_mirrors {
                let cursor = if idx - 1 == self.detail_mirror_idx {
                    "▸ "
                } else {
                    "  "
                };
                let style = if idx - 1 == self.detail_mirror_idx {
                    selected_style(cat_color(3))
                } else {
                    Style::new()
                };
                right_lines.push(Line::from(vec![
                    span(cursor.to_string(), accent_bold()),
                    span(format!("{:>2}. ", idx), dim()),
                    span(mirror.url.clone(), style),
                ]));
                idx += 1;
            }
        }

        if p.total_active_mirrors() == 0 {
            right_lines.push(Line::from(span(
                "  (No active mirrors found in mirrorlist file)",
                dim(),
            )));
        }

        f.render_widget(
            Paragraph::new(right_lines).block(panel("Mirror Targets")),
            cols[1],
        );
    }

    fn draw_bulk_rank(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = panel_color("Bulk Ranking Workflow", cat_color(1));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).split(inner);

        let mut lines = Vec::new();
        lines.push(Line::from(span(
            "Select providers to rank in one automated operation:",
            hint(),
        )));
        lines.push(Line::from(""));

        for (i, p) in self.providers.iter().enumerate() {
            let is_focused = i == self.bulk_idx;
            let is_checked = self.bulk_selected.get(i).copied().unwrap_or(false);
            let check_box = if is_checked { "[✔]" } else { "[ ]" };
            let cursor = if is_focused { "▸ " } else { "  " };

            let status_note = if !p.is_supported() {
                "(unsupported backend)"
            } else if !p.detection.is_detected() {
                "(not detected)"
            } else {
                ""
            };

            let row_style = if is_focused {
                selected_style(cat_color(1))
            } else {
                Style::new()
            };

            lines.push(Line::from(vec![
                span(cursor.to_string(), accent_bold()),
                span(
                    format!("{check_box} "),
                    if is_checked { success() } else { dim() },
                ),
                span(format!("{:<20}", p.name), row_style),
                span(format!("{:<15}", p.backend.name()), dim()),
                span(format!("{:>2} target(s) ", p.total_targets()), dim()),
                span(status_note.to_string(), warning()),
            ]));
        }

        f.render_widget(Paragraph::new(lines), chunks[0]);

        // Summary footer
        let selected_count = self.bulk_selected.iter().filter(|&&s| s).count();
        let summary =
            vec![
            Line::from(vec![
                span("Summary: ", accent_bold()),
                span(format!("{selected_count} providers selected | "), success()),
                span("Backups created automatically before each modification", dim()),
            ]),
            Line::from(span(
                "Press [Enter] to start bulk ranking, [Space] to toggle, [a] select/deselect all",
                hint(),
            )),
        ];
        f.render_widget(Paragraph::new(summary).block(panel("Ready")), chunks[1]);
    }

    fn draw_benchmark(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = panel_color("Real-Time Mirror Benchmark", cat_color(2));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::vertical([
            Constraint::Length(3), // Progress indicator
            Constraint::Fill(1),   // Benchmark results table
        ])
        .split(inner);

        // Progress bar / text
        let status_line = Line::from(vec![
            span("Progress: ", accent_bold()),
            span(
                self.bench_progress_msg.clone(),
                if self.bench_finished {
                    success()
                } else {
                    accent()
                },
            ),
        ]);
        f.render_widget(
            Paragraph::new(vec![status_line]).block(panel("Status")),
            chunks[0],
        );

        // Results table
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            span(format!("{:<4}", "#"), accent_bold()),
            span(format!("{:<45}", "Mirror URL"), accent_bold()),
            span(format!("{:<10}", "HTTP Code"), accent_bold()),
            span(format!("{:<12}", "Latency"), accent_bold()),
            span(format!("{:<15}", "Throughput"), accent_bold()),
            span(format!("{:<12}", "Status"), accent_bold()),
        ]));
        lines.push(Line::from(span(
            "─────────────────────────────────────────────────────────────────────────────────────────────",
            dim(),
        )));

        for (i, res) in self.bench_results.iter().enumerate() {
            let http_str = res
                .http_status
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string());
            let lat_str = res
                .latency_ms
                .map(|l| format!("{l} ms"))
                .unwrap_or_else(|| "-".to_string());
            let speed_str = res
                .throughput_kbps
                .map(|s| format!("{s:.1} KB/s"))
                .unwrap_or_else(|| "-".to_string());
            let (stat_str, stat_style) = if res.reachable {
                ("OK", success())
            } else {
                ("FAILED", danger())
            };

            lines.push(Line::from(vec![
                span(format!("{:<4}", i + 1), dim()),
                span(format!("{:<45}", truncate_str(&res.url, 44)), Style::new()),
                span(format!("{:<10}", http_str), Style::new()),
                span(format!("{:<12}", lat_str), accent()),
                span(format!("{:<15}", speed_str), success()),
                span(format!("{:<12}", stat_str), stat_style),
            ]));
        }

        if self.bench_results.is_empty() {
            lines.push(Line::from(span("  Benchmarking mirrors...", dim())));
        }

        f.render_widget(
            Paragraph::new(lines)
                .block(panel("Benchmark Results"))
                .scroll((self.bench_scroll as u16, 0)),
            chunks[1],
        );
    }

    fn draw_restore(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = panel_color("Mirrorlist Backup History & Restoration", cat_color(4));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines = Vec::new();
        lines.push(Line::from(span(
            "Select a backup to view [v/p], diff [c], restore [enter], delete [d/x], or create new [b]:",
            hint(),
        )));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            span("  ", Style::new()),
            span(format!("{:<32}", "Backup File"), accent_bold()),
            span(format!("{:<22}", "Created / Modified"), accent_bold()),
            span(format!("{:<15}", "Mirrors"), accent_bold()),
            span(format!("{:>10}", "Size"), accent_bold()),
        ]));
        lines.push(Line::from(span(
            "  ────────────────────────────────────────────────────────────────────────────",
            dim(),
        )));

        for (i, b) in self.backups.iter().enumerate() {
            let is_sel = i == self.selected_backup_idx;
            let cursor = if is_sel { "▸ " } else { "  " };
            let style = if is_sel {
                selected_style(cat_color(4))
            } else {
                Style::new()
            };

            lines.push(Line::from(vec![
                span(cursor.to_string(), accent_bold()),
                span(format!("{:<32}", truncate_str(&b.filename(), 31)), style),
                span(format!("{:<22}", b.timestamp), dim()),
                span(
                    format!("{:<15}", format!("{} mirrors", b.mirror_count)),
                    success(),
                ),
                span(format!("{:>10}", format!("{} B", b.size_bytes)), dim()),
            ]));
        }

        if self.backups.is_empty() {
            lines.push(Line::from(span(
                "  No backups found for this mirrorlist. Press [b] to create one now.",
                dim(),
            )));
        }

        f.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_custom(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = panel_color("Custom Mirror Providers", cat_color(5));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut lines = Vec::new();
        lines.push(Line::from(span(
            "User-registered mirror providers stored in ~/.config/archman/custom_mirrors.conf:",
            hint(),
        )));
        lines.push(Line::from(""));

        for (i, p) in self.custom_providers.iter().enumerate() {
            let is_sel = i == self.selected_custom_idx;
            let cursor = if is_sel { "▸ " } else { "  " };
            let style = if is_sel {
                selected_style(cat_color(5))
            } else {
                Style::new()
            };

            lines.push(Line::from(vec![
                span(cursor.to_string(), accent_bold()),
                span(format!("{:<16}", p.id.as_str()), style),
                span(format!("{:<20}", p.name), style),
                span(format!("{:<30}", p.mirrorlist.display()), dim()),
                span(format!("Backend: {}", p.backend_type), accent()),
            ]));
        }

        if self.custom_providers.is_empty() {
            lines.push(Line::from(span(
                "  No custom providers registered. Press [a] to add a new provider.",
                dim(),
            )));
        }

        f.render_widget(Paragraph::new(lines), inner);
    }

    /// Retrieve contextual keybindings for the current view mode.
    fn shortcuts_for_mode(&self) -> Vec<(&'static str, String)> {
        match self.mode {
            MirrorViewMode::List => {
                let (e_desc, x_desc) = if let Some(p) = self.selected_provider() {
                    (
                        format!("Enable {} repo", p.name),
                        format!("Remove {} repo", p.name),
                    )
                } else {
                    ("Enable repo".to_string(), "Remove repo".to_string())
                };

                vec![
                    ("[↑/k] [↓/j]", "Navigate providers".to_string()),
                    ("[enter/v]", "Provider detail & mirrors".to_string()),
                    ("[R]", "Rank selected provider".to_string()),
                    ("[b]", "Benchmark selected provider".to_string()),
                    ("[M]", "Benchmark all detected".to_string()),
                    ("[B]", "Backup mirrorlist archive".to_string()),
                    ("[U]", "Restore backup history".to_string()),
                    ("[A]", "Bulk rank multiple providers".to_string()),
                    ("[c]", "Custom provider manager".to_string()),
                    ("[r]", "Refresh configuration".to_string()),
                    ("[E]", e_desc),
                    ("[X]", x_desc),
                    ("[esc] [q]", "Back to main menu".to_string()),
                ]
            }
            MirrorViewMode::Detail => {
                let (e_desc, x_desc) = if let Some(p) = self.selected_provider() {
                    (
                        format!("Enable {} repo", p.name),
                        format!("Remove {} repo", p.name),
                    )
                } else {
                    ("Enable repo".to_string(), "Remove repo".to_string())
                };

                vec![
                    ("[↑/k] [↓/j]", "Scroll active mirrors".to_string()),
                    ("[enter/v]", "View raw mirrorlist file".to_string()),
                    ("[R]", "Rank this provider".to_string()),
                    ("[b]", "Benchmark active mirrors".to_string()),
                    ("[B]", "Create timestamped backup".to_string()),
                    ("[U]", "Restore backup history".to_string()),
                    ("[E]", e_desc),
                    ("[X]", x_desc),
                    ("[esc] [q]", "Return to provider list".to_string()),
                ]
            }
            MirrorViewMode::BulkRank => vec![
                ("[↑/k] [↓/j]", "Select provider".to_string()),
                ("[space]", "Toggle provider selection".to_string()),
                ("[a]", "Select / Deselect all providers".to_string()),
                ("[enter]", "Confirm and start bulk ranking".to_string()),
                ("[esc] [q]", "Cancel & return to list".to_string()),
            ],
            MirrorViewMode::Benchmark => vec![
                ("[↑/k] [↓/j]", "Scroll benchmark results".to_string()),
                ("[esc] [q]", "Stop benchmark job & return".to_string()),
                (
                    "Live Test",
                    "Measuring latency & download throughput".to_string(),
                ),
            ],
            MirrorViewMode::Restore => vec![
                ("[↑/k] [↓/j]", "Navigate backups".to_string()),
                ("[enter/r]", "Restore backup (atomic)".to_string()),
                ("[v/p]", "View backup content".to_string()),
                ("[c]", "Diff with active mirrorlist".to_string()),
                ("[b]", "Create new backup now".to_string()),
                ("[d/x]", "Delete selected backup".to_string()),
                ("[esc] [q]", "Return to provider list".to_string()),
            ],
            MirrorViewMode::Custom => vec![
                ("[↑/k] [↓/j]", "Select custom provider".to_string()),
                ("[a]", "Add custom provider (wizard)".to_string()),
                ("[d]", "Delete selected custom provider".to_string()),
                ("[esc] [q]", "Return to provider list".to_string()),
            ],
        }
    }

    /// Render boxed multi-column shortcuts pane at the bottom of the screen.
    fn draw_shortcuts_pane(&self, f: &mut Frame<'_>, area: Rect, items: &[(&'static str, String)]) {
        let block = panel_color("Shortcuts", cat_color(3));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let num_cols = if inner.width >= 105 {
            3
        } else if inner.width >= 60 {
            2
        } else {
            1
        };

        let col_constraints = match num_cols {
            3 => vec![
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ],
            2 => vec![Constraint::Percentage(50), Constraint::Percentage(50)],
            _ => vec![Constraint::Percentage(100)],
        };

        let cols = Layout::horizontal(col_constraints).split(inner);
        let items_per_col = items.len().div_ceil(num_cols);

        for (col_idx, chunk) in cols.iter().enumerate() {
            let start = col_idx * items_per_col;
            let end = (start + items_per_col).min(items.len());
            if start >= items.len() {
                continue;
            }

            let lines: Vec<Line> = items[start..end]
                .iter()
                .map(|(k, d)| {
                    Line::from(vec![
                        span(format!("{k:<13}"), styled(cat_color(1))),
                        span(format!(" {d}"), dim()),
                    ])
                })
                .collect();

            f.render_widget(Paragraph::new(lines), *chunk);
        }
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max.saturating_sub(1)])
    } else {
        s.to_string()
    }
}

// ── Repository Management Helpers (Preserved for repos.rs) ───────────

/// Enable Chaotic-AUR repository by importing keys, installing keyring/mirrorlist,
/// configuring /etc/pacman.conf, and refreshing databases.
pub fn enable_chaotic_aur(app: &mut App) {
    app.log("REPO: Enabling Chaotic-AUR repository");
    let script = r#"set -e
echo "==> [1/4] Receiving and signing Chaotic-AUR GPG keys..."
pacman-key --recv-key FBA220DFC880C036 --keyserver keyserver.ubuntu.com || pacman-key --recv-key FBA220DFC880C036 --keyserver hkp://keyserver.ubuntu.com:80
pacman-key --lsign-key FBA220DFC880C036
pacman-key --recv-key 3056513887B78AEB --keyserver keyserver.ubuntu.com 2>/dev/null && pacman-key --lsign-key 3056513887B78AEB 2>/dev/null || true

echo "==> [2/4] Installing chaotic-keyring and chaotic-mirrorlist..."
pacman -U --noconfirm 'https://cdn-mirror.chaotic.cx/chaotic-aur/chaotic-keyring.pkg.tar.zst' 'https://cdn-mirror.chaotic.cx/chaotic-aur/chaotic-mirrorlist.pkg.tar.zst'

echo "==> [3/4] Configuring /etc/pacman.conf..."
if grep -q "^\s*\[chaotic-aur\]" /etc/pacman.conf; then
    echo "    [chaotic-aur] already configured in /etc/pacman.conf"
else
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    printf '\n[chaotic-aur]\nInclude = /etc/pacman.d/chaotic-mirrorlist\n' >> /etc/pacman.conf
    echo "    Added [chaotic-aur] repository to /etc/pacman.conf"
fi

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> Chaotic-AUR repository enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-chaotic-aur",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Enable Chaotic-AUR repository")
        .result(
            "Chaotic-AUR repository enabled!",
            "Failed to enable Chaotic-AUR repository.",
            "REPO: Chaotic-AUR repository enabled",
        ),
    );
}

/// Enable CachyOS repositories by downloading official installer script,
/// auto-detecting CPU microarchitecture, and configuring optimized repositories.
pub fn enable_cachyos(app: &mut App) {
    app.log("REPO: Enabling CachyOS repositories");
    let script = r#"set -e
echo "==> [1/4] Preparing temporary workspace..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/4] Downloading official CachyOS repository installer..."
curl -sSL https://mirror.cachyos.org/cachyos-repo.tar.xz -o "$tmp/cachyos-repo.tar.xz"
tar -xf "$tmp/cachyos-repo.tar.xz" -C "$tmp"

echo "==> [3/4] Running CachyOS repository setup and CPU instruction set detection..."
(cd "$tmp/cachyos-repo" && ./cachyos-repo.sh --install)

echo "==> [4/4] Refreshing package databases with CachyOS repositories..."
pacman -Sy
echo "==> CachyOS repositories enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new("mirrors-cachyos", "sudo", &args(&["bash", "-c", script]))
            .note("Enable CachyOS repositories")
            .result(
                "CachyOS repositories enabled!",
                "Failed to enable CachyOS repositories.",
                "REPO: CachyOS repositories enabled",
            ),
    );
}

/// Enable BlackArch penetration testing repository via official strap.sh.
pub fn enable_blackarch(app: &mut App) {
    app.log("REPO: Enabling BlackArch repository");
    let script = r#"set -e
echo "==> [1/3] Preparing workspace for BlackArch setup..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/3] Downloading official BlackArch bootstrap script (strap.sh)..."
curl -sSL https://blackarch.org/strap.sh -o "$tmp/strap.sh"
chmod +x "$tmp/strap.sh"

echo "==> [3/3] Running BlackArch repository setup..."
(cd "$tmp" && ./strap.sh)

echo "==> Refreshing package databases..."
pacman -Sy
echo "==> BlackArch repository enabled successfully!"
"#;
    app.queue_ext(
        ExtCmd::new("mirrors-blackarch", "sudo", &args(&["bash", "-c", script]))
            .note("Enable BlackArch repository")
            .result(
                "BlackArch repository enabled!",
                "Failed to enable BlackArch repository.",
                "REPO: BlackArch repository enabled",
            ),
    );
}

/// Remove Chaotic-AUR repository, mirrorlist, keyrings, and pacman.conf configuration.
pub fn remove_chaotic_aur(app: &mut App) {
    app.log("REPO: Removing Chaotic-AUR repository");
    let script = r#"set -e
echo "==> [1/4] Removing [chaotic-aur] configuration from /etc/pacman.conf..."
if grep -q "^\s*\[chaotic-aur\]" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[chaotic-aur\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/chaotic-mirrorlist/d' /etc/pacman.conf
    sed -i '/^\s*\[chaotic-aur\]/d' /etc/pacman.conf
    echo "    Removed [chaotic-aur] from /etc/pacman.conf"
fi

echo "==> [2/4] Removing chaotic-keyring and chaotic-mirrorlist..."
pacman -Rdd --noconfirm chaotic-keyring chaotic-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/chaotic-mirrorlist
rm -f /var/lib/pacman/sync/chaotic-aur.*

echo "==> [3/4] Removing Chaotic-AUR GPG keys from pacman keyring..."
pacman-key --delete FBA220DFC880C036 3056513887B78AEB 2>/dev/null || true

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> Chaotic-AUR repository removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-chaotic-aur-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove Chaotic-AUR repository")
        .result(
            "Chaotic-AUR repository removed!",
            "Failed to remove Chaotic-AUR repository.",
            "REPO: Chaotic-AUR repository removed",
        ),
    );
}

/// Remove CachyOS repositories, mirrorlists, keyrings, and pacman.conf configuration.
pub fn remove_cachyos(app: &mut App) {
    app.log("REPO: Removing CachyOS repositories");
    let script = r#"set -e
echo "==> [1/4] Preparing workspace..."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "==> [2/4] Downloading official CachyOS repository uninstaller..."
if curl -sSL https://mirror.cachyos.org/cachyos-repo.tar.xz -o "$tmp/cachyos-repo.tar.xz"; then
    tar -xf "$tmp/cachyos-repo.tar.xz" -C "$tmp"
    echo "==> Running CachyOS uninstaller..."
    (cd "$tmp/cachyos-repo" && ./cachyos-repo.sh --remove) || true
fi

echo "==> [3/4] Cleaning CachyOS configurations and packages..."
if grep -q "cachyos" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[cachyos.*\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/cachyos.*mirrorlist/d' /etc/pacman.conf
    sed -i '/cachyos/d' /etc/pacman.conf
fi
pacman -Rdd --noconfirm cachyos-keyring cachyos-mirrorlist cachyos-v3-mirrorlist cachyos-v4-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/cachyos-mirrorlist* /etc/pacman.d/cachyos-v*-mirrorlist*
rm -f /var/lib/pacman/sync/cachyos*

echo "==> [4/4] Refreshing package databases..."
pacman -Sy
echo "==> CachyOS repositories removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-cachyos-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove CachyOS repositories")
        .result(
            "CachyOS repositories removed!",
            "Failed to remove CachyOS repositories.",
            "REPO: CachyOS repositories removed",
        ),
    );
}

/// Remove BlackArch penetration testing repository and pacman.conf configuration.
pub fn remove_blackarch(app: &mut App) {
    app.log("REPO: Removing BlackArch repository");
    let script = r#"set -e
echo "==> [1/3] Removing BlackArch entries from /etc/pacman.conf..."
if grep -q "^\s*\[blackarch\]" /etc/pacman.conf; then
    cp -a /etc/pacman.conf /etc/pacman.conf.bak
    sed -i '/^\s*\[blackarch\]/,/^\s*Include\s*=\s*\/etc\/pacman\.d\/blackarch-mirrorlist/d' /etc/pacman.conf
    sed -i '/^\s*\[blackarch\]/d' /etc/pacman.conf
    echo "    Removed [blackarch] from /etc/pacman.conf"
fi

echo "==> [2/3] Removing BlackArch keyring and mirrorlist..."
pacman -Rdd --noconfirm blackarch-keyring blackarch-mirrorlist 2>/dev/null || true
rm -f /etc/pacman.d/blackarch-mirrorlist
rm -f /var/lib/pacman/sync/blackarch.*

echo "==> [3/3] Refreshing package databases..."
pacman -Sy
echo "==> BlackArch repository removed successfully!"
"#;
    app.queue_ext(
        ExtCmd::new(
            "mirrors-blackarch-remove",
            "sudo",
            &args(&["bash", "-c", script]),
        )
        .note("Remove BlackArch repository")
        .result(
            "BlackArch repository removed!",
            "Failed to remove BlackArch repository.",
            "REPO: BlackArch repository removed",
        ),
    );
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn text_of(term: &ratatui::Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol().to_string())
            .collect()
    }

    #[test]
    fn test_mirrors_screen_creation() {
        let app = App::new();
        let screen = MirrorsScreen::new(&app);
        assert!(!screen.providers.is_empty());
        assert_eq!(screen.mode, MirrorViewMode::List);
    }

    #[test]
    fn test_mirrors_screen_navigation() {
        let app = App::new();
        let mut screen = MirrorsScreen::new(&app);
        assert_eq!(screen.selected_idx, 0);

        let down_key = KeyEvent::from(KeyCode::Down);
        screen.handle_key(&mut App::new(), down_key);
        if screen.providers.len() > 1 {
            assert_eq!(screen.selected_idx, 1);
        }

        let up_key = KeyEvent::from(KeyCode::Up);
        screen.handle_key(&mut App::new(), up_key);
        assert_eq!(screen.selected_idx, 0);
    }

    #[test]
    fn test_mirrors_screen_mode_transitions() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);

        // Open detail
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Enter));
        assert_eq!(screen.mode, MirrorViewMode::Detail);

        // Return to list
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Esc));
        assert_eq!(screen.mode, MirrorViewMode::List);

        // Open bulk rank
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('A')));
        assert_eq!(screen.mode, MirrorViewMode::BulkRank);

        // Toggle bulk selection
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char(' ')));
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Esc));
        assert_eq!(screen.mode, MirrorViewMode::List);

        // Open custom providers
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('c')));
        assert_eq!(screen.mode, MirrorViewMode::Custom);
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Esc));
        assert_eq!(screen.mode, MirrorViewMode::List);
    }

    #[test]
    fn test_mirrors_screen_provider_list_rendering() {
        let app = App::new();
        let mut screen = MirrorsScreen::new(&app);
        let mut term = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();

        term.draw(|f| screen.draw(f, f.area())).unwrap();
        let text = text_of(&term);

        assert!(text.contains("Provider-Aware Mirror Management"));
        assert!(text.contains("Provider"));
        assert!(text.contains("Status"));
        assert!(text.contains("Arch Linux"));
        assert!(text.contains("Shortcuts"));
        assert!(text.contains("[R]"));
        assert!(text.contains("[b]"));
        assert!(text.contains("[M]"));
        assert!(text.contains("[B]"));
        assert!(text.contains("[U]"));
        assert!(text.contains("[A]"));
        assert!(text.contains("[c]"));
        assert!(text.contains("[r]"));
        assert!(text.contains("[E]"));
        assert!(text.contains("[X]"));
        assert!(text.contains("Enable Arch Linux repo"));
        assert!(text.contains("Remove Arch Linux repo"));
        assert!(!text.contains("3rd-party"));
    }

    #[test]
    fn test_mirrors_screen_detail_rendering() {
        let app = App::new();
        let mut screen = MirrorsScreen::new(&app);
        let mut term = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();

        screen.open_detail();
        term.draw(|f| screen.draw(f, f.area())).unwrap();
        let text = text_of(&term);

        assert!(text.contains("Provider Detail:"));
        assert!(text.contains("Configuration"));
        assert!(text.contains("Mirror Targets"));
        assert!(text.contains("Shortcuts"));
        assert!(text.contains("[enter/v]"));
        assert!(text.contains("[E]"));
        assert!(text.contains("[X]"));
        assert!(text.contains("Enable Arch Linux repo"));
        assert!(text.contains("Remove Arch Linux repo"));
        assert!(!text.contains("3rd-party"));
    }

    #[test]
    fn test_mirrors_screen_bulk_rendering_and_selection() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);
        let mut term = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();

        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('A')));
        assert_eq!(screen.mode, MirrorViewMode::BulkRank);

        term.draw(|f| screen.draw(f, f.area())).unwrap();
        let text = text_of(&term);
        assert!(text.contains("Bulk Ranking Workflow"));
        assert!(text.contains("Summary:"));

        // Toggle all selection
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('a')));
        assert!(screen.bulk_selected.iter().all(|&s| !s));
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('a')));
        assert!(screen.bulk_selected.iter().all(|&s| s));
    }

    #[test]
    fn test_mirrors_screen_progress_reporting_and_cancellation() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);

        let (tx, rx) = std::sync::mpsc::channel();
        screen.bench_rx = Some(rx);
        screen.bench_cancel = Some(Arc::new(AtomicBool::new(false)));
        screen.mode = MirrorViewMode::Benchmark;

        // Send progress message
        tx.send(BenchmarkProgress {
            total: 10,
            completed: 3,
            current_mirror: "https://test.mirror.org".to_string(),
            result: Some(BenchmarkResult {
                url: "https://test.mirror.org".to_string(),
                reachable: true,
                http_status: Some(200),
                latency_ms: Some(42),
                throughput_kbps: Some(1500.0),
                error: None,
            }),
            finished: false,
        })
        .unwrap();

        screen.poll(&mut app);
        assert_eq!(screen.bench_results.len(), 1);
        assert!(screen.bench_progress_msg.contains("3 of 10"));

        // Cancellation with Esc
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Esc));
        assert_eq!(screen.mode, MirrorViewMode::List);
        assert!(screen.bench_cancel.as_ref().unwrap().load(Ordering::SeqCst));
    }

    #[test]
    fn test_mirrors_screen_error_handling() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);

        // When non-existent candidate file is signaled done
        let plan = RankingPlan {
            provider_id: ProviderId::new("arch"),
            provider_name: "Arch Linux".to_string(),
            target_mirrorlist: PathBuf::from("/etc/pacman.d/mirrorlist"),
            candidate_temp_file: PathBuf::from("/nonexistent/candidate.tmp"),
            backup_path: PathBuf::from("/etc/pacman.d/mirrorlist.bak"),
            generate_cmd: ExtCmd::new("test", "echo", &[]),
        };
        screen.pending_plan = Some(plan);

        screen.on_ext_done(&mut app, "mirrors-rank-generate", true);
        assert!(screen.pending_plan.is_none()); // Cleaned up upon validation failure!
    }

    #[test]
    fn test_mirrors_screen_repo_action_routing() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);

        // Find blackarch or chaotic-aur index
        if let Some(idx) = screen
            .providers
            .iter()
            .position(|p| p.id.as_str() == "blackarch")
        {
            screen.selected_idx = idx;
            let is_enabled = crate::sys::is_blackarch_enabled();
            if is_enabled {
                screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('x')));
                assert!(matches!(screen.await_kind, Some(AwaitKind::RemoveRepo(_))));
            } else {
                screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('e')));
                assert!(matches!(screen.await_kind, Some(AwaitKind::EnableRepo(_))));
            }

            // Confirm
            screen.on_confirm(&mut app, true);
            assert!(screen.await_kind.is_none());
        }

        // Test unsupported direct repo setup for arch
        if let Some(idx) = screen
            .providers
            .iter()
            .position(|p| p.id.as_str() == "arch")
        {
            screen.selected_idx = idx;
            screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('e')));
            assert!(screen.await_kind.is_none());
        }
    }

    #[test]
    fn test_mirrors_screen_backup_actions() {
        let mut app = App::new();
        let mut screen = MirrorsScreen::new(&app);

        let temp_dir = std::env::temp_dir().join("archman_test_bkp_actions");
        let _ = std::fs::create_dir_all(&temp_dir);
        let target = temp_dir.join("mirrorlist");
        let bkp = temp_dir.join("mirrorlist.bak.20261031_120000");

        let _ = std::fs::write(&target, "Server = https://active.example.com\n");
        let _ = std::fs::write(&bkp, "Server = https://backup.example.com\n");

        screen.backups = vec![BackupEntry {
            mirrorlist_path: target.clone(),
            backup_path: bkp.clone(),
            timestamp: "2026-10-31 12:00:00".to_string(),
            mirror_count: 1,
            size_bytes: 35,
        }];
        screen.mode = MirrorViewMode::Restore;
        screen.selected_backup_idx = 0;

        // Test view (v) pushes ViewerScreen
        let initial_screens = app.screen_count();
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('v')));
        assert_eq!(app.screen_count(), initial_screens + 1);
        app.on_key(KeyEvent::from(KeyCode::Esc));

        // Test diff (c) pushes ViewerScreen
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('c')));
        assert_eq!(app.screen_count(), initial_screens + 1);
        app.on_key(KeyEvent::from(KeyCode::Esc));

        // Test restore (r) sets AwaitKind::RestoreBackup
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('r')));
        assert!(matches!(
            screen.await_kind,
            Some(AwaitKind::RestoreBackup(_, _))
        ));
        screen.on_confirm(&mut app, false);
        assert!(screen.await_kind.is_none());

        // Test delete (d) sets AwaitKind::DeleteBackup and confirm deletes in live time
        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('d')));
        assert!(matches!(
            screen.await_kind,
            Some(AwaitKind::DeleteBackup(_))
        ));
        screen.on_confirm(&mut app, false);
        assert!(screen.await_kind.is_none());
        assert_eq!(screen.backups.len(), 1);

        screen.handle_key(&mut app, KeyEvent::from(KeyCode::Char('d')));
        assert!(matches!(
            screen.await_kind,
            Some(AwaitKind::DeleteBackup(_))
        ));
        screen.on_confirm(&mut app, true);
        assert!(screen.await_kind.is_none());
        assert!(screen.backups.is_empty());
        assert!(!bkp.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
