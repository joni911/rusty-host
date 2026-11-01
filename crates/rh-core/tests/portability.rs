//! TDD-13 — Portability guarantee (NFR-9).
//!
//! The promise that makes this product what it is: **move the folder and it
//! still works**. These tests are the proof, and they are the ones most likely to
//! catch an accidental absolute path creeping into the design.

mod common;
use common::{copy_tree, TempDir};

use rh_core::hosts::{self, Block};
use rh_core::project;
use rh_core::settings::Settings;
use rh_core::vhost::{self, Server};
use rh_core::workspace::Workspace;
use std::fs;
use std::path::Path;

/// Build a workspace with one "installed" component, one project and a vhost.
fn populated(root: &Path) {
    let ws = Workspace::at(root);
    ws.init().unwrap();

    // A stand-in component: we only care that the path resolves after moving.
    let comp = ws.component_dir("php", "8.4.27");
    fs::create_dir_all(&comp).unwrap();
    fs::write(comp.join("php.exe"), b"stub").unwrap();

    let mut lock = ws.load_lock().unwrap();
    lock.add("php", "8.4.27", "bin/php/8.4.27");
    ws.save_lock(&lock).unwrap();

    // A project, with a framework-style public dir.
    let proj = ws.www().join("Demo Shop");
    fs::create_dir_all(proj.join("public")).unwrap();
    fs::write(proj.join("public").join("index.php"), "<?php").unwrap();

    let s = Settings::load(&ws).unwrap();
    let projects = project::scan(&ws.www(), &s.tld).unwrap();
    let ctx = vhost::Context {
        conf_dir: &ws.conf(),
        port: s.port("http"),
        php_port: 9000,
    };
    vhost::sync(&projects, Server::Apache, &ctx).unwrap();
}

#[test]
fn relocate_workspace_and_keep_working() {
    let a = TempDir::new("reloc-a");
    populated(a.path());

    // Sanity: it worked where it was created.
    let ws_a = Workspace::at(a.path());
    assert!(ws_a.component_dir("php", "8.4.27").is_dir());

    // Move the whole tree.
    let b = TempDir::new("reloc-b");
    copy_tree(a.path(), b.path());

    // Everything must still resolve, under the NEW root.
    let ws_b = Workspace::at(b.path());

    let lock = ws_b.load_lock().unwrap();
    assert_eq!(lock.versions_of("php"), vec!["8.4.27".to_string()]);
    assert!(
        ws_b.component_dir("php", "8.4.27").join("php.exe").is_file(),
        "component must resolve under the new root"
    );

    let s = Settings::load(&ws_b).unwrap();
    let projects = project::scan(&ws_b.www(), &s.tld).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].hostname, "demo-shop.test");
    assert!(
        projects[0].document_root.ends_with("public"),
        "document root must be re-derived, not remembered: {:?}",
        projects[0].document_root
    );

    // Re-syncing at the new location must produce configs pointing at B.
    let ctx = vhost::Context {
        conf_dir: &ws_b.conf(),
        port: s.port("http"),
        php_port: 9000,
    };
    let report = vhost::sync(&projects, Server::Apache, &ctx).unwrap();
    assert_eq!(report.written.len(), 1);

    let conf = ws_b
        .conf()
        .join("apache")
        .join("sites-enabled")
        .join("demo-shop.test.conf");
    let body = fs::read_to_string(&conf).unwrap();
    let b_str = b.path().display().to_string().replace('\\', "/");
    assert!(
        body.contains(&b_str),
        "regenerated config must reference the new root {b_str}:\n{body}"
    );
    let a_str = a.path().display().to_string().replace('\\', "/");
    assert!(
        !body.contains(&a_str),
        "regenerated config must not reference the old root {a_str}:\n{body}"
    );
}

#[test]
fn no_absolute_paths_in_rh_toml_or_lock() {
    let a = TempDir::new("reloc-paths");
    populated(a.path());

    let ws = Workspace::at(a.path());
    for file in [ws.settings_path(), ws.lock_path()] {
        let text = fs::read_to_string(&file).unwrap();
        let root = a.path().display().to_string().replace('\\', "/");
        assert!(
            !text.contains(&root),
            "{} must not contain the absolute workspace root:\n{text}",
            file.display()
        );
        // No drive letters either.
        assert!(
            !text.contains(":\\") && !text.contains(":/"),
            "{} must not contain a drive-qualified path:\n{text}",
            file.display()
        );
    }
}

#[test]
fn generated_configs_are_rewritten_after_relocation() {
    // The same guarantee, stated as a behaviour: stale generated output is
    // harmless because sync regenerates it from relative state.
    let a = TempDir::new("reloc-gen");
    populated(a.path());

    let b = TempDir::new("reloc-gen2");
    copy_tree(a.path(), b.path());

    let ws_b = Workspace::at(b.path());
    let s = Settings::load(&ws_b).unwrap();
    let projects = project::scan(&ws_b.www(), &s.tld).unwrap();
    let ctx = vhost::Context {
        conf_dir: &ws_b.conf(),
        port: s.port("http"),
        php_port: 9000,
    };

    // A dry run already reports the file as needing a write, because the
    // document root changed.
    let plan = vhost::sync_dry_run(&projects, Server::Apache, &ctx).unwrap();
    assert_eq!(plan.written.len(), 1);

    // And applying it makes the file consistent with the new location.
    vhost::sync(&projects, Server::Apache, &ctx).unwrap();
    let conf = ws_b
        .conf()
        .join("apache")
        .join("sites-enabled")
        .join("demo-shop.test.conf");
    let body = fs::read_to_string(&conf).unwrap();
    assert!(body.contains(&b.path().display().to_string().replace('\\', "/")));
}

#[test]
fn hosts_block_is_independent_of_location() {
    // The hosts file only knows hostnames, never paths, so relocation cannot
    // corrupt it.
    let d = TempDir::new("reloc-hosts");
    let h = d.path().join("hosts");
    fs::write(&h, "127.0.0.1 localhost\n").unwrap();

    let block = Block::new([("demo-shop.test", "127.0.0.1")]);
    hosts::sync(&h, &block).unwrap();
    let before = fs::read_to_string(&h).unwrap();

    // Simulate a relocation: nothing about the block changes.
    hosts::sync(&h, &block).unwrap();
    assert_eq!(fs::read_to_string(&h).unwrap(), before);
    assert!(!before.contains(":\\"), "hosts must never contain paths");
}

#[test]
fn workspace_can_live_at_a_very_different_path_depth() {
    // Guard against code that assumes a fixed depth, e.g. "../.." tricks.
    let a = TempDir::new("reloc-depth");
    populated(a.path());

    let deep_root = TempDir::new("reloc-deep");
    let deep = deep_root
        .path()
        .join("one")
        .join("two")
        .join("three")
        .join("four");
    fs::create_dir_all(&deep).unwrap();
    copy_tree(a.path(), &deep);

    let ws = Workspace::at(&deep);
    assert!(ws.component_dir("php", "8.4.27").join("php.exe").is_file());
    let s = Settings::load(&ws).unwrap();
    assert_eq!(
        project::scan(&ws.www(), &s.tld).unwrap().len(),
        1,
        "deeply nested workspace must still scan"
    );
}
