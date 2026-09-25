// ──────────────────────────────────────────────────────────────────────
// screens/mod.rs — one module per archman feature, mirroring lib/*.sh
// ──────────────────────────────────────────────────────────────────────

pub mod exportimport;
pub mod favorites;
pub mod groups;
pub mod home;
pub mod info;
pub mod install;
pub mod lockfile;
pub mod mirrors;
pub mod packages;
pub mod preview;
pub mod registry;
pub mod remove;
pub mod repos;
pub mod runpane;
pub mod search;
pub mod settingsscr;
pub mod update;
pub mod viewer;

pub use home::HomeScreen;
pub use preview::PreviewScreen;

use ratatui::style::Style;
use ratatui::text::Span;

/// Render selected `pacman -Si` / `-Qi` fields as aligned kv lines,
/// mirroring the grep-filtered display from the bash UI. An empty
/// `fields` slice renders every keyed line.
pub fn info_lines(raw: &str, fields: &[&str]) -> Vec<Vec<Span<'static>>> {
    let mut out = Vec::new();
    for line in raw.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            if fields.is_empty() || fields.contains(&key) {
                out.push(vec![
                    crate::widgets::span(format!("  {key:<18}"), crate::widgets::accent_bold()),
                    Span::styled(value.trim().to_string(), Style::new()),
                ]);
            }
        }
    }
    out
}

/// Helper: owned arg vector from literals.
pub fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}
