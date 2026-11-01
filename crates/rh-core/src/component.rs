//! Component descriptors — the extensibility core (PRD §4.3).
//!
//! A component is described entirely by a TOML file in `components/`. Adding a
//! new tool requires **no Rust code**: drop the file in, and it appears in the
//! catalogue. This is the structural difference from Laragon, where every tool
//! needed compiled-in menu, checkbox and port wiring.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

/// What role a component plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentKind {
    /// Language runtime (php, node, python)
    Runtime,
    /// Web server (apache, nginx)
    Server,
    /// Database engine (mariadb, postgres)
    Database,
    /// Standalone tool (dbx, mailpit)
    Tool,
}

/// How to discover available versions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "strategy", rename_all = "kebab-case")]
pub enum ResolveStrategy {
    /// JSON document mapping versions to artifacts (php.net, custom indexes).
    JsonIndex {
        /// URL of the JSON index.
        index_url: String,
        /// Field path inside each version entry holding the download URL.
        #[serde(default)]
        url_field: Option<String>,
        /// Base URL prepended to a relative `url_field` value.
        #[serde(default)]
        base_url: Option<String>,
        /// Preferred build variant key (e.g. `ts-vs17-x64`).
        #[serde(default)]
        variant: Option<String>,
    },
    /// An HTML directory listing or download page.
    HtmlScrape {
        /// Page to fetch.
        index_url: String,
        /// Regex whose first capture group is the version.
        file_regex: String,
        /// URL template; `{version}` is substituted (may be the page itself).
        #[serde(default)]
        base_url: Option<String>,
        /// Prefer long-term-support releases when several are present.
        #[serde(default)]
        prefer_lts: bool,
    },
    /// A GitHub release, selecting an asset by name.
    GithubRelease {
        /// `owner/repo`.
        repo: String,
        /// Regex selecting the asset file name.
        asset_regex: String,
    },
    /// A single fixed URL with no version resolution.
    Direct {
        /// Download URL.
        url: String,
    },
}

/// One executable inside the component.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    /// Path to the executable, relative to the component version directory.
    pub executable: String,
    /// Command used to confirm the installed version.
    #[serde(default)]
    pub version_cmd: Option<Vec<String>>,
    /// Regex with capture group 1 = version, applied to the command output.
    #[serde(default)]
    pub version_regex: Option<String>,
}

/// Config generation rules.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfigSpec {
    /// Template file name, resolved from `templates/` then user overrides.
    pub template: String,
    /// Output path relative to the workspace root; `{version}` is substituted.
    pub target: String,
}

/// A complete component descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Descriptor {
    /// Lower-case component name; used as the directory name under `bin/`.
    pub name: String,
    /// Role of the component.
    pub kind: ComponentKind,
    /// Capability names this component satisfies (defaults to `[name]`).
    #[serde(default)]
    pub provides: Vec<String>,
    /// Version discovery rules.
    pub resolve: ResolveStrategy,
    /// Executables provided.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// Optional config generation.
    #[serde(default)]
    pub config: Option<ConfigSpec>,
}

impl Descriptor {
    /// Parse and validate a descriptor from TOML text.
    pub fn from_str(text: &str) -> Result<Self, DescriptorError> {
        let d: Descriptor = toml::from_str(text).map_err(|e| DescriptorError::Parse {
            source: e.to_string(),
        })?;
        d.validate()?;
        Ok(d)
    }

    /// Parse and validate a descriptor from a file, naming it in any error.
    pub fn from_path(path: &Path) -> Result<Self, DescriptorError> {
        let text = fs::read_to_string(path).map_err(|e| DescriptorError::Read {
            path: path.display().to_string(),
            source: e.to_string(),
        })?;
        Descriptor::from_str(&text).map_err(|e| e.with_path(path))
    }

    fn validate(&self) -> Result<(), DescriptorError> {
        if self.name.trim().is_empty() {
            return Err(DescriptorError::Invalid {
                path: None,
                reason: "`name` must not be empty".into(),
            });
        }
        // A name becomes a directory and a URL-ish identifier: keep it boring.
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(DescriptorError::Invalid {
                path: None,
                reason: format!(
                    "`name` must contain only lowercase letters, digits, `-` or `_` (got `{}`)",
                    self.name
                ),
            });
        }
        if self.artifacts.is_empty() {
            return Err(DescriptorError::Invalid {
                path: None,
                reason: "`[[artifacts]]` must declare at least one executable".into(),
            });
        }
        Ok(())
    }

    /// The capability names this component provides.
    pub fn provided(&self) -> Vec<String> {
        if self.provides.is_empty() {
            vec![self.name.clone()]
        } else {
            self.provides.clone()
        }
    }
}

/// Why a descriptor could not be used.
#[derive(Debug)]
pub enum DescriptorError {
    /// Reading the file failed.
    Read { path: String, source: String },
    /// TOML or schema mismatch.
    Parse { source: String },
    /// Structurally valid but semantically wrong.
    Invalid {
        path: Option<String>,
        reason: String,
    },
}

impl DescriptorError {
    fn with_path(mut self, path: &Path) -> Self {
        if let DescriptorError::Invalid { path: p, .. } = &mut self {
            *p = Some(path.display().to_string());
        }
        if let DescriptorError::Parse { source } = &mut self {
            *source = format!("{}: {source}", path.display());
        }
        self
    }
}

impl std::fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorError::Read { path, source } => {
                write!(f, "cannot read descriptor {path}: {source}")
            }
            DescriptorError::Parse { source } => write!(f, "invalid descriptor: {source}"),
            DescriptorError::Invalid { path, reason } => match path {
                Some(p) => write!(f, "invalid descriptor {p}: {reason}"),
                None => write!(f, "invalid descriptor: {reason}"),
            },
        }
    }
}

impl std::error::Error for DescriptorError {}

/// Load every `*.toml` in `dir` into a name-keyed catalogue.
///
/// Duplicate names are rejected: two files claiming the same component would
/// make behaviour depend on directory iteration order.
pub fn load_catalogue(dir: &Path) -> Result<BTreeMap<String, Descriptor>, DescriptorError> {
    let mut out: BTreeMap<String, Descriptor> = BTreeMap::new();

    let entries = fs::read_dir(dir).map_err(|e| DescriptorError::Read {
        path: dir.display().to_string(),
        source: e.to_string(),
    })?;

    // Deterministic order so the first error reported is stable.
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();

    for path in paths {
        let d = Descriptor::from_path(&path)?;
        if let Some(existing) = out.get(&d.name) {
            return Err(DescriptorError::Invalid {
                path: Some(path.display().to_string()),
                reason: format!(
                    "duplicate component name `{}` (already defined by another descriptor, provides {:?})",
                    d.name,
                    existing.provided()
                ),
            });
        }
        out.insert(d.name.clone(), d);
    }
    Ok(out)
}
