//! Project discovery, slugging and document-root detection (PRD §3.2).
//!
//! One directory under `www/` is one project and becomes one hostname. That
//! convention is the whole reason `rh link` needs no configuration.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum length of a single DNS label.
const MAX_LABEL: usize = 63;

/// Directory names that indicate a framework's public web root, in priority
/// order. `public` wins because Laravel/Symfony are the common case.
const DOCROOT_CANDIDATES: [&str; 4] = ["public", "public_html", "web", "htdocs"];

/// A discovered project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Directory name as it appears on disk.
    pub name: String,
    /// DNS-safe hostname label.
    pub slug: String,
    /// Fully-qualified hostname, e.g. `demo.test`.
    pub hostname: String,
    /// Absolute path to the project directory.
    pub path: PathBuf,
    /// Directory the web server should serve.
    pub document_root: PathBuf,
}

impl Project {
    /// The URL this project is served at.
    pub fn url(&self, scheme: &str) -> String {
        format!("{scheme}://{}", self.hostname)
    }
}

/// Why scanning failed.
#[derive(Debug)]
pub enum ScanError {
    /// The directory could not be read.
    Io { path: String, detail: String },
    /// Two projects map to the same hostname.
    SlugConflict {
        slug: String,
        first: String,
        second: String,
    },
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::Io { path, detail } => write!(f, "cannot read {path}: {detail}"),
            ScanError::SlugConflict {
                slug,
                first,
                second,
            } => write!(
                f,
                "two projects map to the hostname `{slug}.`: `{first}` and `{second}`.\n\
                 Rename one of them, or add a `.rh.toml` pinning a different slug."
            ),
        }
    }
}

impl std::error::Error for ScanError {}

/// Every directory under `www`, sorted by name for deterministic output.
///
/// Hidden directories (leading `.`) and plain files are ignored. A missing
/// `www` is treated as "no projects" rather than an error — a fresh workspace
/// is not a broken one.
pub fn scan(www: &Path, tld: &str) -> Result<Vec<Project>, ScanError> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();

    if !www.is_dir() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(www).map_err(|e| ScanError::Io {
        path: www.display().to_string(),
        detail: e.to_string(),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| ScanError::Io {
            path: www.display().to_string(),
            detail: e.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|e| ScanError::Io {
            path: entry.path().display().to_string(),
            detail: e.to_string(),
        })?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        found.push((name, entry.path()));
    }

    found.sort_by_key(|(name, _)| name.to_lowercase());

    let mut projects: Vec<Project> = Vec::with_capacity(found.len());
    let mut seen: Vec<(String, String)> = Vec::new();

    for (name, path) in found {
        let slug = slugify(&name);
        if let Some((_, other)) = seen.iter().find(|(s, _)| *s == slug) {
            return Err(ScanError::SlugConflict {
                slug,
                first: other.clone(),
                second: name,
            });
        }
        seen.push((slug.clone(), name.clone()));

        projects.push(Project {
            document_root: document_root(&path),
            hostname: format!("{slug}.{tld}"),
            slug,
            name,
            path,
        });
    }

    Ok(projects)
}

/// Turn a directory name into a DNS-safe label.
///
/// Lowercase, ASCII alphanumerics and single dashes, no leading/trailing dash,
/// at most 63 characters. Never empty.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_dash = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
            if out.len() >= MAX_LABEL {
                break;
            }
        } else if !out.is_empty() {
            // Any run of non-alphanumerics collapses to one dash, and a dash
            // never lands at the start.
            pending_dash = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        "app".to_string()
    } else {
        out
    }
}

/// Which subdirectory of `project` the web server should serve.
pub fn document_root(project: &Path) -> PathBuf {
    for candidate in DOCROOT_CANDIDATES {
        let p = project.join(candidate);
        if p.is_dir() {
            return p;
        }
    }
    project.to_path_buf()
}
