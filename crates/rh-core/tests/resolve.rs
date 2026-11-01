//! TDD-03 — Version resolution against recorded fixtures.
//!
//! Refs: PRD §4.4, NFR-12. **Never hits the network**: every strategy is tested
//! against a recorded upstream payload in `tests/fixtures/`.

use rh_core::resolve::{resolve, ResolveError, Upstream};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("fixture {} : {e}", p.display()))
}

/// An `Upstream` backed by a single canned body.
struct Canned(String);

impl Upstream for Canned {
    fn get(&self, _url: &str) -> Result<String, ResolveError> {
        Ok(self.0.clone())
    }
}

fn php() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/php.toml"
    ))
    .unwrap()
}

fn apache() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/apache.toml"
    ))
    .unwrap()
}

fn nginx() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/nginx.toml"
    ))
    .unwrap()
}

fn mariadb() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/mariadb.toml"
    ))
    .unwrap()
}

fn dbx() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/dbx.toml"
    ))
    .unwrap()
}

fn mailpit() -> rh_core::component::Descriptor {
    rh_core::component::Descriptor::parse(include_str!(
        "../../../components/mailpit.toml"
    ))
    .unwrap()
}

// ---------------------------------------------------------------- php

#[test]
fn php_resolver_picks_latest_patch_for_minor() {
    let up = Canned(fixture("php_releases.json"));
    let r = resolve(&php(), Some("8.4"), &up).unwrap();
    assert_eq!(r.version, "8.4.27");
    assert!(
        r.url.ends_with("php-8.4.27-1-Win32-vs17-x64.zip"),
        "got {}",
        r.url
    );
}

#[test]
fn php_resolver_picks_newest_major_when_unspecified() {
    let up = Canned(fixture("php_releases.json"));
    let r = resolve(&php(), None, &up).unwrap();
    assert_eq!(r.version, "8.5.12");
}

#[test]
fn php_resolver_prefers_ts_variant_over_nts() {
    let up = Canned(fixture("php_releases.json"));
    let r = resolve(&php(), Some("8.4"), &up).unwrap();
    assert!(r.url.contains("-Win32-vs17-x64.zip"));
    assert!(!r.url.contains("-nts-"), "must not pick the NTS build");
}

#[test]
fn php_resolver_falls_back_when_variant_absent_in_that_major() {
    // 8.3 only ships ts-vs16 in the fixture; descriptor asks for ts-vs17.
    let up = Canned(fixture("php_releases.json"));
    let r = resolve(&php(), Some("8.3"), &up).unwrap();
    assert!(
        r.url.ends_with("php-8.3.36-1-Win32-vs16-x64.zip"),
        "must fall back to an available variant, got {}",
        r.url
    );
}

// -------------------------------------------------------------- apache

#[test]
fn apache_resolver_parses_lounge_listing() {
    let up = Canned(fixture("apache_lounge.html"));
    let r = resolve(&apache(), None, &up).unwrap();
    assert_eq!(r.version, "2.4.69");
    assert!(
        r.url.ends_with("httpd-2.4.69-261002-Win64-VS18.zip"),
        "got {}",
        r.url
    );
}

#[test]
fn apache_resolver_ignores_win32_builds() {
    // The listing also offers win32; we must never select it.
    let up = Canned(fixture("apache_lounge.html"));
    let r = resolve(&apache(), None, &up).unwrap();
    assert!(!r.url.contains("win32"), "picked a 32-bit build: {}", r.url);
}

#[test]
fn apache_resolver_honours_exact_version() {
    let up = Canned(fixture("apache_lounge.html"));
    let r = resolve(&apache(), Some("2.4.65"), &up).unwrap();
    assert_eq!(r.version, "2.4.65");
    assert!(r.url.contains("httpd-2.4.65-250101-Win64-VS17.zip"));
}

// --------------------------------------------------------------- nginx

#[test]
fn nginx_resolver_parses_download_page() {
    let up = Canned(fixture("nginx_download.html"));
    let r = resolve(&nginx(), None, &up).unwrap();
    assert_eq!(r.version, "1.31.8");
    assert!(r.url.ends_with("nginx-1.31.8.zip"), "got {}", r.url);
}

#[test]
fn nginx_resolver_honours_exact_version() {
    let up = Canned(fixture("nginx_download.html"));
    let r = resolve(&nginx(), Some("1.29.4"), &up).unwrap();
    assert_eq!(r.version, "1.29.4");
}

