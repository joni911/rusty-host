//! TDD-12 (partial) — CLI surface integration tests.
//!
//! Refs: PRD §5. Uses the built binary via `CARGO_BIN_EXE_rh`.

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::TempDir;

fn rh() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rh"))
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let out = rh().args(args).output().expect("spawn rh");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

fn run_root(root: &Path, args: &[&str]) -> Run {
    let mut all = vec!["--root", root.to_str().unwrap()];
    all.extend_from_slice(args);
    run(&all)
}

#[test]
fn init_creates_workspace() {
    let d = TempDir::new("cli-init");
    let r = run_root(d.path(), &["init"]);
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert!(d.path().join("rh.toml").is_file());
    assert!(d.path().join("components.lock.json").is_file());
    assert!(d.path().join("www").is_dir());
}

#[test]
fn init_is_idempotent() {
    let d = TempDir::new("cli-init2");
    assert_eq!(run_root(d.path(), &["init"]).code, 0);
    let r = run_root(d.path(), &["init"]);
    assert_eq!(r.code, 0, "second init must succeed: {}", r.stderr);
}

#[test]
fn list_on_fresh_workspace_reports_empty() {
    let d = TempDir::new("cli-list");
    run_root(d.path(), &["init"]);
    let r = run_root(d.path(), &["list"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("no components installed"), "{}", r.stdout);
}

#[test]
fn doctor_reports_healthy_after_init() {
    let d = TempDir::new("cli-doc");
    run_root(d.path(), &["init"]);
    let r = run_root(d.path(), &["doctor"]);
    assert_eq!(r.code, 0, "stdout: {}", r.stdout);
    assert!(r.stdout.contains("healthy"), "{}", r.stdout);
}

#[test]
fn doctor_fails_on_empty_directory() {
    let d = TempDir::new("cli-doc2");
    let r = run_root(d.path(), &["doctor"]);
    assert_ne!(r.code, 0, "doctor must fail on an uninitialised dir");
    assert!(
        r.stderr.contains("problem"),
        "expected problem report, got: {}",
        r.stderr
    );
}

#[test]
fn unknown_command_exits_1() {
    let r = run(&["definitely-not-a-command"]);
    assert_eq!(r.code, 1, "stderr: {}", r.stderr);
}

#[test]
fn errors_go_to_stderr_not_stdout() {
    let r = run(&["nope"]);
    assert!(
        r.stdout.is_empty(),
        "stdout must stay clean on error, got: {}",
        r.stdout
    );
    assert!(!r.stderr.is_empty(), "error must be on stderr");
}

#[test]
fn version_flag_prints_semver() {
    let r = run(&["--version"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("rh "), "{}", r.stdout);
}

#[test]
fn help_is_helpful() {
    let r = run(&["--help"]);
    assert_eq!(r.code, 0);
    for expected in ["init", "list", "doctor"] {
        assert!(r.stdout.contains(expected), "help missing `{expected}`");
    }
}

#[test]
fn root_flag_works_before_and_after_command() {
    let d = TempDir::new("cli-root");
    let root = d.path().to_str().unwrap().to_string();

    // before
    let a = run(&["--root", &root, "init"]);
    assert_eq!(a.code, 0);
    // after
    let b = run(&["init", "--root", &root]);
    assert_eq!(b.code, 0);

    let p: PathBuf = d.path().to_path_buf();
    assert!(p.join("rh.toml").is_file());
}

#[test]
fn discover_works_from_subdirectory() {
    let d = TempDir::new("cli-sub");
    run_root(d.path(), &["init"]);
    let sub = d.path().join("www").join("proj");
    std::fs::create_dir_all(&sub).unwrap();

    // no --root: must discover the workspace by walking up
    let out = rh()
        .current_dir(&sub)
        .arg("list")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
}
