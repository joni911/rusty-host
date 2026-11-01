//! Project scaffolding recipes (PRD §3.7).
//!
//! The format is deliberately minimal — one `Name=<command or URL>` per line:
//!
//! ```text
//! Blank=
//! WordPress=https://wordpress.org/latest.tar.gz
//! Laravel=composer create-project laravel/laravel %s --prefer-dist
//! ```
//!
//! `%s` is the project name. A URL value is downloaded and unpacked; anything
//! else is executed as a command. That duality is the whole plugin model, and it
//! is why recipes can be extended without touching Rust.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// What a recipe does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Create an empty project directory.
    Blank,
    /// Download and unpack this URL.
    Url(String),
    /// Run this command in the new project directory.
    Command(String),
}

/// One scaffolding recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// Display name, also usable as a selector.
    pub name: String,
    /// What to do.
    pub action: Action,
}

/// Why a recipe could not be used.
#[derive(Debug)]
pub enum RecipeError {
    /// The line had no `=`.
    Malformed { line: String },
    /// The name would escape the projects directory.
    UnsafeName { name: String },
    /// The project directory is already there.
    AlreadyExists { path: String },
    /// Nothing to execute.
    NothingToRun { name: String },
    /// Filesystem problem.
    Io { path: String, detail: String },
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecipeError::Malformed { line } => write!(
                f,
                "invalid recipe `{line}`: expected `Name=<command or URL>`"
            ),
            RecipeError::UnsafeName { name } => write!(
                f,
                "invalid name `{name}`: must not contain path separators or `..`"
            ),
            RecipeError::AlreadyExists { path } => {
                write!(f, "`{path}` already exists; choose another name")
            }
            RecipeError::NothingToRun { name } => {
                write!(f, "recipe `{name}` has nothing to run")
            }
            RecipeError::Io { path, detail } => write!(f, "cannot use {path}: {detail}"),
        }
    }
}

impl std::error::Error for RecipeError {}

impl Recipe {
    /// Parse one `Name=value` line.
    pub fn parse_line(line: &str) -> Result<Self, RecipeError> {
        let Some((name, value)) = line.split_once('=') else {
            return Err(RecipeError::Malformed {
                line: line.trim().to_string(),
            });
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(RecipeError::Malformed {
                line: line.trim().to_string(),
            });
        }
        check_safe_name(name)?;

        let value = value.trim();
        let action = if value.is_empty() {
            Action::Blank
        } else if value.starts_with("http://") || value.starts_with("https://") {
            Action::Url(value.to_string())
        } else {
            Action::Command(value.to_string())
        };

        Ok(Recipe {
            name: name.to_string(),
            action,
        })
    }

    /// The shell command for `project`, with `%s` substituted.
    pub fn build_command(&self, project: &str) -> Result<String, RecipeError> {
        match &self.action {
            Action::Command(cmd) => Ok(cmd.replace("%s", project)),
            Action::Blank | Action::Url(_) => Err(RecipeError::NothingToRun {
                name: self.name.clone(),
            }),
        }
    }

    /// The URL to fetch, when this recipe downloads.
    pub fn url(&self) -> Option<&str> {
        match &self.action {
            Action::Url(u) => Some(u),
            _ => None,
        }
    }
}

/// Parse a whole recipe list, skipping comments and blank lines.
pub fn parse_list(text: &str) -> Vec<Recipe> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| Recipe::parse_line(l).ok())
        .collect()
}

/// Load the built-in recipes shipped with the binary.
pub fn builtin() -> Vec<Recipe> {
    parse_list(include_str!("../../../recipes.toml"))
}

/// Create the project directory for `recipe` under `www`.
///
/// Refuses to overwrite an existing directory: scaffolding over someone's work
/// would be destructive.
pub fn scaffold(
    recipe: &Recipe,
    project: &str,
    www: &Path,
    _extra_args: &[String],
) -> Result<PathBuf, RecipeError> {
    check_safe_name(project)?;

    let dir = www.join(project);
    if dir.exists() {
        return Err(RecipeError::AlreadyExists {
            path: dir.display().to_string(),
        });
    }

    fs::create_dir_all(&dir).map_err(|e| RecipeError::Io {
        path: dir.display().to_string(),
        detail: e.to_string(),
    })?;

    // Command execution belongs to the CLI binding so this module stays free of
    // process spawning.
    let _ = recipe;
    Ok(dir)
}

/// Rewrite Laravel-style database keys in a `.env` file.
///
/// Only `DB_DATABASE`, `DB_USERNAME` and `DB_PASSWORD` are touched. The original
/// spacing style (`KEY=value` vs `KEY = "value"`) is preserved so a hand-formatted
/// file keeps its shape, and absent keys are never invented.
pub fn rewrite_env(
    path: &Path,
    database: &str,
    username: &str,
    password: &str,
) -> Result<(), RecipeError> {
    let Ok(text) = fs::read_to_string(path) else {
        // A project without a .env is normal; nothing to do.
        return Ok(());
    };

    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(text.len());
    let mut changed = false;

    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n').trim_end_matches('\r');

        let replaced = for_key(body, "DB_DATABASE", database)
            .or_else(|| for_key(body, "DB_USERNAME", username))
            .or_else(|| for_key(body, "DB_PASSWORD", password));

        match replaced {
            Some(new_line) => {
                out.push_str(&new_line);
                out.push_str(eol);
                changed = true;
            }
            None => out.push_str(line),
        }
    }

    if changed {
        fs::write(path, out).map_err(|e| RecipeError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
    }
    Ok(())
}

/// Rebuild a `<key>[pad]=[pad]<value>` line with a new value, keeping the
/// original spacing and quoting style.
///
/// Returns `None` when the line is not this key at all, or is a comment.
fn for_key(line: &str, key: &str, value: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let rest = line.strip_prefix(key)?;

    let eq = rest.find('=')?;
    // Reject longer keys that merely start with `key` (e.g. DB_DATABASE_URL).
    let between = &rest[..eq];
    if !between.trim().is_empty() {
        return None;
    }

    let after = &rest[eq + 1..];
    let lead_len = after.len() - after.trim_start().len();
    let lead = &after[..lead_len];
    let after_lead = &after[lead_len..];

    let trailing_len = after_lead.len() - after_lead.trim_end().len();
    let old = &after_lead[..after_lead.len() - trailing_len];
    let trailing = &after_lead[after_lead.len() - trailing_len..];

    let quoted = old.len() >= 2 && old.starts_with('"') && old.ends_with('"');
    let body = if quoted {
        format!("\"{value}\"")
    } else {
        value.to_string()
    };

    Some(format!("{key}{between}={lead}{body}{trailing}"))
}

/// Reject names that could escape the projects directory.
fn check_safe_name(name: &str) -> Result<(), RecipeError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == ".."
        || name == "."
        || name.contains(':')
    {
        return Err(RecipeError::UnsafeName {
            name: name.to_string(),
        });
    }
    Ok(())
}