// ------------------------------------------------------------- mariadb

#[test]
fn mariadb_resolver_prefers_lts_over_newer_non_lts() {
    // Fixture contains 13.0 (newer, non-LTS) and 11.8 (LTS). LTS must win.
    let up = Canned(fixture("mariadb_archive.html"));
    let r = resolve(&mariadb(), None, &up).unwrap();
    assert_eq!(
        r.version, "11.8.4",
        "expected LTS 11.8, got {} (prefer_lts must beat newest)",
        r.version
    );
    assert!(r.url.contains("mariadb-11.8.4-winx64.zip"), "{}", r.url);
}

#[test]
fn mariadb_resolver_exact_version_beats_lts_preference() {
    let up = Canned(fixture("mariadb_archive.html"));
    let r = resolve(&mariadb(), Some("13.0.2"), &up).unwrap();
    assert_eq!(r.version, "13.0.2");
}

// ----------------------------------------------------------- github

#[test]
fn github_resolver_matches_portable_asset_not_installer() {
    let up = Canned(fixture("github_dbx.json"));
    let r = resolve(&dbx(), None, &up).unwrap();
    assert_eq!(r.version, "0.6.62");
    assert!(
        r.url.ends_with("DBX_0.6.62_x64-portable.zip"),
        "must pick the portable zip, got {}",
        r.url
    );
    for forbidden in ["setup.exe", ".msi", ".deb", ".rpm", "AppImage", ".dmg"] {
        assert!(
            !r.url.contains(forbidden),
            "must not pick {forbidden}: {}",
            r.url
        );
    }
}

#[test]
fn github_resolver_selects_windows_asset_by_platform() {
    let up = Canned(fixture("github_mailpit.json"));
    let r = resolve(&mailpit(), None, &up).unwrap();
    assert_eq!(r.version, "1.33.0");
    assert!(r.url.ends_with("mailpit-windows-amd64.zip"), "{}", r.url);
    assert!(!r.url.contains("linux"), "picked a linux asset: {}", r.url);
}

// ------------------------------------------------------------ errors

#[test]
fn resolver_fails_with_actionable_error_when_version_absent() {
    let up = Canned(fixture("php_releases.json"));
    let err = resolve(&php(), Some("9.9"), &up).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("9.9"),
        "error must echo the requested version: {msg}"
    );
    // and it must tell the user what IS available
    assert!(
        msg.contains("8.5") || msg.contains("available"),
        "error should list available versions: {msg}"
    );
}

#[test]
fn malformed_upstream_payload_yields_no_partial_result() {
    struct Broken;
    impl Upstream for Broken {
        fn get(&self, _url: &str) -> Result<String, ResolveError> {
            Ok("{\"8.4\": { \"version\": \"8.4.27\"".to_string()) // truncated
        }
    }
    let err = resolve(&php(), None, &Broken).unwrap_err();
    assert!(
        matches!(err, ResolveError::Upstream { .. }),
        "truncated JSON must be reported as an upstream error, got {err:?}"
    );
}

#[test]
fn resolver_is_deterministic() {
    let up = Canned(fixture("php_releases.json"));
    let first = resolve(&php(), None, &up).unwrap();
    for _ in 0..10 {
        assert_eq!(resolve(&php(), None, &up).unwrap(), first);
    }
}

#[test]
fn resolver_returns_exact_version_when_fully_specified() {
    let up = Canned(fixture("php_releases.json"));
    let r = resolve(&php(), Some("8.4.27"), &up).unwrap();
    assert_eq!(r.version, "8.4.27");
}

#[test]
fn direct_strategy_needs_no_payload() {
    struct NoNetwork;
    impl Upstream for NoNetwork {
        fn get(&self, url: &str) -> Result<String, ResolveError> {
            panic!("direct strategy must not fetch an index (asked for {url})");
        }
    }
    let pma = rh_core::component::Descriptor::parse(include_str!(
        "../../../components/phpmyadmin.toml"
    ))
    .unwrap();
    let r = resolve(&pma, None, &NoNetwork).unwrap();
    assert!(r.url.contains("phpMyAdmin-5.2.3-all-languages.zip"));
    assert_eq!(r.version, "5.2.3");
}
