//! Safe archive extraction (PRD §7, NFR-8).
//!
//! The invariant this module exists to uphold:
//!
//! > **Nothing may be written outside the destination directory.**
//!
//! That is enforced three ways: entry names are rejected if they are absolute,
//! contain `..` after separator normalisation, or would resolve outside the
//! target; symlinks are refused outright; and the whole extraction runs into a
//! staging directory that is only moved into place on success, so a failure
//! never leaves a half-populated component directory.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why extraction failed.
#[derive(Debug)]
pub enum UnpackError {
    /// The archive could not be opened or read.
    Io { detail: String },
    /// An entry name tried to escape the destination.
    UnsafePath { entry: String },
    /// An entry type we refuse to materialise (symlink, device, …).
    UnsupportedEntry { entry: String, kind: String },
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnpackError::Io { detail } => write!(f, "cannot read archive: {detail}"),
            UnpackError::UnsafePath { entry } => write!(
                f,
                "refusing to extract `{entry}`: it would write outside the destination directory"
            ),
            UnpackError::UnsupportedEntry { entry, kind } => {
                write!(f, "refusing to extract `{entry}`: {kind} entries are not allowed")
            }
        }
    }
}

impl std::error::Error for UnpackError {}

/// Extract `archive` into `dest`, stripping a single wrapper directory when the
/// archive has exactly one.
///
/// Atomicity: contents are written to `dest.staging`, then the staging directory
/// is renamed onto `dest`. On any error the staging directory is removed, so
/// `dest` is never left partially populated.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<(), UnpackError> {
    let file = fs::File::open(archive).map_err(|e| UnpackError::Io {
        detail: format!("{}: {e}", archive.display()),
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| UnpackError::Io {
        detail: e.to_string(),
    })?;

    // Pass 1: validate every entry before writing anything.
    let entries = plan_entries(&mut zip)?;

    // Pass 2: extract into staging.
    let staging = staging_path(dest);
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging).map_err(|e| UnpackError::Io {
        detail: format!("{}: {e}", staging.display()),
    })?;

    let result = write_entries(&mut zip, &entries, &staging);
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Pass 3: commit. Remove any previous destination first so extraction is
    // idempotent and the rename cannot fail on a non-empty target.
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| UnpackError::Io {
            detail: format!("{}: {e}", dest.display()),
        })?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| UnpackError::Io {
            detail: format!("{}: {e}", parent.display()),
        })?;
    }
    fs::rename(&staging, dest).map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        UnpackError::Io {
            detail: format!("moving staging into place: {e}"),
        }
    })?;

    Ok(())
}

/// One entry, already validated and with any wrapper directory stripped.
struct PlannedEntry {
    /// Index into the zip archive.
    index: usize,
    /// Destination path relative to the staging root.
    rel: PathBuf,
    /// Whether this entry is a directory.
    is_dir: bool,
}

fn staging_path(dest: &Path) -> PathBuf {
    let mut os = dest.as_os_str().to_os_string();
    os.push(".staging");
    PathBuf::from(os)
}

/// Validate all entries and compute their destination-relative paths.
fn plan_entries(zip: &mut zip::ZipArchive<fs::File>) -> Result<Vec<PlannedEntry>, UnpackError> {
    // Collect (index, raw_name, is_dir) first.
    let mut raw: Vec<(usize, String, bool)> = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| UnpackError::Io {
            detail: e.to_string(),
        })?;

        let name = entry.name().to_string();

        // Refuse symlinks and other exotic entry types outright.
        if let Some(mode) = entry.unix_mode() {
            let ft = mode & 0o170000;
            if ft == 0o120000 {
                return Err(UnpackError::UnsupportedEntry {
                    entry: name,
                    kind: "symlink".into(),
                });
            }
        }

        let is_dir = entry.is_dir();
        raw.push((i, name, is_dir));
    }

    // Decide whether to strip a single wrapper directory.
    let strip = single_root_prefix(&raw);

    let mut out = Vec::with_capacity(raw.len());
    for (index, name, is_dir) in raw {
        let rel = normalise_entry(&name, strip.as_deref())?;
        let Some(rel) = rel else {
            // The entry *is* the wrapper directory itself; nothing to write.
            continue;
        };
        out.push(PlannedEntry { index, rel, is_dir });
    }
    Ok(out)
}

