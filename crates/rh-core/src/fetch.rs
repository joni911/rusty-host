//! Artifact integrity: hashing and verification (PRD §4.4, NFR-7).
//!
//! Every artifact is hashed before it is trusted. When upstream publishes a
//! digest we enforce it; when it does not we record ours and say so plainly
//! rather than implying a verification we did not perform.

use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// Why verification failed.
#[derive(Debug)]
pub enum VerifyError {
    /// Could not read the file.
    Io { path: String, detail: String },
    /// The digest was not 64 hex characters.
    Malformed { digest: String },
    /// Digest mismatch between upstream and downloaded bytes.
    Mismatch {
        expected: String,
        actual: String,
    },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::Io { path, detail } => write!(f, "cannot read {path}: {detail}"),
            VerifyError::Malformed { digest } => write!(
                f,
                "malformed sha256 `{digest}` (expected 64 hex characters)"
            ),
            VerifyError::Mismatch { expected, actual } => write!(
                f,
                "checksum mismatch\n  expected: {expected}\n  actual:   {actual}\n\
                 Refusing to install an unverified artifact."
            ),
        }
    }
}

impl std::error::Error for VerifyError {}

/// SHA-256 of a file, streamed so a large archive does not need to fit in RAM.
pub fn sha256_file(path: &Path) -> Result<String, VerifyError> {
    let f = File::open(path).map_err(|e| VerifyError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    let mut reader = BufReader::with_capacity(64 * 1024, f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| VerifyError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Verify `path` against `expected` (case-insensitive hex).
pub fn verify(path: &Path, expected: &str) -> Result<(), VerifyError> {
    let expected = expected.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(VerifyError::Malformed { digest: expected });
    }
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(VerifyError::Mismatch { expected, actual });
    }
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
