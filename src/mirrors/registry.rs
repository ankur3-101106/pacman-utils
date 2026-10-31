// ──────────────────────────────────────────────────────────────────────
// mirrors/registry.rs — Built-in Provider Registry
// ──────────────────────────────────────────────────────────────────────

use super::model::{
    DetectionStatus, MirrorProvider, ProviderCapabilities, ProviderId, RankingBackend,
};

/// Definition for a registered provider template before discovery.
#[derive(Debug, Clone)]
pub struct ProviderDefinition {
    pub id: ProviderId,
    pub name: &'static str,
    pub dist_type: &'static str,
    /// Known repository section names in pacman.conf.
    pub repo_patterns: &'static [&'static str],
    /// Known mirrorlist filenames or patterns in /etc/pacman.d/.
    pub file_patterns: &'static [&'static str],
    /// Default ranking backend for this provider.
    pub default_backend: fn() -> RankingBackend,
    /// Capabilities.
    pub capabilities: ProviderCapabilities,
}

impl ProviderDefinition {
    pub fn instantiate(&self) -> MirrorProvider {
        MirrorProvider {
            id: self.id.clone(),
            name: self.name.to_string(),
            dist_type: self.dist_type.to_string(),
            detection: DetectionStatus::NotConfigured,
            repos: Vec::new(),
            mirrorlists: Vec::new(),
            backend: (self.default_backend)(),
            capabilities: self.capabilities.clone(),
            last_ranked: None,
            is_custom: false,
        }
    }
}

