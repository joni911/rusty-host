//! TDD-08 — Vhost rendering and reconciliation.
//!
//! Refs: PRD §3.2 items 16, 17, 19.

mod common;
use common::TempDir;

use rh_core::project::Project;
use rh_core::vhost::{self, Server, SyncAction};
use std::fs;
use std::path::{Path, PathBuf};

fn proj(root: &Path, name: &str, hostname: &str) -> Project {
    let path = root.join("www").join(name);
    fs::create_dir_all(&path).unwrap();
    Project {
        name: name.to_string(),
        slug: hostname.split('.').next().unwrap().to_string(),
        hostname: hostname.to_string(),
        document_root: path.clone(),
        path,
    }
}

fn sites(conf: &Path) -> PathBuf {
    conf.join("apache").join("sites-enabled")
}

fn ctx<'a>(conf: &'a Path) -> vhost::Context<'a> {
    vhost::Context {
        conf_dir: conf,
        port: 80,
        php_port: 9000,
    }
}

// ---------------------------------------------------------------- render

#[test]
fn link_creates_apache_conf_and_returns_path() {
    let d = TempDir::new("v1");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");
    let out = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();

    assert!(out.path.is_file(), "conf must exist at {}", out.path.display());
    let body = fs::read_to_string(&out.path).unwrap();
    assert!(body.contains("ServerName demo.test"));
    assert!(body.contains("<VirtualHost *:80>"));
}

#[test]
fn generated_conf_is_marked_as_managed() {
    // The marker is how `sync --prune` knows it may delete the file.
    let d = TempDir::new("v2");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");
    let out = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();
    let body = fs::read_to_string(&out.path).unwrap();
    assert!(
        body.contains("Managed-by: rusty-host"),
        "generated conf must carry the managed marker:\n{body}"
    );
}

#[test]
fn link_is_idempotent() {
    let d = TempDir::new("v3");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");

    let a = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();
    let first = fs::read_to_string(&a.path).unwrap();
    let b = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();
    let second = fs::read_to_string(&b.path).unwrap();

    assert_eq!(a.path, b.path);
    assert_eq!(first, second, "re-linking must produce identical output");
}

#[test]
fn link_updates_existing_conf_when_document_root_changes() {
    let d = TempDir::new("v4");
    let mut p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");

    vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();

    // Framework added later -> document root moves to public/
    let public = p.path.join("public");
    fs::create_dir_all(&public).unwrap();
    p.document_root = public;

    let out = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();
    let body = fs::read_to_string(&out.path).unwrap();
    assert!(
        body.contains("public"),
        "document root change must be picked up:\n{body}"
    );
}

#[test]
fn nginx_variant_emits_server_block() {
    let d = TempDir::new("v5");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");
    let out = vhost::link(&p, Server::Nginx, &ctx(&conf)).unwrap();
    let body = fs::read_to_string(&out.path).unwrap();

    assert!(body.contains("server {"));
    assert!(body.contains("server_name demo.test www.demo.test;"));
    assert!(body.contains("fastcgi_pass 127.0.0.1:9000;"));
}

#[test]
fn ssl_variant_includes_cert_directives() {
    let d = TempDir::new("v6");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");
    let c = vhost::Context {
        conf_dir: &conf,
        port: 80,
        php_port: 9000,
    };
    let out = vhost::link_ssl(
        &p,
        Server::Apache,
        &c,
        Path::new("C:/rh/conf/ssl/demo.test.crt"),
        Path::new("C:/rh/conf/ssl/demo.test.key"),
    )
    .unwrap();
    let body = fs::read_to_string(&out.path).unwrap();

    assert!(body.contains("SSLEngine on"));
    assert!(body.contains("SSLCertificateFile"));
    assert!(body.contains("SSLCertificateKeyFile"));
    assert!(body.contains("C:/rh/conf/ssl/demo.test.crt"));
}

// ----------------------------------------------------------------- sync

#[test]
fn sync_creates_conf_for_every_project() {
    let d = TempDir::new("s1");
    let a = proj(d.path(), "a", "a.test");
    let b = proj(d.path(), "b", "b.test");
    let conf = d.path().join("conf");

    let report = vhost::sync(&[a, b], Server::Apache, &ctx(&conf)).unwrap();

    assert_eq!(report.written.len(), 2);
    assert!(sites(&conf).join("a.test.conf").is_file());
    assert!(sites(&conf).join("b.test.conf").is_file());
}

