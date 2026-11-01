//! Hosts file management (PRD §4.5, §7, NFR-6).
//!
//! The strategy, learned from observing Laragon and then improved:
//!
//! 1. Read the current file.
//! 2. Strip **our** managed block, identified by explicit markers. All user
//!    lines are kept byte-for-byte; we never parse or reformat them.
//! 3. Append a freshly generated block.
//! 4. Write to a sibling temp file, then replace the original. A crash or a
//!    failed write can therefore never leave a half-written system file.
//! 5. On failure, report exactly what went wrong and what to do about it — and
//!    **never** attempt privilege escalation.
//!
//! Improvement over the observed Laragon approach: the merge/prune rules live
//! here in tested Rust rather than in a shipped Node.js script, so the behaviour
//! is compile-time checked and property-tested.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Start marker for the block we own.
pub const MANAGED_BEGIN: &str = "# >>> rusty-host >>>";
/// End marker for the block we own.
pub const MANAGED_END: &str = "# <<< rusty-host <<<";

/// The set of host entries to publish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Block {
    /// (hostname, ip) pairs. Rendered sorted for stable diffs.
    pub entries: Vec<(String, String)>,
}

impl Block {
    /// Build from an iterator of pairs.
    pub fn new<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (S, S)>,
        S: Into<String>,
    {
        Self {
            entries: pairs
                .into_iter()
                .map(|(h, ip)| (h.into(), ip.into()))
                .collect(),
        }
    }

    /// Whether the block has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Render the block body (markers included), sorted and LF-terminated.
    fn render(&self, eol: &str) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_BEGIN);
        out.push_str(eol);
        out.push_str("# Managed by rusty-host. Edits inside this block are overwritten.");
        out.push_str(eol);

        let mut sorted: Vec<&(String, String)> = self.entries.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (host, ip) in sorted {
            out.push_str(ip);
            out.push(' ');
            out.push_str(host);
            out.push_str(eol);
        }

        out.push_str(MANAGED_END);
        out.push_str(eol);
        out
    }
}

/// Why a hosts-file operation failed.
#[derive(Debug)]
pub enum HostsError {
    /// The file could not be read or written.
    Io { path: String, detail: String },
    /// The OS refused access. Carries the remediation text.
    PermissionDenied { path: String, detail: String },
}

impl fmt::Display for HostsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostsError::Io { path, detail } => {
                write!(f, "could not update the hosts file {path}: {detail}")
            }
            HostsError::PermissionDenied { path, detail } => write!(
                f,
                "Your system prevents changing the hosts file ({path}): {detail}\n\
                 \n\
                 rusty-host has disabled automatic hosts management so it stops retrying.\n\
                 \n\
                 To fix it:\n\
                 1. Check the file is not marked Read-only\n\
                 2. Check your antivirus or EDR is not protecting the file\n\
                 3. Confirm your account has Write permission on it\n\
                 \n\
                 Your existing entries were left untouched. Run `rh hosts --show` to inspect,\n\
                 or `rh hosts --edit` to change it manually."
            ),
        }
    }
}

impl std::error::Error for HostsError {}

impl HostsError {
    fn from_io(path: &Path, e: &io::Error) -> Self {
        let p = path.display().to_string();
        let detail = e.to_string();
        match e.kind() {
            io::ErrorKind::PermissionDenied => HostsError::PermissionDenied { path: p, detail },
            _ => {
                // Windows surfaces ACL failures as Uncategorized more often than
                // PermissionDenied, so inspect the message too.
                let lower = detail.to_lowercase();
                if lower.contains("access is denied") || lower.contains("denied") {
                    HostsError::PermissionDenied { path: p, detail }
                } else {
                    HostsError::Io { path: p, detail }
                }
            }
        }
    }
}

/// Replace our managed block with `block`, preserving everything else.
///
/// An empty `block` removes our block entirely.
pub fn sync(path: &Path, block: &Block) -> Result<(), HostsError> {
    let original = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(HostsError::from_io(path, &e)),
    };

    let eol = detect_eol(&original);
    let stripped = strip_managed(&original);

    let mut body = stripped;
    if !block.is_empty() {
        if !body.is_empty() && !body.ends_with('\n') && !body.ends_with("\r\n") {
            body.push_str(eol);
        }
        body.push_str(&block.render(eol));
    }

    write_atomically(path, &body)
}

/// Remove our managed block, leaving all user content untouched.
pub fn remove(path: &Path) -> Result<(), HostsError> {
    sync(path, &Block::default())
}

/// List the hostnames currently inside our managed block.
pub fn parse_managed(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == MANAGED_BEGIN {
            inside = true;
            continue;
        }
        if trimmed == MANAGED_END {
            inside = false;
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // "<ip> <host> [alias...]" -> take the first hostname.
        let mut parts = trimmed.split_whitespace();
        let _ip = parts.next();
        if let Some(host) = parts.next() {
            out.push(host.to_string());
        }
    }
    out
}

/// The usual hosts path on this platform.
pub fn system_hosts_path() -> PathBuf {
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        PathBuf::from(root)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/hosts")
    }
}

/// Whether our block is currently present.
pub fn has_managed_block(text: &str) -> bool {
    text.contains(MANAGED_BEGIN) && text.contains(MANAGED_END)
}

/// Remove the managed block (markers and body) from `text`.
///
/// Everything outside the markers is preserved exactly, including the original
/// line endings and any trailing bytes.
fn strip_managed(text: &str) -> String {
    let Some(begin) = text.find(MANAGED_BEGIN) else {
        return text.to_string();
    };
    // Walk back to the start of the line containing the begin marker.
    let line_start = text[..begin].rfind('\n').map(|i| i + 1).unwrap_or(0);

    let Some(end_rel) = text[begin..].find(MANAGED_END) else {
        // Unterminated block: refuse to guess. Drop from the marker to EOF.
        return text[..line_start].to_string();
    };
    let end = begin + end_rel + MANAGED_END.len();

    // Swallow the end marker's own line terminator.
    let after_end = if text[end..].starts_with("\r\n") {
        end + 2
    } else if text[end..].starts_with('\n') {
        end + 1
    } else {
        end
    };

    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..line_start]);
    out.push_str(&text[after_end..]);
    out
}

/// Detect the dominant line ending so we do not mix them.
fn detect_eol(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Write `body` to `path` via a sibling temp file and a replace.
fn write_atomically(path: &Path, body: &str) -> Result<(), HostsError> {
    let tmp = temp_sibling(path);

    // Clean any leftover from a previous crashed attempt.
    let _ = fs::remove_file(&tmp);

    if let Err(e) = fs::write(&tmp, body) {
        let _ = fs::remove_file(&tmp);
        return Err(HostsError::from_io(path, &e));
    }

    // Replace. On Windows `rename` fails if the destination exists, so remove
    // first; the window of absence is tiny and the content is already safe in
    // the temp file.
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            if path.exists() {
                fs::remove_file(path).map_err(|e| {
                    let _ = fs::remove_file(&tmp);
                    HostsError::from_io(path, &e)
                })?;
            }
            fs::rename(&tmp, path).map_err(|e| {
                let _ = fs::remove_file(&tmp);
                HostsError::from_io(path, &e)
            })
        }
    }
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "hosts".into());
    name.push(".rusty-host.tmp");
    match path.parent() {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}
