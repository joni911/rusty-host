//! Workspace settings (`rh.toml`) and the install pipeline.
//!
//! This is the glue that the bindings call. It stays in the library so the CLI
//! and the GUI share one implementation of every operation.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::component::{load_catalogue, Descriptor};
use crate::fetch;
use crate::resolve::{resolve, Release, ResolveError, Upstream};
use crate::unpack::extract_zip;
use crate::workspace::Workspace;

/// Parsed `rh.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Default TLD for generated hostnames.
    #[serde(default = "default_tld")]
    pub tld: String,
    /// Default component versions.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    /// Listening ports.
    #[serde(default)]
    pub ports: BTreeMap<String, u16>,
    /// Services to start with `rh start` and no arguments.
    #[serde(default)]
    pub autostart: Vec<String>,
    /// Hosts-file management.
    #[serde(default)]
    pub hosts: HostsSettings,
}

/// Hosts-file preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostsSettings {
    /// Whether `rh link` may write the system hosts file.
    #[serde(default = "default_true")]
    pub auto: bool,
}

impl Default for HostsSettings {
    fn default() -> Self {
        Self { auto: true }
    }
}

fn default_tld() -> String {
    "test".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tld: default_tld(),
            defaults: BTreeMap::new(),
            ports: BTreeMap::new(),
            autostart: Vec::new(),
            hosts: HostsSettings::default(),
        }
    }
}

impl Settings {
    /// Read `rh.toml`, falling back to defaults when absent.
    pub fn load(ws: &Workspace) -> io::Result<Self> {
        let path = ws.settings_path();
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Write `rh.toml`.
    pub fn save(&self, ws: &Workspace) -> io::Result<()> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = ws.settings_path();
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)
    }

    /// Port for a service name, with a fallback.
    pub fn port(&self, name: &str) -> u16 {
        self.ports
            .get(name)
            .copied()
            .or_else(|| {
                crate::ports::default_ports()
                    .into_iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, p)| p)
            })
            .unwrap_or(0)
    }

    /// The version of `component` this workspace prefers.
    pub fn preferred_version(&self, component: &str, installed: &[String]) -> Option<String> {
        if let Some(want) = self.defaults.get(component) {
            // A default may be a full version or a major.minor prefix.
            if installed.iter().any(|v| v == want) {
                return Some(want.clone());
            }
            let prefix = format!("{want}.");
            if let Some(hit) = installed.iter().find(|v| v.starts_with(&prefix)) {
                return Some(hit.clone());
            }
        }
        installed.last().cloned()
    }
}

/// Where component descriptors come from.
pub fn components_dir() -> PathBuf {
    // Shipped alongside the executable, or the repository copy during development.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("components");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("components"))
        .unwrap_or_else(|| PathBuf::from("components"))
}

/// The templates directory.
pub fn templates_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("templates");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("templates"))
        .unwrap_or_else(|| PathBuf::from("templates"))
}

/// Load the component catalogue shipped with this build.
pub fn catalogue() -> Result<BTreeMap<String, Descriptor>, InstallError> {
    load_catalogue(&components_dir()).map_err(|e| InstallError::Catalogue {
        detail: e.to_string(),
    })
}

/// Why an install failed.
#[derive(Debug)]
pub enum InstallError {
    /// Component name unknown.
    UnknownComponent { name: String, known: Vec<String> },
    /// Descriptor catalogue could not be read.
    Catalogue { detail: String },
    /// Version resolution failed.
    Resolve(ResolveError),
    /// Download or verification failed.
    Fetch { detail: String },
    /// Extraction failed.
    Unpack { detail: String },
    /// Filesystem problem.
    Io { detail: String },
    /// The artifact's own version command disagreed with what we expected.
    VersionMismatch { expected: String, actual: String },
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::UnknownComponent { name, known } => write!(
                f,
                "unknown component `{name}`. Available: {}",
                known.join(", ")
            ),
            InstallError::Catalogue { detail } => write!(f, "cannot read components: {detail}"),
            InstallError::Resolve(e) => write!(f, "{e}"),
            InstallError::Fetch { detail } => write!(f, "download failed: {detail}"),
            InstallError::Unpack { detail } => write!(f, "extraction failed: {detail}"),
            InstallError::Io { detail } => write!(f, "{detail}"),
            InstallError::VersionMismatch { expected, actual } => write!(
                f,
                "installed artifact reports version {actual}, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<ResolveError> for InstallError {
    fn from(e: ResolveError) -> Self {
        InstallError::Resolve(e)
    }
}

/// What an install produced.
#[derive(Debug, Clone)]
pub struct Installed {
    /// Component name.
    pub component: String,
    /// Resolved version.
    pub version: String,
    /// Where it was unpacked.
    pub dir: PathBuf,
    /// Digest of the artifact.
    pub sha256: String,
}

/// Resolve a version without downloading anything.
pub fn plan_install(
    comp: &Descriptor,
    want: Option<&str>,
    upstream: &dyn Upstream,
) -> Result<Release, InstallError> {
    resolve(comp, want, upstream).map_err(InstallError::from)
}

/// Install an already-downloaded archive.
///
/// Kept separate from downloading so the pipeline is testable without a network:
/// pass a local zip and a known version.
pub fn install_archive(
    ws: &Workspace,
    comp: &Descriptor,
    version: &str,
    archive: &Path,
    expected_sha256: Option<&str>,
) -> Result<Installed, InstallError> {
    let digest = fetch::sha256_file(archive).map_err(|e| InstallError::Fetch {
        detail: e.to_string(),
    })?;

    if let Some(expected) = expected_sha256 {
        fetch::verify(archive, expected).map_err(|e| InstallError::Fetch {
            detail: e.to_string(),
        })?;
    }

    let dir = ws.component_dir(&comp.name, version);
    extract_zip(archive, &dir).map_err(|e| InstallError::Unpack {
        detail: e.to_string(),
    })?;

    let mut lock = ws.load_lock().map_err(|e| InstallError::Io {
        detail: e.to_string(),
    })?;
    lock.add(&comp.name, version, &format!("bin/{}/{}", comp.name, version));
    if let Some(entry) = lock
        .components
        .get_mut(&comp.name)
        .and_then(|m| m.get_mut(version))
    {
        entry.sha256 = Some(digest.clone());
        entry.verified = expected_sha256.is_some();
    }
    ws.save_lock(&lock).map_err(|e| InstallError::Io {
        detail: e.to_string(),
    })?;

    Ok(Installed {
        component: comp.name.clone(),
        version: version.to_string(),
        dir,
        sha256: digest,
    })
}

/// Remove an installed version from the lock and delete its directory.
pub fn uninstall(ws: &Workspace, component: &str, version: &str) -> Result<(), InstallError> {
    let mut lock = ws.load_lock().map_err(|e| InstallError::Io {
        detail: e.to_string(),
    })?;

    let Some(versions) = lock.components.get_mut(component) else {
        return Err(InstallError::UnknownComponent {
            name: component.to_string(),
            known: lock.components.keys().cloned().collect(),
        });
    };
    if versions.remove(version).is_none() {
        return Err(InstallError::UnknownComponent {
            name: format!("{component}@{version}"),
            known: versions.keys().cloned().collect(),
        });
    }
    let empty = versions.is_empty();
    if empty {
        lock.components.remove(component);
    }
    ws.save_lock(&lock).map_err(|e| InstallError::Io {
        detail: e.to_string(),
    })?;

    let dir = ws.component_dir(component, version);
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| InstallError::Io {
            detail: format!("{}: {e}", dir.display()),
        })?;
    }
    Ok(())
}