/// If every entry lives under one shared top-level directory, return that name.
fn single_root_prefix(raw: &[(usize, String, bool)]) -> Option<String> {
    let mut roots: Vec<String> = Vec::new();
    for (_, name, _) in raw {
        let cleaned = name.replace('\\', "/");
        let first = cleaned.split('/').next().unwrap_or("").to_string();
        if first.is_empty() {
            // A bare top-level file means there is no single wrapper directory.
            return None;
        }
        // A file directly at the top level (no separator) also means no wrapper.
        if !cleaned.contains('/') {
            return None;
        }
        if !roots.contains(&first) {
            roots.push(first);
        }
        if roots.len() > 1 {
            return None;
        }
    }
    if roots.len() == 1 {
        roots.pop()
    } else {
        None
    }
}

/// Normalise an entry name into a safe relative path.
///
/// Returns `None` when the entry is the stripped wrapper directory itself.
fn normalise_entry(name: &str, strip: Option<&str>) -> Result<Option<PathBuf>, UnpackError> {
    let unsafe_entry = || UnpackError::UnsafePath {
        entry: name.to_string(),
    };

    // Unify separators so `a\..\..\b` is caught the same as `a/../../b`.
    let unified = name.replace('\\', "/");

    // Absolute or drive-qualified names are always refused.
    if unified.starts_with('/') || unified.starts_with("//") {
        return Err(unsafe_entry());
    }
    let bytes = unified.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(unsafe_entry());
    }

    // Strip the wrapper directory if present.
    let trimmed = match strip {
        Some(root) => {
            let rest = unified
                .strip_prefix(root)
                .unwrap_or(&unified)
                .trim_start_matches('/');
            if rest.is_empty() {
                return Ok(None); // the wrapper dir itself
            }
            rest.to_string()
        }
        None => unified.clone(),
    };

    // Build a relative path, rejecting anything that could climb out.
    let candidate = Path::new(&trimmed);
    let mut rel = PathBuf::new();
    for comp in candidate.components() {
        match comp {
            Component::Normal(part) => rel.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(unsafe_entry());
            }
        }
    }
    if rel.as_os_str().is_empty() {
        return Ok(None);
    }
    Ok(Some(rel))
}

/// Write the planned entries into `staging`.
fn write_entries(
    zip: &mut zip::ZipArchive<fs::File>,
    entries: &[PlannedEntry],
    staging: &Path,
) -> Result<(), UnpackError> {
    for planned in entries {
        let target = staging.join(&planned.rel);

        // Belt and braces: the joined path must still be inside staging.
        if !is_within(&target, staging) {
            return Err(UnpackError::UnsafePath {
                entry: planned.rel.display().to_string(),
            });
        }

        if planned.is_dir {
            fs::create_dir_all(&target).map_err(|e| UnpackError::Io {
                detail: format!("{}: {e}", target.display()),
            })?;
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| UnpackError::Io {
                detail: format!("{}: {e}", parent.display()),
            })?;
        }

        let mut entry = zip.by_index(planned.index).map_err(|e| UnpackError::Io {
            detail: e.to_string(),
        })?;
        let mut out = fs::File::create(&target).map_err(|e| UnpackError::Io {
            detail: format!("{}: {e}", target.display()),
        })?;
        io::copy(&mut entry, &mut out).map_err(|e| UnpackError::Io {
            detail: format!("{}: {e}", target.display()),
        })?;
    }
    Ok(())
}

/// Whether `path` stays inside `base`.
///
/// Purely lexical: the destination does not exist yet when we check, so
/// `canonicalize` would fail. Both arguments here are already-built from
/// validated relative components, so normalising `..` lexically is sound.
fn is_within(path: &Path, base: &Path) -> bool {
    let p = lexical_normalise(path);
    let b = lexical_normalise(base);
    p.starts_with(&b)
}

/// Resolve `.` and `..` components without touching the filesystem.
fn lexical_normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Prefix(pre) => out.push(pre.as_os_str()),
            Component::RootDir => out.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}
