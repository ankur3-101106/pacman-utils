// ──────────────────────────────────────────────────────────────────────
// info.rs — System information dashboard (lib/info.sh)
//
// Gathers system/pacman/package/storage stats on a background thread
// and renders four key-value sections.
// ──────────────────────────────────────────────────────────────────────

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::time::Instant;

use crate::app::{App, Screen};
use crate::sys::{self, Job};
use crate::widgets;

struct SysInfo {
    hostname: String,
    kernel: String,
    arch: String,
    uptime: String,
    pacman_ver: String,
    mirrors: usize,
    aur_helper_info: String,
    total_pkgs: usize,
    explicit_pkgs: usize,
    aur_pkgs: usize,
    orphan_pkgs: usize,
    cache_size: Option<u64>,
    disk_usage: String,
}

pub struct InfoScreen {
    job: Option<Job<SysInfo>>,
    info: Option<SysInfo>,
}

impl InfoScreen {
    pub fn new_with_app(app: &App) -> Box<Self> {
        let helper_info = app.aur_helper().map(|h| {
            let ver = crate::sys::capture(&h, &["--version"])
                .and_then(|v| v.lines().next().map(str::to_string))
                .unwrap_or_default();
            format!("{h} ({ver})")
        });
        Box::new(Self::with_helper(helper_info))
    }

    fn with_helper(helper_info: Option<String>) -> Self {
        Self {
            info: None,
            job: Some(Job::spawn("Gathering system information...", move || {
                let (cache_bytes, _) = sys::cache_stats();
                SysInfo {
                    hostname: sys::hostname(),
                    kernel: sys::kernel_version(),
                    arch: sys::arch(),
                    uptime: sys::uptime_pretty(),
                    pacman_ver: sys::pacman_version(),
                    mirrors: sys::mirror_count(),
                    aur_helper_info: helper_info.unwrap_or_else(|| "not installed".into()),
                    total_pkgs: count_of(&["-Q"]),
                    explicit_pkgs: count_of(&["-Qe"]),
                    aur_pkgs: count_of(&["-Qm"]),
                    orphan_pkgs: count_of(&["-Qtdq"]),
                    cache_size: cache_bytes,
                    disk_usage: sys::disk_usage_root(),
                }
            })),
        }
    }
}

fn count_of(flag: &[&str]) -> usize {
    sys::capture("pacman", flag).map(|s| s.lines().count()).unwrap_or(0)
}

impl Screen for InfoScreen {
    fn handle_key(&mut self, app: &mut App, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.pop(),
            _ => {}
        }
    }

    fn poll(&mut self, _app: &mut App) {
        if let Some(mut job) = self.job.take() {
            match job.poll() {
                Some(info) => self.info = Some(info),
                None => self.job = Some(job),
            }
        }
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) {
        let block = widgets::panel("📊  System Information");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let Some(info) = &self.info else { return };

        let rows = Layout::vertical([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 5),
        ])
        .split(inner);

        let section = |f: &mut Frame<'_>, rect: Rect, title: &str, rows_kv: Vec<(&str, String)>| {
            let width = rows_kv.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
            let mut lines: Vec<Line> = vec![Line::from(widgets::span(format!("  ── {title} ──"), widgets::dim()))];
            for (k, v) in rows_kv {
                lines.push(Line::from(vec![
                    widgets::span(format!("  {k:<width$} │ "), widgets::accent_bold()),
                    Span::styled(v, Style::new()),
                ]));
            }
            f.render_widget(Paragraph::new(lines), rect);
        };

        section(
            f,
            rows[0],
            "System",
            vec![
                ("Hostname", info.hostname.clone()),
                ("Kernel", info.kernel.clone()),
                ("Architecture", info.arch.clone()),
                ("Uptime", info.uptime.clone()),
            ],
        );
        section(
            f,
            rows[1],
            "Pacman",
            vec![
                ("Pacman Version", info.pacman_ver.clone()),
                ("Mirror Count", info.mirrors.to_string()),
                ("AUR Helper", info.aur_helper_info.clone()),
            ],
        );

        // Packages section gains an orphan warning line like the bash UI.
        let mut pkg_rows = vec![
            ("Total Packages", info.total_pkgs.to_string()),
            ("Explicit Packages", info.explicit_pkgs.to_string()),
            ("AUR Packages", info.aur_pkgs.to_string()),
            ("Orphan Packages", info.orphan_pkgs.to_string()),
        ];
        if info.orphan_pkgs > 0 {
            pkg_rows.push((
                "⚠",
                format!("{} orphan packages found. Consider cleaning them.", info.orphan_pkgs),
            ));
        }
        section(f, rows[2], "Packages", pkg_rows);

        section(
            f,
            rows[3],
            "Storage",
            vec![
                (
                    "Cache Size",
                    info.cache_size.map(sys::human_size).unwrap_or_else(|| "unknown".into()),
                ),
                ("Disk Usage", info.disk_usage.clone()),
            ],
        );
    }

    fn busy(&self) -> Option<(String, Instant)> {
        self.job.as_ref().map(|j| (j.label.clone(), j.started))
    }

    fn help_hints(&self) -> Vec<&'static str> {
        vec!["esc back"]
    }
}
