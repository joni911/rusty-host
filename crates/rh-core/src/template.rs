//! Template rendering (PRD §3.1 item 6, §4.3).
//!
//! Templates are `<<TOKEN>>` fragments. Rendering is substitution only — no
//! conditionals, no loops, no logic. Control flow stays in Rust where it can be
//! tested.
//!
//! Two rules matter for safety:
//!
//! 1. **Values are never re-expanded.** A value containing `<<X>>` is emitted
//!    literally, so a directory name or hostname cannot inject tokens.
//! 2. **Path values are normalised to forward slashes.** Apache, Nginx and PHP
//!    all accept `/` on Windows; `\` is an escape character in some config
//!    contexts, so emitting it would be a bug waiting to happen.
//!
//! Unknown tokens are left verbatim so a broken template is *visible* rather
//! than silently producing wrong config.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// Why rendering failed.
#[derive(Debug)]
pub enum RenderError {
    /// A `<<` was not closed by `>>`.
    MalformedToken { at: usize },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::MalformedToken { at } => write!(
                f,
                "unterminated template token starting at byte {at} (expected `>>`)"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Substitute every `<<NAME>>` in `template` using `vars`.
pub fn render(template: &str, vars: &BTreeMap<String, String>) -> Result<String, RenderError> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;

    while i < bytes.len() {
        // Look for the opening `<<`.
        if i + 1 < bytes.len() && bytes[i] == b'<' && bytes[i + 1] == b'<' {
            match find_close(template, i + 2) {
                Some(close) => {
                    let name = &template[i + 2..close];
                    match vars.get(name) {
                        Some(value) => out.push_str(&normalise_value(value)),
                        None => {
                            // Unknown token: emit verbatim so it is noticeable.
                            out.push_str(&template[i..close + 2]);
                        }
                    }
                    i = close + 2;
                }
                None => return Err(RenderError::MalformedToken { at: i }),
            }
        } else {
            // Copy one whole UTF-8 character so multibyte text survives.
            let ch_len = utf8_len(bytes[i]);
            out.push_str(&template[i..(i + ch_len).min(bytes.len())]);
            i += ch_len;
        }
    }

    Ok(out)
}

/// Convenience: render a template read from disk.
pub fn render_file(
    path: &Path,
    vars: &BTreeMap<String, String>,
) -> std::io::Result<Result<String, RenderError>> {
    let text = std::fs::read_to_string(path)?;
    Ok(render(&text, vars))
}

/// Find the byte offset of the `>>` that closes a token opened just before `from`.
fn find_close(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b'>' && bytes[i + 1] == b'>' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Normalise a substituted value: Windows path separators become forward
/// slashes. Nothing else is altered.
fn normalise_value(value: &str) -> String {
    value.replace('\\', "/")
}
