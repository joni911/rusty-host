//! TDD-12 — CLI surface integration tests.
//!
//! Refs: PRD §5. Every test gets a fresh workspace.

mod common;
use common::TempDir;

use std::fs;
use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rh"))
        .args(args)
        .output()
        .expect("spawn rh");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn run_root(root: &Path, args: &[&str]) -> (i32, String, String) {
    let mut all = vec!["--root", root.to_str().unwrap()];
    all.extend_from_slice(args);
    run(&all)
}

fn ws(label: &str) -> TempDir {
    let d = TempDir::new(label);
    let (code, _, err) = run_root(d.path(), &["init"]);
    assert_eq!(code, 0, "init failed: {err}");
    d
}

// -------------------------------------------------------------- basics

#[test]
fn init_creates_workspace() {
    let d = TempDir::new("c-init");
    let (code, _, err) = run_root(d.path(), &["init"]);
    assert_eq!(code, 0, "{err}");
    assert!(d.path().join("rh.toml").is_file());
    assert!(d.path().join("components.lock.json").is_file());
    assert!(d.path().join("www").is_dir());
}

#[test]
fn list_shows_available_components_when_empty() {
    let d = ws("c-list");
    let (code, out, _) = run_root(d.path(), &["list"]);
    assert_eq!(code, 0);
    assert!(out.contains("no components installed"), "{out}");
    assert!(out.contains("available:"), "must name what can be installed: {out}");
    for expected in ["php", "mariadb", "apache", "phpmyadmin", "dbx"] {
        assert!(out.contains(expected), "missing {expected} in {out}");
    }
}

