// ──────────────────────────────────────────────────────────────────────
// mirrors/custom.rs — Custom Provider Configuration & Persistence
// ──────────────────────────────────────────────────────────────────────

use std::fs;
use std::path::{Path, PathBuf};

use super::model::{
    DetectionStatus, MirrorListTarget, MirrorProvider, ProviderCapabilities, ProviderId,
    RankingBackend,
};
use crate::settings;

/// Configuration record for a user-registered custom provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProvider {
    pub id: ProviderId,
    pub name: String,
    pub mirrorlist: PathBuf,
    pub backend_type: String,
    pub backend_arg: String,
    pub repos: Vec<String>,
}

impl CustomProvider {
    pub fn validate(&self) -> Result<(), String> {
        let id_str = self.id.as_str();
        if id_str.is_empty() {
            return Err("Provider ID cannot be empty".to_string());
        }
        if !id_str
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Provider ID must only contain letters, numbers, hyphens, and underscores"
                    .to_string(),
            );
        }
        if self.name.trim().is_empty() {
            return Err("Provider name cannot be empty".to_string());
        }
        if !self.mirrorlist.is_absolute() {
            return Err("Mirrorlist path must be an absolute path".to_string());
        }
        let b = self.backend_type.to_ascii_lowercase();
        match b.as_str() {
            "reflector" | "rate-mirrors" | "native" | "unsupported" => {}
            _ => {
                return Err(format!(
                    "Invalid backend '{b}'. Must be reflector, rate-mirrors, native, or unsupported"
                ))
            }
        }
        Ok(())
    }

    pub fn to_provider(&self) -> MirrorProvider {
        let backend = match self.backend_type.to_ascii_lowercase().as_str() {
            "reflector" => RankingBackend::Reflector {
                country: if self.backend_arg.is_empty() {
                    None
                } else {
                    Some(self.backend_arg.clone())
                },
                latest: 20,
                sort: "rate".to_string(),
            },
            "rate-mirrors" => RankingBackend::RateMirrors {
                provider: if self.backend_arg.is_empty() {
                    self.id.to_string()
                } else {
                    self.backend_arg.clone()
                },
                extra_args: Vec::new(),
            },
            "native" => RankingBackend::Native {
                executable: self.backend_arg.clone(),
                args: Vec::new(),
            },
            _ => RankingBackend::Unsupported,
        };

        let mut target = MirrorListTarget::new(&self.mirrorlist);
        target.associated_repos = self.repos.clone();

        MirrorProvider {
            id: self.id.clone(),
            name: self.name.clone(),
            dist_type: "Custom Provider".to_string(),
            detection: DetectionStatus::NotConfigured,
            repos: self.repos.clone(),
            mirrorlists: vec![target],
            backend,
            capabilities: ProviderCapabilities {
                can_rank: true,
                can_benchmark: true,
                supports_country_filter: self.backend_type == "reflector",
                supports_protocols: true,
            },
            last_ranked: None,
            is_custom: true,
        }
    }
}

pub fn custom_config_path() -> PathBuf {
    settings::config_dir().join("custom_mirrors.conf")
}

/// Load custom providers from user configuration file.
pub fn load_custom_providers() -> Vec<CustomProvider> {
    load_custom_providers_from(&custom_config_path())
}

