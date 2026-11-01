//! Workspace layout, discovery and the installed-component lock file.
//!
//! Layout (PRD §4.2) — deliberately relative-path-only so the whole tree can be
//! moved or copied anywhere and keep working (NFR-9):
//!
//! ```text
//! <root>/
//!   rh.toml               settings (relative paths only)
//!   components.lock.json  installed versions + digests
//!   bin/                  components, one directory per version
//!   conf/                 generated config
//!   data/                 mutable state (database datadir, logs, run/)
//!   tmp/                  scratch (downloads, hosts.tmp)
//!   www/                  document root — one directory per project
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Marker file that identifies a workspace root.
pub const LOCK_MARKER: &str = "components.lock.json";
/// Settings file name.
pub const SETTINGS_FILE: &str = "rh.toml";

/// Why discovery failed.
#[derive(Debug)]
pub enum DiscoverError {
    /// No ancestor directory contained a workspace marker.
    NotFound { start: PathBuf },
}

impl fmt::Display for DiscoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoverError::NotFound { start } => write!(
                f,
                "no rusty-host workspace found (looked for `{LOCK_MARKER}` upward from {})",
                start.display()
            ),
        }
    }
}

impl std::error::Error for DiscoverError {}

/// A rusty-host workspace rooted at a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Create a handle for `root` without touching the filesystem.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn conf(&self) -> PathBuf {
        self.root.join("conf")
    }
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
    pub fn www(&self) -> PathBuf {
        self.root.join("www")
    }
    pub fn settings_path(&self) -> PathBuf {
        self.root.join(SETTINGS_FILE)
    }
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_MARKER)
    }

    /// Where one component version lives on disk.
    pub fn component_dir(&self, component: &str, version: &str) -> PathBuf {
        self.bin().join(component).join(version)
    }

    /// Find the nearest ancestor containing the workspace marker.
    pub fn discover(start: impl AsRef<Path>) -> Result<Self, DiscoverError> {
        let start = start.as_ref();
        let mut cur: Option<&Path> = Some(start);
        while let Some(dir) = cur {
            if dir.join(LOCK_MARKER).is_file() {
                return Ok(Self::at(dir));
            }
            cur = dir.parent();
        }
        Err(DiscoverError::NotFound {
            start: start.to_path_buf(),
        })
    }

    /// Create the directory skeleton and default files. Idempotent: an existing
    /// `rh.toml` is never overwritten (NFR-11).
    pub fn init(&self) -> io::Result<()> {
        for dir in [self.bin(), self.conf(), self.data(), self.tmp(), self.www()] {
            fs::create_dir_all(&dir)?;
        }

        let settings = self.settings_path();
        if !settings.exists() {
            fs::write(&settings, DEFAULT_SETTINGS)?;
        }

        let lock = self.lock_path();
        if !lock.exists() {
            self.save_lock(&ComponentLock::default())?;
        }
        Ok(())
    }

    /// Load the installed-component lock file. Missing file yields an empty lock.
    pub fn load_lock(&self) -> io::Result<ComponentLock> {
        let path = self.lock_path();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ComponentLock::default()),
            Err(e) => Err(e),
        }
    }

    /// Write the lock file atomically (temp + rename) so a crash cannot leave a
    /// half-written index.
    pub fn save_lock(&self, lock: &ComponentLock) -> io::Result<()> {
        let path = self.lock_path();
        let body = serde_json::to_string_pretty(lock)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)
    }
}

/// Default settings written on `init`. Contains no absolute paths.
const DEFAULT_SETTINGS: &str = r#"# rusty-host settings
# All paths are relative to this directory. Never put absolute paths here:
# the workspace is designed to be moved or copied anywhere (see PRD NFR-9).

tld = "test"

[defaults]
# Component versions used when a project does not pin its own.
# php = "8.4"
# server = "apache"
# database = "mariadb"

[ports]
http = 80
https = 443
mysql = 3306
phpmyadmin = 8080

[boot]
autostart = []

[hosts]
# Set to false automatically when the hosts file cannot be written.
auto = true
"#;

/// The installed-component index (`components.lock.json`).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentLock {
    /// component -> (version -> path relative to the workspace root)
    #[serde(default)]
    pub components: BTreeMap<String, BTreeMap<String, LockedComponent>>,
}

/// One installed component version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedComponent {
    /// Path relative to the workspace root, always with forward slashes.
    pub path: String,
    /// SHA-256 of the artifact we installed from, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Whether that digest was verified against an upstream-published value.
    #[serde(default)]
    pub verified: bool,
}

impl ComponentLock {
    /// Record an installed version.
    pub fn add(&mut self, component: &str, version: &str, rel_path: &str) {
        self.components
            .entry(component.to_string())
            .or_default()
            .insert(
                version.to_string(),
                LockedComponent {
                    path: rel_path.replace('\\', "/"),
                    sha256: None,
                    verified: false,
                },
            );
    }

    /// Versions of `component`, sorted ascending.
    pub fn versions_of(&self, component: &str) -> Vec<String> {
        self.components
            .get(component)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether any version of `component` is installed.
    pub fn has(&self, component: &str) -> bool {
        self.components
            .get(component)
            .is_some_and(|m| !m.is_empty())
    }
}