#[test]
fn sync_prunes_conf_for_deleted_project() {
    let d = TempDir::new("s2");
    let a = proj(d.path(), "a", "a.test");
    let b = proj(d.path(), "b", "b.test");
    let conf = d.path().join("conf");

    vhost::sync(&[a.clone(), b], Server::Apache, &ctx(&conf)).unwrap();
    assert!(sites(&conf).join("b.test.conf").is_file());

    // b's project directory is gone from the list -> its conf must be pruned.
    let report = vhost::sync(&[a], Server::Apache, &ctx(&conf)).unwrap();

    assert!(
        !sites(&conf).join("b.test.conf").exists(),
        "orphaned vhost must be removed"
    );
    assert_eq!(report.pruned.len(), 1);
    assert!(
        report.pruned[0].path.ends_with("b.test.conf"),
        "pruned the wrong file: {:?}",
        report.pruned[0].path
    );
}

#[test]
fn sync_never_deletes_hand_written_conf() {
    // A conf without our marker is the user's; deleting it would be destructive.
    let d = TempDir::new("s3");
    let conf = d.path().join("conf");
    fs::create_dir_all(&conf).unwrap();
    let hand = conf.join("mine.conf");
    fs::write(&hand, "<VirtualHost *:80>\n  # hand written\n</VirtualHost>\n").unwrap();

    let report = vhost::sync(&[], Server::Apache, &ctx(&conf)).unwrap();

    assert!(hand.is_file(), "hand-written conf must survive");
    assert!(report.pruned.is_empty());
}

#[test]
fn sync_dry_run_reports_without_touching_disk() {
    let d = TempDir::new("s4");
    let a = proj(d.path(), "a", "a.test");
    let conf = d.path().join("conf");

    let report = vhost::sync_dry_run(&[a], Server::Apache, &ctx(&conf)).unwrap();

    assert_eq!(report.written.len(), 1);
    assert!(!conf.exists(), "dry run must not create anything");
}

#[test]
fn sync_removes_stale_conf_in_dry_run_report_only() {
    let d = TempDir::new("s5");
    let a = proj(d.path(), "a", "a.test");
    let b = proj(d.path(), "b", "b.test");
    let conf = d.path().join("conf");
    vhost::sync(&[a.clone(), b], Server::Apache, &ctx(&conf)).unwrap();

    let report = vhost::sync_dry_run(&[a], Server::Apache, &ctx(&conf)).unwrap();
    assert_eq!(report.pruned.len(), 1);
    assert!(
        sites(&conf).join("b.test.conf").is_file(),
        "dry run must not prune"
    );
}

#[test]
fn sync_reports_every_action() {
    let d = TempDir::new("s6");
    let a = proj(d.path(), "a", "a.test");
    let conf = d.path().join("conf");
    vhost::sync(&[a], Server::Apache, &ctx(&conf)).unwrap();

    let report = vhost::sync(&[proj(d.path(), "c", "c.test")], Server::Apache, &ctx(&conf)).unwrap();

    assert_eq!(report.written.len(), 1, "c is new");
    assert_eq!(report.pruned.len(), 1, "a is gone");
    for action in report.written.iter().chain(report.pruned.iter()) {
        assert!(!action.display().is_empty(), "actions must be describable");
    }
}

#[test]
fn sync_action_display_is_informative() {
    let d = TempDir::new("s7");
    let p = proj(d.path(), "demo", "demo.test");
    let conf = d.path().join("conf");
    let out = vhost::link(&p, Server::Apache, &ctx(&conf)).unwrap();

    let action = SyncAction {
        path: out.path.clone(),
        hostname: "demo.test".to_string(),
        body: String::new(),
    };
    let shown = action.display();
    assert!(shown.contains("demo.test"), "{shown}");
}

#[test]
fn conf_path_uses_hostname() {
    let conf = PathBuf::from("X:/conf");
    let p = Project {
        name: "Demo".into(),
        slug: "demo".into(),
        hostname: "demo.test".into(),
        path: PathBuf::from("X:/www/Demo"),
        document_root: PathBuf::from("X:/www/Demo"),
    };
    let out = vhost::render_project(
        &p,
        Server::Apache,
        &vhost::Context {
            conf_dir: &conf,
            port: 8080,
            php_port: 9000,
        },
        None,
    )
    .unwrap();
    assert_eq!(out.path.file_name().unwrap(), "demo.test.conf");
    assert_eq!(out.hostname, "demo.test");
    assert!(
        out.path.to_string_lossy().contains("sites-enabled"),
        "confs belong in sites-enabled: {}",
        out.path.display()
    );
}
