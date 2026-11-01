//! `rh` — Rusty Host command line interface.
//!
//! Thin presentation layer over `rh-core` (PRD §4.1). All logic — and all
//! error classification — lives in the library so the GUI and terminal can
//! reuse it.

use std::path::PathBuf;
use std::process::ExitCode;

use rh_core::workspace::{DiscoverError, Workspace};

/// Exit codes, per PRD §5. Single source of truth, mirrored by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Ok = 0,
    User = 1,
    Network = 2,
    Permission = 3,
    PortConflict = 4,
    Internal = 5,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::from(Exit::Ok as u8),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.code as u8)
        }
    }
}

/// A CLI error with an exit classification.
struct CliError {
    message: String,
    code: Exit,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::PermissionDenied => Exit::Permission,
            _ => Exit::Internal,
        };
        CliError {
            message: e.to_string(),
            code,
        }
    }
}

fn user_err(msg: impl Into<String>) -> CliError {
    CliError {
        message: msg.into(),
        code: Exit::User,
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    // Global flag may appear before or after the command.
    let (root, rest) = extract_root(args)?;

    let Some(cmd) = rest.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match cmd {
        "init" => cmd_init(root),
        "doctor" => cmd_doctor(root),
        "list" => cmd_list(root),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("rh {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(user_err(format!(
            "unknown command `{other}`\n\nRun `rh --help` for usage."
        ))),
    }
}

fn extract_root(args: &[String]) -> Result<(Option<PathBuf>, Vec<String>), CliError> {
    let mut root = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--root" {
            let Some(v) = args.get(i + 1) else {
                return Err(user_err("--root requires a directory"));
            };
            root = Some(PathBuf::from(v));
            i += 2;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok((root, rest))
}

/// Resolve the workspace: explicit `--root`, else nearest ancestor, else cwd.
fn resolve(root: Option<PathBuf>) -> Workspace {
    if let Some(r) = root {
        return Workspace::at(r);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match Workspace::discover(&cwd) {
        Ok(ws) => ws,
        Err(DiscoverError::NotFound { .. }) => Workspace::at(cwd),
    }
}

fn cmd_init(root: Option<PathBuf>) -> Result<(), CliError> {
    let ws = resolve(root);
    ws.init()?;
    println!("initialised workspace at {}", ws.root().display());
    println!("  bin/   {}", ws.bin().display());
    println!("  conf/  {}", ws.conf().display());
    println!("  data/  {}", ws.data().display());
    println!("  tmp/   {}", ws.tmp().display());
    println!("  www/   {}", ws.www().display());
    Ok(())
}

fn cmd_list(root: Option<PathBuf>) -> Result<(), CliError> {
    let ws = resolve(root);
    let lock = ws.load_lock()?;
    if lock.components.is_empty() {
        println!("no components installed");
        return Ok(());
    }
    for (name, versions) in &lock.components {
        let vs: Vec<String> = versions.keys().cloned().collect();
        println!("{name:<12} {}", vs.join(", "));
    }
    Ok(())
}

fn cmd_doctor(root: Option<PathBuf>) -> Result<(), CliError> {
    let ws = resolve(root);
    println!("workspace   {}", ws.root().display());

    let mut problems = 0;
    for (label, dir) in [
        ("bin", ws.bin()),
        ("conf", ws.conf()),
        ("data", ws.data()),
        ("tmp", ws.tmp()),
        ("www", ws.www()),
    ] {
        let ok = dir.is_dir();
        println!("  {label:<6} {}", if ok { "ok" } else { "MISSING" });
        if !ok {
            problems += 1;
        }
    }

    let settings_ok = ws.settings_path().is_file();
    println!(
        "  rh.toml {}",
        if settings_ok { "ok" } else { "MISSING" }
    );
    if !settings_ok {
        problems += 1;
    }

    match ws.load_lock() {
        Ok(lock) => {
            let n: usize = lock.components.values().map(|m| m.len()).sum();
            println!("  lock   ok ({n} component versions)");
        }
        Err(e) => {
            println!("  lock   INVALID ({e})");
            problems += 1;
        }
    }

    if problems == 0 {
        println!("\nhealthy");
        Ok(())
    } else {
        Err(CliError {
            message: format!("{problems} problem(s) found; run `rh init` to repair"),
            code: Exit::User,
        })
    }
}

fn print_help() {
    println!(
        "rh {} — Rusty Host

Usage: rh <command> [--root <dir>]

Commands:
  init      Create a workspace (bin/ conf/ data/ tmp/ www/)
  list      List installed components and versions
  doctor    Check workspace health
  help      Show this message

Options:
  --root <dir>   Operate on a specific workspace
  -h, --help     Show help
  -V, --version  Show version",
        env!("CARGO_PKG_VERSION")
    );
}
