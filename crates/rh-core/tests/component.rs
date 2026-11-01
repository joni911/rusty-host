//! TDD-02 — Component descriptor parsing.
//!
//! Refs: PRD §4.3. The extensibility core: adding a component must require
//! **no Rust code**, only a TOML file.

mod common;
use common::TempDir;

use rh_core::component::{Artifact, ComponentKind, Descriptor, ResolveStrategy};
use std::fs;

const PHP: &str = r#"
name = "php"
kind = "runtime"
provides = ["php"]

[resolve]
strategy = "json-index"
index_url = "https://downloads.php.net/~windows/releases/releases.json"
variant = "ts-vs17-x64"
url_field = "zip.path"
base_url = "https://downloads.php.net/~windows/releases/"

[[artifacts]]
executable = "php.exe"
version_cmd = ["php.exe", "-r", "echo PHP_VERSION;"]
version_regex = '^(\d+\.\d+\.\d+)'

[config]
template = "php.ini.tpl"
target = "conf/php/{version}/php.ini"
"#;

const MARIADB: &str = r#"
name = "mariadb"
kind = "database"

[resolve]
strategy = "html-scrape"
index_url = "https://archive.mariadb.org/"
file_regex = 'mariadb-(\d+\.\d+\.\d+)-winx64\.zip'
base_url = "https://archive.mariadb.org/mariadb-{version}/winx64-packages/"
prefer_lts = true

[[artifacts]]
executable = "bin/mariadb.exe"
version_cmd = ["bin/mariadb.exe", "--version"]
version_regex = 'Distrib (\d+\.\d+\.\d+)'
"#;

const DBX: &str = r#"
name = "dbx"
kind = "tool"

[resolve]
strategy = "github-release"
repo = "t8y2/dbx"
asset_regex = 'DBX_[\d.]+_x64-portable\.zip$'

[[artifacts]]
executable = "dbx.exe"
"#;

#[test]
fn parses_minimal_runtime_descriptor() {
    let d = Descriptor::parse(PHP).unwrap();
    assert_eq!(d.name, "php");
    assert_eq!(d.kind, ComponentKind::Runtime);
    assert_eq!(d.provides, vec!["php".to_string()]);
    assert!(matches!(d.resolve, ResolveStrategy::JsonIndex { .. }));
    assert_eq!(d.artifacts.len(), 1);
    assert_eq!(d.artifacts[0].executable, "php.exe");
    assert_eq!(d.artifacts[0].version_regex.as_deref(), Some(r"^(\d+\.\d+\.\d+)"));
    let cfg = d.config.expect("config block");
    assert_eq!(cfg.template, "php.ini.tpl");
    assert!(cfg.target.contains("{version}"));
}

#[test]
fn parses_database_descriptor_with_html_scrape() {
    let d = Descriptor::parse(MARIADB).unwrap();
    assert_eq!(d.kind, ComponentKind::Database);
    match d.resolve {
        ResolveStrategy::HtmlScrape {
            prefer_lts,
            file_regex,
            ..
        } => {
            assert!(prefer_lts);
            assert!(file_regex.contains("mariadb-"));
        }
        other => panic!("expected HtmlScrape, got {other:?}"),
    }
}

#[test]
fn parses_tool_descriptor_with_github_release() {
    let d = Descriptor::parse(DBX).unwrap();
    assert_eq!(d.kind, ComponentKind::Tool);
    match d.resolve {
        ResolveStrategy::GithubRelease { repo, asset_regex } => {
            assert_eq!(repo, "t8y2/dbx");
            // the portable zip, never the installer
            assert!(asset_regex.contains("portable"));
        }
        other => panic!("expected GithubRelease, got {other:?}"),
    }
}

#[test]
fn rejects_descriptor_missing_name() {
    let bad = r#"
kind = "runtime"
[resolve]
strategy = "direct"
url = "https://example.com/x.zip"
[[artifacts]]
executable = "x.exe"
"#;
    let err = Descriptor::parse(bad).unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("name"), "error must mention `name`: {err}");
}

#[test]
fn rejects_unknown_resolve_strategy() {
    let bad = r#"
name = "weird"
kind = "runtime"
[resolve]
strategy = "quantum-teleport"
[[artifacts]]
executable = "x.exe"
"#;
    let err = Descriptor::parse(bad).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("quantum-teleport") || msg.contains("strategy"),
        "error must name the offending strategy: {err}"
    );
}

#[test]
fn rejects_descriptor_with_empty_artifacts() {
    let bad = r#"
name = "empty"
kind = "runtime"
[resolve]
strategy = "direct"
url = "https://example.com/x.zip"
artifacts = []
"#;
    let err = Descriptor::parse(bad).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("artifact"),
        "error must mention artifacts: {err}"
    );
}

#[test]
fn rejects_component_name_with_path_separators() {
    let bad = r#"
name = "../escape"
kind = "runtime"
[resolve]
strategy = "direct"
url = "https://example.com/x.zip"
[[artifacts]]
executable = "x.exe"
"#;
    assert!(
        Descriptor::parse(bad).is_err(),
        "component names must not contain path separators"
    );
}

#[test]
fn every_shipped_descriptor_parses() {
    // Guards the set of components we ship. Any malformed file fails CI.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("components");

    let mut seen = 0;
    for entry in fs::read_dir(&dir).expect("components/ must exist") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let d = Descriptor::from_path(&path).unwrap_or_else(|e| {
            panic!("shipped descriptor {} failed to parse: {e}", path.display())
        });
        assert!(!d.name.is_empty());
        seen += 1;
    }
    assert!(seen >= 6, "expected at least 6 shipped descriptors, found {seen}");
}

#[test]
fn descriptor_requires_no_code_to_add_a_component() {
    // PRD §4.3 claim, verified: drop a TOML file in, it is discovered.
    let d = TempDir::new("desc");
    let cdir = d.path().join("components");
    fs::create_dir_all(&cdir).unwrap();
    fs::write(
        cdir.join("redis.toml"),
        r#"
name = "redis"
kind = "tool"
[resolve]
strategy = "github-release"
repo = "redis-windows/redis-windows"
asset_regex = 'Redis-.*-Windows-x64-.*\.zip'
[[artifacts]]
executable = "redis-server.exe"
"#,
    )
    .unwrap();

    let catalogue = rh_core::component::load_catalogue(&cdir).unwrap();
    assert!(catalogue.contains_key("redis"));
    assert_eq!(catalogue["redis"].name, "redis");
}

#[test]
fn catalogue_rejects_duplicate_names() {
    let d = TempDir::new("dup");
    let cdir = d.path().join("components");
    fs::create_dir_all(&cdir).unwrap();
    let body = r#"
name = "same"
kind = "tool"
[resolve]
strategy = "direct"
url = "https://example.com/a.zip"
[[artifacts]]
executable = "a.exe"
"#;
    fs::write(cdir.join("a.toml"), body).unwrap();
    fs::write(cdir.join("b.toml"), body).unwrap();
    assert!(
        rh_core::component::load_catalogue(&cdir).is_err(),
        "duplicate component names must be rejected"
    );
}

#[test]
fn artifact_helper_picks_matching_binary() {
    let a = Artifact {
        executable: "php.exe".into(),
        version_cmd: Some(vec!["php.exe".into(), "-v".into()]),
        version_regex: None,
    };
    assert_eq!(a.executable, "php.exe");
    assert_eq!(a.version_cmd.as_ref().unwrap().len(), 2);
}
