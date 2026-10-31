// ──────────────────────────────────────────────────────────────────────
// mirrors/ranking.rs — Ranking Engine & Backend Adapters
// ──────────────────────────────────────────────────────────────────────

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::backup::{create_backup_fs, generate_backup_path};
use super::discovery::parse_mirrorlist_content;
use super::model::{MirrorEntry, MirrorProvider, ProviderId, RankingBackend};
use crate::cmd::CommandSpec;

/// Options for configuring Reflector ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectorOptions {
    pub country: Option<String>,
    pub https_only: bool,
    pub latest: usize,
    pub sort: String,
    pub max_age_hours: Option<u32>,
    pub number: usize,
}

impl Default for ReflectorOptions {
    fn default() -> Self {
        Self {
            country: None,
            https_only: true,
            latest: 20,
            sort: "rate".to_string(),
            max_age_hours: None,
            number: 10,
        }
    }
}

/// Options for configuring rate-mirrors ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateMirrorsOptions {
    pub max_mirrors: Option<usize>,
    pub protocol: Option<String>,
    pub country: Option<String>,
    pub timeout_ms: Option<u32>,
}

impl Default for RateMirrorsOptions {
    fn default() -> Self {
        Self {
            max_mirrors: Some(15),
            protocol: Some("https".to_string()),
            country: None,
            timeout_ms: None,
        }
    }
}

/// Validate candidate mirrorlist content before applying it to the system.
pub fn validate_candidate_output(content: &str) -> Result<Vec<MirrorEntry>, String> {
    if content.trim().is_empty() {
        return Err("Candidate mirror output is empty".to_string());
    }

    let (active, _) = parse_mirrorlist_content(content);
    if active.is_empty() {
        return Err(
            "Candidate output does not contain any active 'Server = ...' mirrors".to_string(),
        );
    }

    for m in &active {
        let u = m.url.to_ascii_lowercase();
        if !u.starts_with("http://") && !u.starts_with("https://") && !u.starts_with("ftp://") {
            return Err(format!("Candidate mirror contains invalid URL: {}", m.url));
        }
    }

    Ok(active)
}

/// Validate candidate mirrorlist file on disk.
pub fn validate_candidate_file(candidate_path: &Path) -> Result<Vec<MirrorEntry>, String> {
    let content = fs::read_to_string(candidate_path).map_err(|e| {
        format!(
            "Failed to read candidate file {}: {e}",
            candidate_path.display()
        )
    })?;
    validate_candidate_output(&content)
}

/// Generate a secure temporary path for candidate ranking results.
pub fn candidate_temp_path(provider_id: &ProviderId) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("archman_rank_{}_{nanos}.tmp", provider_id.as_str()))
}

/// An executable ranking plan containing candidate destination and validation parameters.
#[derive(Debug, Clone)]
pub struct RankingPlan {
    pub provider_id: ProviderId,
    pub provider_name: String,
    pub target_mirrorlist: PathBuf,
    pub candidate_temp_file: PathBuf,
    pub backup_path: PathBuf,
    pub generate_cmd: CommandSpec,
}

impl RankingPlan {
    /// Build command to safely apply candidate file after validation and backup.
    pub fn build_apply_command(&self) -> CommandSpec {
        let target_str = self.target_mirrorlist.to_string_lossy().into_owned();
        let backup_str = self.backup_path.to_string_lossy().into_owned();
        let cand_str = self.candidate_temp_file.to_string_lossy().into_owned();

        // 1. Back up current mirrorlist
        // 2. Atomically copy candidate file to target
        // 3. Remove candidate file
        let script = "cp -p \"$1\" \"$2\" && cp -p \"$3\" \"$1\" && rm -f \"$3\"";
        let args = vec![
            "sh".to_string(),
            "-c".to_string(),
            script.to_string(),
            "apply-rank".to_string(),
            target_str.clone(),
            backup_str,
            cand_str,
        ];

        CommandSpec::new("mirrors-apply-rank", "sudo", &args)
            .note(format!("Apply ranked mirrors to {target_str}"))
            .result(
                "Mirrorlist updated with ranked mirrors!",
                "Failed to apply ranked mirrorlist.",
                "MIRRORS: Ranking applied",
            )
    }

