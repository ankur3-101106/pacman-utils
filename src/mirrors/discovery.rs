// ──────────────────────────────────────────────────────────────────────
// mirrors/discovery.rs — Provider & Mirrorlist Discovery Engine
// ──────────────────────────────────────────────────────────────────────

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::custom::load_custom_providers;
use super::model::{
    DetectionStatus, MirrorEntry, MirrorListTarget, MirrorProvider, ProviderId, RankingBackend,
};
use super::registry::{builtin_registry, match_mirrorlist_filename, match_repo_section};

/// Discovered pacman repository section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacmanRepoSection {
    pub name: String,
    pub includes: Vec<PathBuf>,
    pub servers: Vec<String>,
}

/// Parse mirrorlist file into active and commented mirror entries.
pub fn parse_mirrorlist_file(path: &Path) -> (Vec<MirrorEntry>, Vec<MirrorEntry>) {
    let Ok(content) = fs::read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    parse_mirrorlist_content(&content)
}

/// Parse mirrorlist content string into active and commented mirror entries.
pub fn parse_mirrorlist_content(content: &str) -> (Vec<MirrorEntry>, Vec<MirrorEntry>) {
    let mut active = Vec::new();
    let mut commented = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Active mirror: Server = ...
        if let Some(rest) = trimmed.strip_prefix("Server") {
            let rest = rest.trim_start();
            if let Some(url_part) = rest.strip_prefix('=') {
                let url = url_part.trim().to_string();
                if !url.is_empty() {
                    active.push(MirrorEntry {
                        url,
                        active: true,
                        comment: None,
                    });
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim_start();
            if let Some(srv_part) = rest.strip_prefix("Server") {
                let srv_part = srv_part.trim_start();
                if let Some(url_part) = srv_part.strip_prefix('=') {
                    let url = url_part.trim().to_string();
                    if !url.is_empty() {
                        commented.push(MirrorEntry {
                            url,
                            active: false,
                            comment: Some(trimmed.to_string()),
                        });
                    }
                }
            }
        }
    }

    (active, commented)
}

/// Expand wildcard file paths (e.g. `/etc/pacman.d/*.conf`).
pub fn expand_wildcard_path(pattern: &Path) -> Vec<PathBuf> {
    let pat_str = pattern.to_string_lossy();
    if !pat_str.contains('*') && !pat_str.contains('?') {
        return vec![pattern.to_path_buf()];
    }

    let Some(parent) = pattern.parent() else {
        return vec![pattern.to_path_buf()];
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };

    let filename_pat = pattern
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut matched = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Some(fname) = p.file_name().and_then(|f| f.to_str()) {
                if wildcard_match(&filename_pat, fname) {
                    matched.push(p);
                }
            }
        }
    }
    matched.sort();
    matched
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p_bytes = pattern.as_bytes();
    let t_bytes = text.as_bytes();
    let mut p = 0;
    let mut t = 0;
    let mut star = None;
    let mut match_idx = 0;

    while t < t_bytes.len() {
        if p < p_bytes.len() && (p_bytes[p] == b'?' || p_bytes[p] == t_bytes[t]) {
            p += 1;
            t += 1;
        } else if p < p_bytes.len() && p_bytes[p] == b'*' {
            star = Some(p);
            p += 1;
            match_idx = t;
        } else if let Some(sp) = star {
            p = sp + 1;
            match_idx += 1;
            t = match_idx;
        } else {
            return false;
        }
    }

    while p < p_bytes.len() && p_bytes[p] == b'*' {
        p += 1;
    }
    p == p_bytes.len()
}

/// Parse a pacman.conf file and any top-level included files recursively.
pub fn parse_pacman_conf(path: &Path) -> Vec<PacmanRepoSection> {
    let mut visited = HashSet::new();
    parse_pacman_conf_recursive(path, &mut visited)
}

