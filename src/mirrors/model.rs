// ──────────────────────────────────────────────────────────────────────
// mirrors/model.rs — Provider-Aware Mirror Management Data Models
// ──────────────────────────────────────────────────────────────────────

use std::fmt;
use std::path::{Path, PathBuf};

/// Unique, stable identifier for a mirror provider (e.g. "arch", "cachyos", "chaotic-aur").
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::ops::Deref for ProviderId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Status of provider detection in pacman configuration and system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionStatus {
    /// Repository is enabled in pacman.conf and all mirrorlist files exist.
    Detected,
    /// Provider repository is not configured in pacman.conf.
    NotConfigured,
    /// Partially configured: repo in pacman.conf but incomplete, or mirrorlist present without active repo.
    PartiallyConfigured(String),
    /// Configured repository includes a mirrorlist path that does not exist on disk.
    MissingMirrorlist(PathBuf),
    /// Configured repository and mirrorlist exist, but the required ranking tool is not installed.
    BackendUnavailable(String),
    /// Provider does not have a supported ranking backend.
    Unsupported(String),
}

impl DetectionStatus {
    pub fn is_detected(&self) -> bool {
        matches!(self, DetectionStatus::Detected)
    }

    pub fn label(&self) -> &'static str {
        match self {
            DetectionStatus::Detected => "Detected",
            DetectionStatus::NotConfigured => "Not Configured",
            DetectionStatus::PartiallyConfigured(_) => "Partially Configured",
            DetectionStatus::MissingMirrorlist(_) => "Missing Mirrorlist",
            DetectionStatus::BackendUnavailable(_) => "Backend Unavailable",
            DetectionStatus::Unsupported(_) => "Unsupported",
        }
    }
}

/// Ranking backend implementation and arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RankingBackend {
    /// Arch Linux Reflector backend.
    Reflector {
        country: Option<String>,
        latest: usize,
        sort: String,
    },
    /// rate-mirrors backend for supported distributions.
    RateMirrors {
        provider: String,
        extra_args: Vec<String>,
    },
    /// Distribution-specific native ranking tool (e.g. `pacman-mirrors` on Manjaro).
    Native {
        executable: String,
        args: Vec<String>,
    },
    /// No automated ranking backend available.
    Unsupported,
}

impl RankingBackend {
    pub fn name(&self) -> &'static str {
        match self {
            RankingBackend::Reflector { .. } => "Reflector",
            RankingBackend::RateMirrors { .. } => "rate-mirrors",
            RankingBackend::Native { .. } => "Native Tool",
            RankingBackend::Unsupported => "Unsupported",
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, RankingBackend::Unsupported)
    }
}

/// Capabilities exposed by a mirror provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub can_rank: bool,
    pub can_benchmark: bool,
    pub supports_country_filter: bool,
    pub supports_protocols: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            can_rank: true,
            can_benchmark: true,
            supports_country_filter: false,
            supports_protocols: true,
        }
    }
}

/// A single mirror URL entry inside a mirrorlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorEntry {
    pub url: String,
    pub active: bool,
    pub comment: Option<String>,
}

/// A resolved target mirrorlist file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorListTarget {
    pub path: PathBuf,
    pub active_mirrors: Vec<MirrorEntry>,
    pub commented_mirrors: Vec<MirrorEntry>,
    pub exists: bool,
    pub associated_repos: Vec<String>,
}

impl MirrorListTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            active_mirrors: Vec::new(),
            commented_mirrors: Vec::new(),
            exists: false,
            associated_repos: Vec::new(),
        }
    }

    pub fn filename(&self) -> String {
        self.path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn total_active(&self) -> usize {
        self.active_mirrors.len()
    }

    pub fn total_commented(&self) -> usize {
        self.commented_mirrors.len()
    }
}

/// Metadata and status for an individual mirror provider.
#[derive(Debug, Clone)]
pub struct MirrorProvider {
    pub id: ProviderId,
    pub name: String,
    pub dist_type: String,
    pub detection: DetectionStatus,
    pub repos: Vec<String>,
    pub mirrorlists: Vec<MirrorListTarget>,
    pub backend: RankingBackend,
    pub capabilities: ProviderCapabilities,
    pub last_ranked: Option<String>,
    pub is_custom: bool,
}

impl MirrorProvider {
    pub fn total_active_mirrors(&self) -> usize {
        self.mirrorlists.iter().map(|m| m.total_active()).sum()
    }

    pub fn total_targets(&self) -> usize {
        self.mirrorlists.len()
    }

    pub fn is_supported(&self) -> bool {
        self.backend.is_supported()
    }

    pub fn primary_mirrorlist(&self) -> Option<&Path> {
        self.mirrorlists.first().map(|m| m.path.as_path())
    }
}

/// Benchmark measurement result for an individual mirror.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkResult {
    pub url: String,
    pub reachable: bool,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u64>,
    pub throughput_kbps: Option<f64>,
    pub error: Option<String>,
}

/// Metadata for a recorded mirrorlist backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    pub mirrorlist_path: PathBuf,
    pub backup_path: PathBuf,
    pub timestamp: String,
    pub mirror_count: usize,
    pub size_bytes: u64,
}

impl BackupEntry {
    pub fn filename(&self) -> String {
        self.backup_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.backup_path.display().to_string())
    }
}