    /// Direct filesystem apply (with backup and atomic replacement).
    pub fn apply_candidate_fs(&self) -> Result<usize, String> {
        let mirrors = validate_candidate_file(&self.candidate_temp_file)?;

        if self.target_mirrorlist.exists() {
            let _ = create_backup_fs(&self.target_mirrorlist);
        }

        // Atomic replacement
        let parent = self
            .target_mirrorlist
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let staged = parent.join(format!(
            ".staged_rank_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        fs::copy(&self.candidate_temp_file, &staged)
            .map_err(|e| format!("Failed to stage candidate: {e}"))?;

        if let Err(e) = fs::rename(&staged, &self.target_mirrorlist) {
            let _ = fs::remove_file(&staged);
            return Err(format!(
                "Failed to atomically update target mirrorlist: {e}"
            ));
        }

        let _ = fs::remove_file(&self.candidate_temp_file);
        Ok(mirrors.len())
    }
}

/// Create a ranking plan for a provider.
pub fn create_ranking_plan(
    provider: &MirrorProvider,
    target_path: Option<&Path>,
    reflector_opts: Option<&ReflectorOptions>,
    rate_opts: Option<&RateMirrorsOptions>,
) -> Result<RankingPlan, String> {
    let target = match target_path {
        Some(t) => t.to_path_buf(),
        None => match provider.primary_mirrorlist() {
            Some(p) => p.to_path_buf(),
            None => {
                return Err(format!(
                    "No target mirrorlist resolved for provider '{}'",
                    provider.name
                ))
            }
        },
    };

    if !target.is_absolute() {
        return Err(format!(
            "Target path must be absolute: {}",
            target.display()
        ));
    }

    let cand_temp = candidate_temp_path(&provider.id);
    let backup_path = generate_backup_path(&target);

    let generate_cmd = match &provider.backend {
        RankingBackend::Reflector {
            country,
            latest,
            sort,
        } => {
            let def_opts = ReflectorOptions::default();
            let opts = reflector_opts.unwrap_or(&def_opts);

            let mut args = vec![
                "reflector".to_string(),
                "--save".to_string(),
                cand_temp.to_string_lossy().into_owned(),
            ];

            if let Some(c) = opts.country.as_deref().or(country.as_deref()) {
                if !c.trim().is_empty() {
                    args.push("--country".to_string());
                    args.push(c.trim().to_string());
                }
            }

            if opts.https_only {
                args.push("--protocol".to_string());
                args.push("https".to_string());
            }

            let l = if opts.latest > 0 {
                opts.latest
            } else {
                *latest
            };
            args.push("--latest".to_string());
            args.push(l.to_string());

            let s = if !opts.sort.is_empty() {
                &opts.sort
            } else {
                sort
            };
            args.push("--sort".to_string());
            args.push(s.clone());

            if let Some(age) = opts.max_age_hours {
                args.push("--age".to_string());
                args.push(age.to_string());
            }

            if opts.number > 0 {
                args.push("--number".to_string());
                args.push(opts.number.to_string());
            }

            CommandSpec::new("mirrors-rank-generate", "reflector", &args[1..])
                .note(format!("Rank mirrors for {} with reflector", provider.name))
        }

        RankingBackend::RateMirrors {
            provider: subcmd,
            extra_args,
        } => {
            let def_opts = RateMirrorsOptions::default();
            let opts = rate_opts.unwrap_or(&def_opts);

            let mut args = vec![
                "rate-mirrors".to_string(),
                "--allow-root".to_string(),
                "--save".to_string(),
                cand_temp.to_string_lossy().into_owned(),
            ];

            if let Some(max) = opts.max_mirrors {
                args.push("--max-mirrors-to-output".to_string());
                args.push(max.to_string());
            }

            if let Some(ref proto) = opts.protocol {
                args.push("--protocol".to_string());
                args.push(proto.clone());
            }

            if let Some(ref c) = opts.country {
                if !c.trim().is_empty() {
                    args.push("--include-countries".to_string());
                    args.push(c.trim().to_string());
                }
            }

            for extra in extra_args {
                args.push(extra.clone());
            }

            // Subcommand identifier must come last for rate-mirrors
            args.push(subcmd.clone());

            CommandSpec::new("mirrors-rank-generate", "rate-mirrors", &args[1..]).note(format!(
                "Rank mirrors for {} with rate-mirrors",
                provider.name
            ))
        }

        RankingBackend::Native { executable, args } => {
            let mut cmd_args = args.clone();
            // If manjaro pacman-mirrors
            if executable.contains("pacman-mirrors") {
                cmd_args = vec!["-f".to_string(), "5".to_string()];
            }

            CommandSpec::new("mirrors-rank-generate", executable, &cmd_args).note(format!(
                "Rank mirrors for {} using native tool",
                provider.name
            ))
        }

        RankingBackend::Unsupported => {
            return Err(format!(
                "Provider '{}' does not have a supported ranking backend",
                provider.name
            ))
        }
    };

    Ok(RankingPlan {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        target_mirrorlist: target,
        candidate_temp_file: cand_temp,
        backup_path,
        generate_cmd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirrors::model::{DetectionStatus, MirrorListTarget, ProviderCapabilities};

    #[test]
    fn test_ranking_valid_output() {
        let sample = r#"
# Comment header
Server = https://mirror.rackspace.com/archlinux/$repo/os/$arch
Server = http://ftp.fau.de/archlinux/$repo/os/$arch
# Server = https://disabled.com
"#;
        let mirrors = validate_candidate_output(sample).unwrap();
        assert_eq!(mirrors.len(), 2);
        assert_eq!(
            mirrors[0].url,
            "https://mirror.rackspace.com/archlinux/$repo/os/$arch"
        );
        assert_eq!(mirrors[1].url, "http://ftp.fau.de/archlinux/$repo/os/$arch");
    }

    #[test]
    fn test_ranking_empty_output() {
        let err1 = validate_candidate_output("").unwrap_err();
        assert!(err1.contains("empty"));

        let err2 =
            validate_candidate_output("  \n# only comments\n# Server commented\n").unwrap_err();
        assert!(err2.contains("active"));
    }

    #[test]
    fn test_ranking_invalid_urls() {
        let sample = "Server = not-a-valid-protocol://foo/bar\n";
        let err = validate_candidate_output(sample).unwrap_err();
        assert!(err.contains("invalid URL"));
    }

    #[test]
    fn test_ranking_unsupported_provider() {
        let p = MirrorProvider {
            id: ProviderId::new("unsupported-id"),
            name: "Unsupported Distro".to_string(),
            dist_type: "Unknown".to_string(),
            detection: DetectionStatus::Detected,
            repos: vec!["repo".to_string()],
            mirrorlists: vec![MirrorListTarget::new("/tmp/test-ml")],
            backend: RankingBackend::Unsupported,
            capabilities: ProviderCapabilities::default(),
            last_ranked: None,
            is_custom: false,
        };

        let err = create_ranking_plan(&p, None, None, None).unwrap_err();
        assert!(err.contains("supported ranking backend"));
    }

    #[test]
    fn test_ranking_reflector_options() {
        let p = MirrorProvider {
            id: ProviderId::new("arch"),
            name: "Arch Linux".to_string(),
            dist_type: "Arch Linux".to_string(),
            detection: DetectionStatus::Detected,
            repos: vec!["core".to_string()],
            mirrorlists: vec![MirrorListTarget::new("/tmp/test-mirrorlist")],
            backend: RankingBackend::Reflector {
                country: None,
                latest: 20,
                sort: "rate".to_string(),
            },
            capabilities: ProviderCapabilities::default(),
            last_ranked: None,
            is_custom: false,
        };

        let opts = ReflectorOptions {
            country: Some("Germany,France".to_string()),
            https_only: true,
            latest: 15,
            sort: "score".to_string(),
            max_age_hours: Some(12),
            number: 5,
        };

        let plan = create_ranking_plan(&p, None, Some(&opts), None).unwrap();
        assert_eq!(plan.generate_cmd.program, "reflector");
        assert!(plan.generate_cmd.args.contains(&"--country".to_string()));
        assert!(plan
            .generate_cmd
            .args
            .contains(&"Germany,France".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--protocol".to_string()));
        assert!(plan.generate_cmd.args.contains(&"https".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--latest".to_string()));
        assert!(plan.generate_cmd.args.contains(&"15".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--sort".to_string()));
        assert!(plan.generate_cmd.args.contains(&"score".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--age".to_string()));
        assert!(plan.generate_cmd.args.contains(&"12".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--number".to_string()));
        assert!(plan.generate_cmd.args.contains(&"5".to_string()));
    }

    #[test]
    fn test_ranking_rate_mirrors_options() {
        let p = MirrorProvider {
            id: ProviderId::new("cachyos"),
            name: "CachyOS".to_string(),
            dist_type: "CachyOS".to_string(),
            detection: DetectionStatus::Detected,
            repos: vec!["cachyos".to_string()],
            mirrorlists: vec![MirrorListTarget::new("/tmp/test-cachyos-mirrorlist")],
            backend: RankingBackend::RateMirrors {
                provider: "cachyos".to_string(),
                extra_args: vec!["--disable-comments".to_string()],
            },
            capabilities: ProviderCapabilities::default(),
            last_ranked: None,
            is_custom: false,
        };

        let opts = RateMirrorsOptions {
            max_mirrors: Some(10),
            protocol: Some("https".to_string()),
            country: Some("US,DE".to_string()),
            timeout_ms: None,
        };

        let plan = create_ranking_plan(&p, None, None, Some(&opts)).unwrap();
        assert_eq!(plan.generate_cmd.program, "rate-mirrors");
        assert!(plan.generate_cmd.args.contains(&"--allow-root".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--save".to_string()));
        assert!(plan
            .generate_cmd
            .args
            .contains(&"--max-mirrors-to-output".to_string()));
        assert!(plan.generate_cmd.args.contains(&"10".to_string()));
        assert!(plan.generate_cmd.args.contains(&"--protocol".to_string()));
        assert!(plan.generate_cmd.args.contains(&"https".to_string()));
        assert!(plan
            .generate_cmd
            .args
            .contains(&"--include-countries".to_string()));
        assert!(plan.generate_cmd.args.contains(&"US,DE".to_string()));
        assert!(plan
            .generate_cmd
            .args
            .contains(&"--disable-comments".to_string()));
        // Provider subcommand must be last
        assert_eq!(plan.generate_cmd.args.last().unwrap(), "cachyos");
    }

    #[test]
    fn test_ranking_apply_fs_lifecycle() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("archman_test_rank_apply_{nanos}"));
        fs::create_dir_all(&dir).unwrap();

        let target = dir.join("mirrorlist");
        let cand = dir.join("candidate.tmp");

        fs::write(&target, "Server = https://old.mirror.org\n").unwrap();
        fs::write(
            &cand,
            "Server = https://new1.mirror.org\nServer = https://new2.mirror.org\n",
        )
        .unwrap();

        let plan = RankingPlan {
            provider_id: ProviderId::new("arch"),
            provider_name: "Arch Linux".to_string(),
            target_mirrorlist: target.clone(),
            candidate_temp_file: cand.clone(),
            backup_path: dir.join("mirrorlist.bak"),
            generate_cmd: CommandSpec::new("test", "echo", &[]),
        };

        let count = plan.apply_candidate_fs().unwrap();
        assert_eq!(count, 2);

        // Verify target now contains new mirrors
        let new_content = fs::read_to_string(&target).unwrap();
        assert!(new_content.contains("https://new1.mirror.org"));
        assert!(new_content.contains("https://new2.mirror.org"));
        assert!(!new_content.contains("https://old.mirror.org"));

        // Verify candidate file was removed
        assert!(!cand.exists());

        // Verify backup was created
        let backups = crate::mirrors::backup::list_backups(&target);
        assert!(!backups.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
