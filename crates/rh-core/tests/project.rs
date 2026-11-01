//! TDD-06 — Project discovery, slugging, and document-root detection.
//!
//! Refs: PRD §3.2 items 14, 18, 59.

mod common;
use common::TempDir;

use rh_core::project::{document_root, scan, slugify, Project};

use std::fs;

fn mkproject(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = root.join(name);
    fs::create_dir_all(&p).unwrap();
    p
}

// ------------------------------------------------------------------ slugify

#[test]
fn slugify_basic() {
    assert_eq!(slugify("My Shop"), "my-shop");
    assert_eq!(slugify("UPPER"), "upper");
    assert_eq!(slugify("simple"), "simple");
}

#[test]
fn slugifies_spaces_underscores_and_mixed() {
    assert_eq!(slugify("absen_smartwork_hpanel"), "absen-smartwork-hpanel");
    assert_eq!(slugify("My_Cool App"), "my-cool-app");
    assert_eq!(slugify("a  b"), "a-b");
}

#[test]
fn slugify_never_produces_empty() {
    assert_eq!(slugify(""), "app");
    assert_eq!(slugify("!!!"), "app");
    assert_eq!(slugify("---"), "app");
    assert_eq!(slugify("   "), "app");
}

#[test]
fn slugify_is_dns_safe() {
    for input in ["My Shop!!", "***", "UPPER CASE", "a.b.c", "x/y", "üñï"] {
        let s = slugify(input);
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{input:?} produced non-DNS-safe {s:?}"
        );
        assert!(!s.starts_with('-'), "{input:?} -> {s:?} starts with a dash");
        assert!(!s.ends_with('-'), "{input:?} -> {s:?} ends with a dash");
        assert!(!s.is_empty(), "{input:?} produced an empty slug");
    }
}

#[test]
fn slugify_truncates_to_dns_label_limit() {
    let long = "a".repeat(200);
    let s = slugify(&long);
    assert!(s.len() <= 63, "label too long: {}", s.len());
    assert!(s.chars().all(|c| c.is_ascii_lowercase()));
}

#[test]
fn slugify_collapses_and_trims_separators() {
    assert_eq!(slugify("---x---"), "x");
    assert_eq!(slugify("a___b"), "a-b");
    assert_eq!(slugify("_a_"), "a");
}

// --------------------------------------------------------------------- scan

#[test]
fn scans_www_and_ignores_hidden_and_files() {
    let d = TempDir::new("scan");
    let www = d.path().join("www");
    fs::create_dir_all(&www).unwrap();

    mkproject(&www, "blog");
    mkproject(&www, "shop");
    mkproject(&www, ".hidden");
    fs::write(www.join("notes.txt"), "x").unwrap();

    let projects = scan(&www, "test").unwrap();
    let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["blog", "shop"], "hidden dirs and files excluded");
}

#[test]
fn scan_is_sorted_deterministically() {
    let d = TempDir::new("scan2");
    let www = d.path().join("www");
    for n in ["zeta", "Alpha", "beta"] {
        mkproject(&www, n);
    }
    let a = scan(&www, "test").unwrap();
    let b = scan(&www, "test").unwrap();
    assert_eq!(a, b, "scan must be deterministic");
    let names: Vec<&str> = a.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "beta", "zeta"]);
}

#[test]
fn scan_of_missing_directory_is_empty_not_error() {
    let d = TempDir::new("scan3");
    let projects = scan(&d.path().join("nope"), "test").unwrap();
    assert!(projects.is_empty());
}

#[test]
fn project_gets_test_hostname() {
    let d = TempDir::new("host");
    let www = d.path().join("www");
    mkproject(&www, "My Shop");

    let projects = scan(&www, "test").unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].hostname, "my-shop.test");
}

#[test]
fn custom_tld_is_respected() {
    let d = TempDir::new("tld");
    let www = d.path().join("www");
    mkproject(&www, "demo");

    let p = scan(&www, "local").unwrap();
    assert_eq!(p[0].hostname, "demo.local");

    let p = scan(&www, "localhost").unwrap();
    assert_eq!(p[0].hostname, "demo.localhost");
}

#[test]
fn conflicting_slugs_are_reported() {
    // "My Shop" and "my_shop" both slug to my-shop: silently overwriting one
    // vhost with the other would be a nasty, hard-to-diagnose bug.
    let d = TempDir::new("conflict");
    let www = d.path().join("www");
    mkproject(&www, "My Shop");
    mkproject(&www, "my_shop");

    let err = scan(&www, "test").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("my-shop"), "error must name the clash: {msg}");
}

// ---------------------------------------------------------- document root

#[test]
fn detects_laravel_style_public_dir() {
    let d = TempDir::new("docroot");
    let p = mkproject(d.path(), "app");
    fs::create_dir_all(p.join("public")).unwrap();
    fs::write(p.join("artisan"), "").unwrap();

    assert_eq!(document_root(&p), p.join("public"));
}

#[test]
fn detects_public_html() {
    let d = TempDir::new("phtml");
    let p = mkproject(d.path(), "site");
    fs::create_dir_all(p.join("public_html")).unwrap();

    assert_eq!(document_root(&p), p.join("public_html"));
}

#[test]
fn detects_web_and_htdocs() {
    let d = TempDir::new("webdir");
    let p = mkproject(d.path(), "s");
    fs::create_dir_all(p.join("web")).unwrap();
    assert_eq!(document_root(&p), p.join("web"));

    let q = mkproject(d.path(), "t");
    fs::create_dir_all(q.join("htdocs")).unwrap();
    assert_eq!(document_root(&q), q.join("htdocs"));
}

#[test]
fn falls_back_to_project_root() {
    let d = TempDir::new("fallback");
    let p = mkproject(d.path(), "plain");
    fs::write(p.join("index.php"), "<?php").unwrap();

    assert_eq!(document_root(&p), p);
}

#[test]
fn public_dir_wins_over_web() {
    let d = TempDir::new("prec");
    let p = mkproject(d.path(), "both");
    fs::create_dir_all(p.join("public")).unwrap();
    fs::create_dir_all(p.join("web")).unwrap();
    assert_eq!(document_root(&p), p.join("public"));
}

#[test]
fn project_exposes_url() {
    let p = Project {
        name: "Demo".into(),
        slug: "demo".into(),
        hostname: "demo.test".into(),
        path: std::path::PathBuf::from("/x/demo"),
        document_root: std::path::PathBuf::from("/x/demo"),
    };
    assert_eq!(p.url("http"), "http://demo.test");
    assert_eq!(p.url("https"), "https://demo.test");
}