#[test]
fn list_json_is_machine_readable() {
    let d = ws("c-json");
    let (code, out, _) = run_root(d.path(), &["list", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("list --json must be valid JSON");
    assert!(v.is_object(), "expected an object, got {v}");
}

#[test]
fn doctor_reports_healthy_after_init() {
    let d = ws("c-doc");
    let (code, out, _) = run_root(d.path(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("healthy"), "{out}");
}

#[test]
fn doctor_fails_on_uninitialised_directory() {
    let d = TempDir::new("c-doc2");
    let (code, _, err) = run_root(d.path(), &["doctor"]);
    assert_ne!(code, 0);
    assert!(err.contains("problem"), "{err}");
}

#[test]
fn errors_go_to_stderr_and_stdout_stays_clean() {
    let (code, out, err) = run(&["definitely-not-a-command"]);
    assert_eq!(code, 1);
    assert!(out.is_empty(), "stdout must be clean, got {out}");
    assert!(!err.is_empty());
}

#[test]
fn help_lists_every_command() {
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, 0);
    for cmd in [
        "init", "doctor", "list", "install", "projects", "link", "sync", "new", "status",
        "stop", "ports", "hosts", "recipes",
    ] {
        assert!(out.contains(cmd), "help missing `{cmd}`");
    }
}

// ------------------------------------------------------------- projects

#[test]
fn projects_lists_hostnames() {
    let d = ws("c-proj");
    fs::create_dir_all(d.path().join("www").join("My Shop")).unwrap();
    fs::create_dir_all(d.path().join("www").join("blog")).unwrap();

    let (code, out, _) = run_root(d.path(), &["projects"]);
    assert_eq!(code, 0);
    assert!(out.contains("my-shop.test"), "{out}");
    assert!(out.contains("blog.test"), "{out}");
}

#[test]
fn projects_json_has_the_documented_fields() {
    let d = ws("c-proj2");
    fs::create_dir_all(d.path().join("www").join("demo")).unwrap();

    let (code, out, _) = run_root(d.path(), &["projects", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let first = &v[0];
    for field in ["name", "hostname", "path", "document_root"] {
        assert!(first.get(field).is_some(), "missing {field} in {first}");
    }
}

#[test]
fn sync_dry_run_creates_nothing() {
    let d = ws("c-sync");
    fs::create_dir_all(d.path().join("www").join("demo")).unwrap();

    let (code, out, _) = run_root(d.path(), &["sync", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("demo.test"), "{out}");
    assert!(
        !d.path().join("conf").join("apache").exists(),
        "dry run must not write configs"
    );
}

#[test]
fn link_writes_a_vhost() {
    let d = ws("c-link");
    fs::create_dir_all(d.path().join("www").join("demo")).unwrap();

    let (code, out, _) = run_root(d.path(), &["link"]);
    assert_eq!(code, 0, "{out}");

    let conf = d
        .path()
        .join("conf")
        .join("apache")
        .join("sites-enabled")
        .join("demo.test.conf");
    assert!(conf.is_file(), "vhost must exist at {}", conf.display());
    let body = fs::read_to_string(&conf).unwrap();
    assert!(body.contains("ServerName demo.test"));
}

#[test]
fn unlink_removes_the_vhost() {
    let d = ws("c-unlink");
    fs::create_dir_all(d.path().join("www").join("demo")).unwrap();
    run_root(d.path(), &["link"]);

    let (code, _, _) = run_root(d.path(), &["unlink", "demo"]);
    assert_eq!(code, 0);
    let conf = d
        .path()
        .join("conf")
        .join("apache")
        .join("sites-enabled")
        .join("demo.test.conf");
    assert!(!conf.exists(), "vhost must be gone");
}

// -------------------------------------------------------------- recipes

#[test]
fn recipes_lists_builtins() {
    let (code, out, _) = run(&["recipes"]);
    assert_eq!(code, 0);
    for r in ["Blank", "WordPress", "Laravel"] {
        assert!(out.contains(r), "missing {r} in {out}");
    }
}

#[test]
fn new_creates_a_project_directory() {
    let d = ws("c-new");
    let (code, out, _) = run_root(d.path(), &["new", "mysite", "--from", "Blank"]);
    assert_eq!(code, 0, "{out}");
    assert!(d.path().join("www").join("mysite").is_dir());
}

#[test]
fn new_refuses_existing_project() {
    let d = ws("c-new2");
    fs::create_dir_all(d.path().join("www").join("taken")).unwrap();
    let (code, _, err) = run_root(d.path(), &["new", "taken", "--from", "Blank"]);
    assert_ne!(code, 0);
    assert!(err.contains("already exists"), "{err}");
}

#[test]
fn new_rejects_unknown_recipe() {
    let d = ws("c-new3");
    let (code, _, err) = run_root(d.path(), &["new", "x", "--from", "NoSuchRecipe"]);
    assert_ne!(code, 0);
    assert!(err.contains("NoSuchRecipe"), "{err}");
}

// --------------------------------------------------------------- ports

#[test]
fn ports_reports_configured_ports() {
    let d = ws("c-ports");
    let (code, out, _) = run_root(d.path(), &["ports"]);
    assert_eq!(code, 0);
    assert!(out.contains("http"), "{out}");
    assert!(out.contains("mysql"), "{out}");
    assert!(out.contains("phpmyadmin"), "{out}");
    // Each row states free or in use.
    assert!(out.contains("free") || out.contains("IN USE"), "{out}");
}

#[test]
fn status_reports_without_any_service() {
    let d = ws("c-status");
    let (code, out, _) = run_root(d.path(), &["status"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("service"), "{out}");
}

#[test]
fn stop_on_nothing_is_benign() {
    let d = ws("c-stop");
    let (code, _, _) = run_root(d.path(), &["stop"]);
    assert_eq!(code, 0, "stopping nothing must not fail");
}

// ---------------------------------------------------------------- misc

#[test]
fn install_without_network_explains_the_options() {
    let d = ws("c-inst");
    let (code, out, _) = run_root(d.path(), &["install", "php"]);
    assert_eq!(code, 0);
    assert!(out.contains("--from"), "must mention the offline path: {out}");
}

#[test]
fn install_rejects_unknown_component() {
    let d = ws("c-inst2");
    let (code, _, err) = run_root(d.path(), &["install", "nosuchthing"]);
    assert_ne!(code, 0);
    assert!(err.contains("nosuchthing"), "{err}");
}

#[test]
fn uninstall_requires_an_explicit_version() {
    let d = ws("c-uninst");
    let (code, _, err) = run_root(d.path(), &["uninstall", "php"]);
    assert_ne!(code, 0);
    assert!(err.contains("version"), "{err}");
}

#[test]
fn discover_works_from_a_subdirectory() {
    let d = ws("c-discover");
    let sub = d.path().join("www").join("proj");
    fs::create_dir_all(&sub).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rh"))
        .current_dir(&sub)
        .arg("list")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0));
}