/// The 12 initial built-in providers specified for archman.
pub static BUILTIN_PROVIDERS: &[ProviderDefinition] = &[
    // 1. Arch Linux
    ProviderDefinition {
        id: ProviderId(String::new()), // Initialized via init
        name: "Arch Linux",
        dist_type: "Arch Linux",
        repo_patterns: &[
            "core",
            "extra",
            "multilib",
            "testing",
            "core-testing",
            "extra-testing",
            "multilib-testing",
            "community",
            "community-testing",
        ],
        file_patterns: &["mirrorlist"],
        default_backend: || {
            if crate::sys::has_bin("reflector") {
                RankingBackend::Reflector {
                    country: None,
                    latest: 20,
                    sort: "rate".to_string(),
                }
            } else {
                RankingBackend::RateMirrors {
                    provider: "arch".to_string(),
                    extra_args: Vec::new(),
                }
            }
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 2. CachyOS
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "CachyOS",
        dist_type: "CachyOS",
        repo_patterns: &[
            "cachyos",
            "cachyos-v3",
            "cachyos-v4",
            "cachyos-core-v3",
            "cachyos-extra-v3",
            "cachyos-znver4",
        ],
        file_patterns: &[
            "cachyos-mirrorlist",
            "cachyos-v3-mirrorlist",
            "cachyos-v4-mirrorlist",
        ],
        default_backend: || RankingBackend::RateMirrors {
            provider: "cachyos".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 3. Arch4edu
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Arch4edu",
        dist_type: "Arch4edu Academic",
        repo_patterns: &["arch4edu"],
        file_patterns: &["arch4edu-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "arch4edu".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 4. Arch Linux ARM
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Arch Linux ARM",
        dist_type: "Arch Linux ARM",
        repo_patterns: &["alarm", "archarm", "aur"],
        file_patterns: &["mirrorlist.arm", "archarm-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "archarm".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 5. ArcoLinux
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "ArcoLinux",
        dist_type: "ArcoLinux",
        repo_patterns: &[
            "arcolinux_repo",
            "arcolinux_repo_3party",
            "arcolinux_repo_xlarge",
            "arcolinux",
        ],
        file_patterns: &["arcolinux-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "arcolinux".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 6. Artix Linux
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Artix Linux",
        dist_type: "Artix Linux",
        repo_patterns: &["system", "world", "galaxy", "universe", "lib32"],
        file_patterns: &["artix-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "artix".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 7. BlackArch
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "BlackArch",
        dist_type: "BlackArch Security",
        repo_patterns: &["blackarch"],
        file_patterns: &["blackarch-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "blackarch".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 8. Chaotic-AUR
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Chaotic-AUR",
        dist_type: "Chaotic-AUR Automated",
        repo_patterns: &["chaotic-aur"],
        file_patterns: &["chaotic-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "chaotic-aur".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 9. Arch Linux CN
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Arch Linux CN",
        dist_type: "Arch Linux CN Community",
        repo_patterns: &["archlinuxcn"],
        file_patterns: &["archlinuxcn-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "archlinuxcn".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 10. EndeavourOS
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "EndeavourOS",
        dist_type: "EndeavourOS",
        repo_patterns: &["endeavouros"],
        file_patterns: &["endeavouros-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "endeavouros".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 11. Manjaro
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "Manjaro",
        dist_type: "Manjaro Linux",
        repo_patterns: &["manjaro"],
        file_patterns: &["manjaro-mirrorlist"],
        default_backend: || {
            if crate::sys::has_bin("pacman-mirrors") {
                RankingBackend::Native {
                    executable: "pacman-mirrors".to_string(),
                    args: vec!["-f".to_string(), "5".to_string()],
                }
            } else {
                RankingBackend::RateMirrors {
                    provider: "manjaro".to_string(),
                    extra_args: Vec::new(),
                }
            }
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
    // 12. RebornOS
    ProviderDefinition {
        id: ProviderId(String::new()),
        name: "RebornOS",
        dist_type: "RebornOS",
        repo_patterns: &["reborn-os", "rebornos"],
        file_patterns: &["reborn-mirrorlist", "rebornos-mirrorlist"],
        default_backend: || RankingBackend::RateMirrors {
            provider: "rebornos".to_string(),
            extra_args: Vec::new(),
        },
        capabilities: ProviderCapabilities {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: true,
            supports_protocols: true,
        },
    },
];

/// List of built-in provider definitions with identifiers assigned.
pub fn builtin_registry() -> Vec<ProviderDefinition> {
    let raw = BUILTIN_PROVIDERS;
    let ids = [
        "arch",
        "cachyos",
        "arch4edu",
        "archarm",
        "arcolinux",
        "artix",
        "blackarch",
        "chaotic-aur",
        "archlinuxcn",
        "endeavouros",
        "manjaro",
        "rebornos",
    ];

    raw.iter()
        .zip(ids.iter())
        .map(|(def, id)| {
            let mut d = def.clone();
            d.id = ProviderId::new(*id);
            d
        })
        .collect()
}

/// Find a built-in provider by identifier.
pub fn find_builtin(id: &str) -> Option<ProviderDefinition> {
    builtin_registry().into_iter().find(|p| p.id.as_str() == id)
}

/// Match a repository section name to a built-in provider ID.
pub fn match_repo_section(section: &str) -> Option<ProviderId> {
    let lower = section.to_ascii_lowercase();
    for def in builtin_registry() {
        for pat in def.repo_patterns {
            if lower == pat.to_ascii_lowercase() {
                return Some(def.id.clone());
            }
        }
    }
    None
}

/// Match a mirrorlist filename (e.g. "cachyos-mirrorlist") to a built-in provider ID.
pub fn match_mirrorlist_filename(name: &str) -> Option<ProviderId> {
    let lower = name.to_ascii_lowercase();
    for def in builtin_registry() {
        for pat in def.file_patterns {
            let pat_lower = pat.to_ascii_lowercase();
            if pat_lower == "mirrorlist" {
                if lower == "mirrorlist"
                    || lower.starts_with("mirrorlist.")
                    || lower.starts_with("mirrorlist-")
                {
                    return Some(def.id.clone());
                }
            } else if lower == pat_lower
                || lower.starts_with(&format!("{pat_lower}."))
                || lower.starts_with(&format!("{pat_lower}-"))
                || lower.contains(&pat_lower)
            {
                return Some(def.id.clone());
            }
        }
    }
    None
}