pub fn load_custom_providers_from(path: &Path) -> Vec<CustomProvider> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut list = Vec::new();
    let mut current_id = None;
    let mut current_name = String::new();
    let mut current_path = PathBuf::new();
    let mut current_backend = "rate-mirrors".to_string();
    let mut current_arg = String::new();
    let mut current_repos = Vec::new();

    let flush = |id: Option<ProviderId>,
                 name: String,
                 path: PathBuf,
                 backend: String,
                 arg: String,
                 repos: Vec<String>,
                 out: &mut Vec<CustomProvider>| {
        if let Some(id) = id {
            let cp = CustomProvider {
                id,
                name: if name.is_empty() {
                    "Custom".to_string()
                } else {
                    name
                },
                mirrorlist: path,
                backend_type: backend,
                backend_arg: arg,
                repos,
            };
            if cp.validate().is_ok() {
                out.push(cp);
            }
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let id_str = &trimmed[1..trimmed.len() - 1].trim();
            if let Some(prev) = current_id.take() {
                flush(
                    Some(prev),
                    std::mem::take(&mut current_name),
                    std::mem::take(&mut current_path),
                    std::mem::replace(&mut current_backend, "rate-mirrors".to_string()),
                    std::mem::take(&mut current_arg),
                    std::mem::take(&mut current_repos),
                    &mut list,
                );
            }
            current_id = Some(ProviderId::new(*id_str));
            continue;
        }

        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            match key.as_str() {
                "name" => current_name = val.to_string(),
                "mirrorlist" => current_path = PathBuf::from(val),
                "backend" => current_backend = val.to_string(),
                "backend_args" | "backend_arg" => current_arg = val.to_string(),
                "repos" => {
                    current_repos = val
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    if let Some(prev) = current_id {
        flush(
            Some(prev),
            current_name,
            current_path,
            current_backend,
            current_arg,
            current_repos,
            &mut list,
        );
    }

    list
}

/// Save custom providers to user configuration file.
pub fn save_custom_providers(providers: &[CustomProvider]) -> Result<(), String> {
    save_custom_providers_to(&custom_config_path(), providers)
}

pub fn save_custom_providers_to(path: &Path, providers: &[CustomProvider]) -> Result<(), String> {
    for p in providers {
        p.validate()?;
    }

    let mut out = String::new();
    out.push_str("# Archman Custom Mirror Providers Configuration\n");
    out.push_str("# Add or edit custom repository mirror providers here.\n\n");

    for p in providers {
        out.push_str(&format!("[{}]\n", p.id.as_str()));
        out.push_str(&format!("name = {}\n", p.name));
        out.push_str(&format!("mirrorlist = {}\n", p.mirrorlist.display()));
        out.push_str(&format!("backend = {}\n", p.backend_type));
        if !p.backend_arg.is_empty() {
            out.push_str(&format!("backend_args = {}\n", p.backend_arg));
        }
        if !p.repos.is_empty() {
            out.push_str(&format!("repos = {}\n", p.repos.join(",")));
        }
        out.push('\n');
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, out).map_err(|e| format!("Failed to write custom mirrors config: {e}"))
}

/// Add or update a custom provider.
pub fn add_custom_provider(provider: CustomProvider) -> Result<(), String> {
    provider.validate()?;
    let mut list = load_custom_providers();
    list.retain(|p| p.id != provider.id);
    list.push(provider);
    save_custom_providers(&list)
}

/// Remove a custom provider by identifier.
pub fn remove_custom_provider(id: &str) -> Result<bool, String> {
    let mut list = load_custom_providers();
    let initial_len = list.len();
    list.retain(|p| p.id.as_str() != id);
    if list.len() < initial_len {
        save_custom_providers(&list)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_custom_provider_validation() {
        let mut valid = CustomProvider {
            id: ProviderId::new("valid-repo_1"),
            name: "Valid Repo".to_string(),
            mirrorlist: PathBuf::from("/etc/pacman.d/valid-mirrorlist"),
            backend_type: "rate-mirrors".to_string(),
            backend_arg: "valid".to_string(),
            repos: vec!["valid".to_string()],
        };
        assert!(valid.validate().is_ok());

        // Empty ID
        valid.id = ProviderId::new("");
        assert!(valid.validate().is_err());

        // Invalid characters in ID (path traversal or spaces)
        valid.id = ProviderId::new("invalid/id");
        assert!(valid.validate().is_err());
        valid.id = ProviderId::new("invalid id");
        assert!(valid.validate().is_err());

        // Empty name
        valid.id = ProviderId::new("valid-id");
        valid.name = "   ".to_string();
        assert!(valid.validate().is_err());

        // Relative path
        valid.name = "Valid".to_string();
        valid.mirrorlist = PathBuf::from("relative/path");
        assert!(valid.validate().is_err());

        // Invalid backend
        valid.mirrorlist = PathBuf::from("/etc/pacman.d/ml");
        valid.backend_type = "invalid-tool".to_string();
        assert!(valid.validate().is_err());
    }

    #[test]
    fn test_custom_provider_save_load_roundtrip() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("custom_mirrors_{nanos}.conf"));

        let list = vec![
            CustomProvider {
                id: ProviderId::new("my-custom-repo"),
                name: "My Custom Repo".to_string(),
                mirrorlist: PathBuf::from("/etc/pacman.d/my-custom-mirrorlist"),
                backend_type: "rate-mirrors".to_string(),
                backend_arg: "my-custom".to_string(),
                repos: vec!["my-custom".to_string(), "my-custom-testing".to_string()],
            },
            CustomProvider {
                id: ProviderId::new("another-repo"),
                name: "Another Repo".to_string(),
                mirrorlist: PathBuf::from("/etc/pacman.d/another-mirrorlist"),
                backend_type: "reflector".to_string(),
                backend_arg: "France".to_string(),
                repos: vec!["another".to_string()],
            },
        ];

        save_custom_providers_to(&path, &list).unwrap();
        let loaded = load_custom_providers_from(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], list[0]);
        assert_eq!(loaded[1], list[1]);

        let _ = fs::remove_file(&path);
    }
}