fn parse_pacman_conf_recursive(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Vec<PacmanRepoSection> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        return Vec::new();
    }

    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut sections = Vec::new();
    let mut current_section: Option<PacmanRepoSection> = None;
    let mut in_options = false;

    let flush = |sec: Option<PacmanRepoSection>, out: &mut Vec<PacmanRepoSection>| {
        if let Some(s) = sec {
            out.push(s);
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Section header
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            flush(current_section.take(), &mut sections);
            let sec_name = trimmed[1..trimmed.len() - 1].trim();
            if sec_name.eq_ignore_ascii_case("options") {
                in_options = true;
            } else {
                in_options = false;
                current_section = Some(PacmanRepoSection {
                    name: sec_name.to_string(),
                    includes: Vec::new(),
                    servers: Vec::new(),
                });
            }
            continue;
        }

        // Directives
        if let Some((key, val)) = trimmed.split_once('=') {
            let key = key.trim();
            let val = val.trim();

            if key.eq_ignore_ascii_case("Include") {
                let inc_path = PathBuf::from(val);
                let expanded = expand_wildcard_path(&inc_path);

                if in_options {
                    // Includes inside [options] might define additional repositories
                    for inc in expanded {
                        let sub_secs = parse_pacman_conf_recursive(&inc, visited);
                        sections.extend(sub_secs);
                    }
                } else if let Some(ref mut sec) = current_section {
                    sec.includes.extend(expanded);
                }
            } else if key.eq_ignore_ascii_case("Server") {
                if let Some(ref mut sec) = current_section {
                    sec.servers.push(val.to_string());
                }
            }
        }
    }

    flush(current_section, &mut sections);
    sections
}

/// Discover all configured and supported providers.
pub fn discover_providers() -> Vec<MirrorProvider> {
    discover_providers_from_path(Path::new("/etc/pacman.conf"))
}

