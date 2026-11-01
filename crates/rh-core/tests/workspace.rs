//! TDD-01 — Workspace layout & discovery.
//!
//! Refs: PRD §4.2, NFR-9 (relocatable), NFR-11 (idempotent).

mod common;
use common::{copy_tree, TempDir};

use rh_core::workspace::{DiscoverError, Workspace};
use std::fs;

fn tmp() -> TempDir {
    TempDir::new("ws")
}

#[test]
fn init_creates_exact_directory_set() {
    let d = tmp();
    let ws = Workspace::at(d.path());
    ws.init().unwrap();

    for sub in ["bin", "conf", "data", "tmp", "www"] {
        let p = d.path().join(sub);
        assert!(p.is_dir(), "expected directory {sub} at {}", p.display());
    }
    assert!(d.path().join("rh.toml").is_file(), "rh.toml must exist");
    assert!(
        d.path().join("components.lock.json").is_file(),
        "components.lock.json must exist"
    );
}

#[test]
fn init_is_idempotent_and_preserves_existing_rh_toml() {
    let d = tmp();
    let ws = Workspace::at(d.path());
    ws.init().unwrap();

    // user edits settings
    let cfg = d.path().join("rh.toml");
    let edited = "# my custom settings\ntld = \"local\"\n";
    fs::write(&cfg, edited).unwrap();

    ws.init().unwrap(); // second init must not clobber

    assert_eq!(fs::read_to_string(&cfg).unwrap(), edited);
}

#[test]
fn discover_walks_up_to_nearest_workspace_root() {
    let d = tmp();
    Workspace::at(d.path()).init().unwrap();

    let deep = d.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();

    let found = Workspace::discover(&deep).unwrap();
    assert_eq!(found.root(), d.path());
}

#[test]
fn discover_fails_cleanly_outside_any_workspace() {
    let d = tmp();
    // an empty temp dir with no rh.toml anywhere above it that we created
    let orphan = d.path().join("orphan");
    fs::create_dir_all(&orphan).unwrap();

    let err = Workspace::discover(&orphan).unwrap_err();
    match err {
        DiscoverError::NotFound { .. } => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn all_paths_are_relative_to_root() {
    let d = tmp();
    Workspace::at(d.path()).init().unwrap();
    let text = fs::read_to_string(d.path().join("rh.toml")).unwrap();

    // No absolute Windows drive letters or leading slashes in the settings file.
    assert!(
        !text.contains(":\\") && !text.contains(":/"),
        "rh.toml must not contain absolute paths, got:\n{text}"
    );
}

#[test]
fn workspace_survives_relocation() {
    // NFR-9: the portability guarantee. Copy the tree elsewhere; it must still work.
    let a = tmp();
    let ws_a = Workspace::at(a.path());
    ws_a.init().unwrap();

    // simulate an installed component directory
    fs::create_dir_all(ws_a.component_dir("php", "8.4.27")).unwrap();
    let mut idx = ws_a.load_lock().unwrap();
    idx.add("php", "8.4.27", "bin/php/8.4.27");
    ws_a.save_lock(&idx).unwrap();

    // relocate: copy the whole tree to b
    let b = tmp();
    copy_tree(a.path(), b.path());

    let ws_b = Workspace::at(b.path());
    let idx_b = ws_b.load_lock().unwrap();
    assert_eq!(idx_b.versions_of("php"), vec!["8.4.27".to_string()]);
    assert!(
        ws_b.component_dir("php", "8.4.27").is_dir(),
        "component dir must resolve under the new root"
    );
    assert_eq!(ws_b.root(), b.path());
}