/// Discover providers starting from a specific pacman.conf path.
pub fn discover_providers_from_path(pacman_conf_path: &Path) -> Vec<MirrorProvider> {
    let sections = parse_pacman_conf(pacman_conf_path);

    // 1. Instantiate all built-in providers
    let mut providers: Vec<MirrorProvider> = builtin_registry()
        .into_iter()
        .map(|def| def.instantiate())
        .collect();

    // 2. Load custom providers
    let custom_providers = load_custom_providers();
    for cp in custom_providers {
        let p = cp.to_provider();
        if let Some(pos) = providers.iter().position(|x| x.id == p.id) {
            providers[pos] = p;
        } else {
            providers.push(p);
        }
    }

    // Map provider ID -> indices in providers list
    let id_to_index: HashMap<String, usize> = providers
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.to_string(), i))
        .collect();

    // Track targets by (provider_id, target_path) to prevent duplicates
    let mut provider_targets: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut target_repos: HashMap<(String, PathBuf), HashSet<String>> = HashMap::new();

    // 3. Map configured repository sections to providers
    let mut unknown_sections = Vec::new();

    for sec in &sections {
        let mut matched_id = None;

        // Try matching custom providers by repo list
        for p in &providers {
            if p.is_custom && p.repos.iter().any(|r| r.eq_ignore_ascii_case(&sec.name)) {
                matched_id = Some(p.id.clone());
                break;
            }
        }

        // Try matching built-in by repo section name
        if matched_id.is_none() {
            matched_id = match_repo_section(&sec.name);
        }

        // Try matching by mirrorlist filename in includes
        if matched_id.is_none() {
            for inc in &sec.includes {
                if let Some(fname) = inc.file_name().and_then(|f| f.to_str()) {
                    if let Some(id) = match_mirrorlist_filename(fname) {
                        matched_id = Some(id);
                        break;
                    }
                }
            }
        }

        if let Some(id) = matched_id {
            if let Some(&idx) = id_to_index.get(id.as_str()) {
                let provider = &mut providers[idx];
                if !provider.repos.contains(&sec.name) {
                    provider.repos.push(sec.name.clone());
                }

                for inc in &sec.includes {
                    let targets = provider_targets.entry(id.to_string()).or_default();
                    if !targets.contains(inc) {
                        targets.push(inc.clone());
                    }
                    target_repos
                        .entry((id.to_string(), inc.clone()))
                        .or_default()
                        .insert(sec.name.clone());
                }
            }
        } else {
            unknown_sections.push(sec.clone());
        }
    }

    // 4. Populate mirrorlists and parse targets for known providers
    for provider in &mut providers {
        if let Some(targets) = provider_targets.remove(&provider.id.to_string()) {
            for target_path in targets {
                let mut target = MirrorListTarget::new(&target_path);
                if target_path.exists() {
                    target.exists = true;
                    let (act, com) = parse_mirrorlist_file(&target_path);
                    target.active_mirrors = act;
                    target.commented_mirrors = com;
                } else {
                    target.exists = false;
                }

                if let Some(repos) =
                    target_repos.get(&(provider.id.to_string(), target_path.clone()))
                {
                    target.associated_repos = repos.iter().cloned().collect();
                    target.associated_repos.sort();
                }

                provider.mirrorlists.push(target);
            }
        } else if provider.is_custom {
            // For custom providers not matched in pacman.conf, inspect their configured mirrorlist
            for target in &mut provider.mirrorlists {
                if target.path.exists() {
                    target.exists = true;
                    let (act, com) = parse_mirrorlist_file(&target.path);
                    target.active_mirrors = act;
                    target.commented_mirrors = com;
                }
            }
        }
    }

    // 5. Add unknown sections as dynamically discovered third-party providers
    for sec in unknown_sections {
        let mut target_paths = sec.includes.clone();
        if target_paths.is_empty() && !sec.servers.is_empty() {
            // Inlined server repository without a mirrorlist
            target_paths.push(pacman_conf_path.to_path_buf());
        }

        let mut mirrorlists = Vec::new();
        for tp in target_paths {
            let mut target = MirrorListTarget::new(&tp);
            if tp.exists() {
                target.exists = true;
                let (act, com) = parse_mirrorlist_file(&tp);
                target.active_mirrors = act;
                target.commented_mirrors = com;
            }
            target.associated_repos = vec![sec.name.clone()];
            mirrorlists.push(target);
        }

        let id = ProviderId::new(format!("repo-{}", sec.name));
        let p = MirrorProvider {
            id,
            name: format!("Custom Repo [{}]", sec.name),
            dist_type: "Third-party Repository".to_string(),
            detection: DetectionStatus::Detected,
            repos: vec![sec.name.clone()],
            mirrorlists,
            backend: RankingBackend::Unsupported,
            capabilities: super::model::ProviderCapabilities {
                can_rank: false,
                can_benchmark: true,
                supports_country_filter: false,
                supports_protocols: true,
            },
            last_ranked: None,
            is_custom: true,
        };
        providers.push(p);
    }

    // 6. Check DetectionStatus for all providers
    for provider in &mut providers {
        provider.detection = evaluate_detection_status(provider);
    }

    providers
}

/// Determine detection status for a provider given its configured repos and mirrorlists.
fn evaluate_detection_status(provider: &MirrorProvider) -> DetectionStatus {
    // If no repos configured in pacman.conf:
    if provider.repos.is_empty() && !provider.is_custom {
        // Check if any default mirrorlists exist on disk
        let def = find_builtin_def(provider.id.as_str());
        if let Some(def) = def {
            let pacman_d = Path::new("/etc/pacman.d");
            for pat in def.file_patterns {
                let candidate = pacman_d.join(pat);
                if candidate.exists() {
                    return DetectionStatus::PartiallyConfigured(format!(
                        "Mirrorlist '{}' exists on disk, but repository is not enabled in pacman.conf",
                        pat
                    ));
                }
            }
        }
        return DetectionStatus::NotConfigured;
    }

    // If repos configured, check mirrorlists
    if provider.mirrorlists.is_empty() {
        return DetectionStatus::PartiallyConfigured(
            "Repository enabled without any resolved Include mirrorlist".to_string(),
        );
    }

    for target in &provider.mirrorlists {
        if !target.exists {
            return DetectionStatus::MissingMirrorlist(target.path.clone());
        }
    }

    // Check backend availability
    match &provider.backend {
        RankingBackend::Reflector { .. } => {
            if !crate::sys::has_bin("reflector") {
                if crate::sys::has_bin("rate-mirrors") {
                    // Reflector not installed, but rate-mirrors is available as fallback
                    DetectionStatus::Detected
                } else {
                    DetectionStatus::BackendUnavailable("reflector is not installed".to_string())
                }
            } else {
                DetectionStatus::Detected
            }
        }
        RankingBackend::RateMirrors { .. } => {
            if !crate::sys::has_bin("rate-mirrors") {
                DetectionStatus::BackendUnavailable("rate-mirrors is not installed".to_string())
            } else {
                DetectionStatus::Detected
            }
        }
        RankingBackend::Native { executable, .. } => {
            if !crate::sys::has_bin(executable) {
                DetectionStatus::BackendUnavailable(format!("{executable} is not installed"))
            } else {
                DetectionStatus::Detected
            }
        }
        RankingBackend::Unsupported => {
            DetectionStatus::Unsupported("No ranking backend configured".to_string())
        }
    }
}

fn find_builtin_def(id: &str) -> Option<super::registry::ProviderDefinition> {
    builtin_registry().into_iter().find(|d| d.id.as_str() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("archman_test_{prefix}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_discovery_arch_only() {
        let dir = create_test_dir("arch_only");
        let mirrorlist_path = dir.join("mirrorlist");
        fs::write(
            &mirrorlist_path,
            "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n#Server = http://slow.com\n",
        )
        .unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[options]
Architecture = auto

[core]
Include = {}

[extra]
Include = {}
"#,
            mirrorlist_path.display(),
            mirrorlist_path.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        assert!(
            arch.detection.is_detected()
                || matches!(arch.detection, DetectionStatus::BackendUnavailable(_))
        );
        assert_eq!(arch.repos.len(), 2);
        assert!(arch.repos.contains(&"core".to_string()));
        assert!(arch.repos.contains(&"extra".to_string()));
        assert_eq!(arch.mirrorlists.len(), 1); // Deduplicated!
        assert_eq!(arch.total_active_mirrors(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_cachyos_system() {
        let dir = create_test_dir("cachyos");
        let cachy_ml = dir.join("cachyos-mirrorlist");
        let cachy_v3_ml = dir.join("cachyos-v3-mirrorlist");

        fs::write(
            &cachy_ml,
            "Server = https://mirror.cachyos.org/repo/$arch/$repo\n",
        )
        .unwrap();
        fs::write(
            &cachy_v3_ml,
            "Server = https://mirror.cachyos.org/repo/$arch_v3/$repo\n",
        )
        .unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[cachyos-v3]
Include = {}

[cachyos]
Include = {}
"#,
            cachy_v3_ml.display(),
            cachy_ml.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let cachy = providers
            .iter()
            .find(|p| p.id.as_str() == "cachyos")
            .unwrap();
        assert_eq!(cachy.repos.len(), 2);
        assert_eq!(cachy.mirrorlists.len(), 2);
        assert_eq!(cachy.total_active_mirrors(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_multiple_providers() {
        let dir = create_test_dir("multi_prov");
        let arch_ml = dir.join("mirrorlist");
        let chaotic_ml = dir.join("chaotic-mirrorlist");

        fs::write(
            &arch_ml,
            "Server = https://arch.mirror.org/$repo/os/$arch\n",
        )
        .unwrap();
        fs::write(
            &chaotic_ml,
            "Server = https://chaotic.mirror.cx/$repo/$arch\n",
        )
        .unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[core]
Include = {}

[chaotic-aur]
Include = {}
"#,
            arch_ml.display(),
            chaotic_ml.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        let chaotic = providers
            .iter()
            .find(|p| p.id.as_str() == "chaotic-aur")
            .unwrap();

        assert_eq!(arch.repos, vec!["core"]);
        assert_eq!(chaotic.repos, vec!["chaotic-aur"]);
        assert_eq!(arch.mirrorlists.len(), 1);
        assert_eq!(chaotic.mirrorlists.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_multiple_include_files() {
        let dir = create_test_dir("multi_inc");
        let ml1 = dir.join("custom1-mirrorlist");
        let ml2 = dir.join("custom2-mirrorlist");

        fs::write(&ml1, "Server = https://ml1.example.org\n").unwrap();
        fs::write(&ml2, "Server = https://ml2.example.org\n").unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[core]
Include = {}
Include = {}
"#,
            ml1.display(),
            ml2.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        assert_eq!(arch.mirrorlists.len(), 2);
        assert_eq!(arch.total_active_mirrors(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_wildcard_includes() {
        let dir = create_test_dir("wildcard");
        let conf_d = dir.join("conf.d");
        fs::create_dir_all(&conf_d).unwrap();

        let ml1 = dir.join("ml-a-mirrorlist");
        let ml2 = dir.join("ml-b-mirrorlist");
        fs::write(&ml1, "Server = https://a.example.org\n").unwrap();
        fs::write(&ml2, "Server = https://b.example.org\n").unwrap();

        let extra_conf = conf_d.join("extra.conf");
        let extra_content = format!(
            r#"
[extra]
Include = {}
"#,
            ml1.display()
        );
        fs::write(&extra_conf, extra_content).unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[options]
Include = {}/*.conf

[core]
Include = {}
"#,
            conf_d.display(),
            ml2.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        assert!(arch.repos.contains(&"core".to_string()));
        assert!(arch.repos.contains(&"extra".to_string()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_duplicate_mirrorlist_targets() {
        let dir = create_test_dir("dedup");
        let ml = dir.join("shared-mirrorlist");
        fs::write(&ml, "Server = https://shared.example.org\n").unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[core]
Include = {}

[extra]
Include = {}

[multilib]
Include = {}
"#,
            ml.display(),
            ml.display(),
            ml.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        assert_eq!(arch.mirrorlists.len(), 1); // Exact deduplication!
        assert_eq!(arch.mirrorlists[0].associated_repos.len(), 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_missing_mirrorlist() {
        let dir = create_test_dir("missing_ml");
        let non_existent = dir.join("non_existent_mirrorlist");

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[blackarch]
Include = {}
"#,
            non_existent.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let blackarch = providers
            .iter()
            .find(|p| p.id.as_str() == "blackarch")
            .unwrap();
        assert_eq!(blackarch.repos, vec!["blackarch"]);
        assert!(matches!(
            blackarch.detection,
            DetectionStatus::MissingMirrorlist(_)
        ));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_unknown_provider() {
        let dir = create_test_dir("unknown");
        let custom_ml = dir.join("weird-mirrorlist");
        fs::write(&custom_ml, "Server = https://weird.repo.org/$repo\n").unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
[weird-personal-repo]
Include = {}
"#,
            custom_ml.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        let providers = discover_providers_from_path(&conf_path);
        let weird = providers
            .iter()
            .find(|p| p.id.as_str() == "repo-weird-personal-repo")
            .unwrap();
        assert_eq!(weird.repos, vec!["weird-personal-repo"]);
        assert_eq!(weird.backend, RankingBackend::Unsupported);
        assert_eq!(weird.total_active_mirrors(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_discovery_malformed_configuration() {
        let dir = create_test_dir("malformed");
        let ml = dir.join("valid-mirrorlist");
        fs::write(&ml, "Server = https://valid.com\n").unwrap();

        let conf_path = dir.join("pacman.conf");
        let conf_content = format!(
            r#"
# Random comment
[options
broken section header
Include
= = =
[core]
UnknownDirective = random_value
Include = {}
[
"#,
            ml.display()
        );
        fs::write(&conf_path, conf_content).unwrap();

        // Must not panic and must recover valid section [core]
        let providers = discover_providers_from_path(&conf_path);
        let arch = providers.iter().find(|p| p.id.as_str() == "arch").unwrap();
        assert!(arch.repos.contains(&"core".to_string()));
        assert_eq!(arch.total_active_mirrors(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
