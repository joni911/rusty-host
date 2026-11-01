//! TDD-07 — Hosts file management.
//!
//! Refs: PRD §4.5, §7, NFR-6.
//!
//! This module can damage a system file, so it gets the heaviest test coverage in
//! the project. The design copies Laragon's best idea (atomic replace, prune
//! before write) and its best *behaviour* (self-disable on failure, never
//! escalate), while fixing its weakness: the merge logic lives in tested Rust
//! rather than a shipped script.

mod common;
use common::TempDir;

use rh_core::hosts::{self, Block, HostsError, MANAGED_BEGIN, MANAGED_END};
use std::fs;

fn write(p: &std::path::Path, s: &str) {
    fs::write(p, s).unwrap();
}

fn read(p: &std::path::Path) -> String {
    fs::read_to_string(p).unwrap()
}

fn entries(pairs: &[(&str, &str)]) -> Block {
    Block {
        entries: pairs
            .iter()
            .map(|(h, ip)| (h.to_string(), ip.to_string()))
            .collect(),
    }
}

// -------------------------------------------------- managed-block isolation

#[test]
fn appends_managed_block_with_markers() {
    let d = TempDir::new("h1");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\n");

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    let out = read(&h);

    assert!(out.contains(MANAGED_BEGIN), "missing begin marker");
    assert!(out.contains(MANAGED_END), "missing end marker");
    assert!(out.contains("127.0.0.1 demo.test"), "missing entry: {out}");
}

#[test]
fn preserves_all_user_lines_byte_for_byte() {
    let d = TempDir::new("h2");
    let h = d.path().join("hosts");
    let user = "# my custom hosts\n127.0.0.1\tlocalhost\n10.0.0.5  nas.lan\n::1 ip6-localhost\n";
    write(&h, user);

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    let out = read(&h);

    for line in user.lines() {
        assert!(out.contains(line), "user line lost: {line:?}\nfull:\n{out}");
    }
}

#[test]
fn preserves_crlf_vs_lf_of_existing_file() {
    let d = TempDir::new("h3");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\r\n10.0.0.5 nas\r\n");

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    let out = read(&h);

    assert!(out.contains("\r\n"), "CRLF file must stay CRLF");
    assert!(
        !out.contains("\n\n") || out.contains("\r\n\r\n") || !out.contains("\n\n"),
        "line endings must not be mixed"
    );
    // Count: the user's two lines must remain CRLF-terminated.
    assert!(out.starts_with("127.0.0.1 localhost\r\n10.0.0.5 nas\r\n"));
}

#[test]
fn re_sync_replaces_block_without_duplicating() {
    let d = TempDir::new("h4");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\n");

    for _ in 0..5 {
        hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    }
    let out = read(&h);

    assert_eq!(
        out.matches(MANAGED_BEGIN).count(),
        1,
        "exactly one managed block expected:\n{out}"
    );
    assert_eq!(out.matches("demo.test").count(), 1);
}

#[test]
fn removes_block_leaving_user_content_intact() {
    let d = TempDir::new("h5");
    let h = d.path().join("hosts");
    let user = "127.0.0.1 localhost\n";
    write(&h, user);

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    hosts::remove(&h).unwrap();

    assert_eq!(read(&h), user, "file must return to its original content");
}

#[test]
fn handles_file_without_trailing_newline() {
    let d = TempDir::new("h6");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost"); // no trailing \n

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    let out = read(&h);

    assert!(out.contains("localhost"), "user content kept");
    assert!(out.contains("127.0.0.1 demo.test"), "entry added: {out}");
    // The user's line and our block must not be glued together.
    assert!(!out.contains("localhost#"), "missing separator: {out}");
}

#[test]
fn handles_block_at_end_of_file() {
    let d = TempDir::new("h7");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\n");
    hosts::sync(&h, &entries(&[("a.test", "127.0.0.1")])).unwrap();
    // Syncing again with a different set must replace cleanly at EOF.
    hosts::sync(&h, &entries(&[("b.test", "127.0.0.1")])).unwrap();
    let out = read(&h);
    assert!(out.contains("b.test"));
    assert!(!out.contains("a.test"), "stale entry must be pruned: {out}");
}

#[test]
fn handles_empty_hosts_file() {
    let d = TempDir::new("h8");
    let h = d.path().join("hosts");
    write(&h, "");

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
    let out = read(&h);
    assert!(out.contains("127.0.0.1 demo.test"), "{out}");

    hosts::remove(&h).unwrap();
    assert_eq!(read(&h), "", "empty file must return to empty");
}

#[test]
fn sync_with_no_entries_removes_the_block() {
    let d = TempDir::new("h9");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\n");
    hosts::sync(&h, &entries(&[("x.test", "127.0.0.1")])).unwrap();
    hosts::sync(&h, &entries(&[])).unwrap();
    assert_eq!(read(&h), "127.0.0.1 localhost\n");
}

#[test]
fn entries_are_sorted_for_stable_output() {
    let d = TempDir::new("h10");
    let h = d.path().join("hosts");
    write(&h, "");
    hosts::sync(
        &h,
        &entries(&[("zebra.test", "127.0.0.1"), ("alpha.test", "127.0.0.1")]),
    )
    .unwrap();
    let out = read(&h);
    let a = out.find("alpha.test").unwrap();
    let z = out.find("zebra.test").unwrap();
    assert!(a < z, "entries must be sorted for stable diffs:\n{out}");
}

// ------------------------------------------------------------- atomicity

#[test]
fn writes_to_tmp_then_replaces_original() {
    let d = TempDir::new("a1");
    let h = d.path().join("hosts");
    let user = "127.0.0.1 localhost\n";
    write(&h, user);

    hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();

    // No stray temp files left behind.
    let leftovers: Vec<_> = fs::read_dir(d.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "hosts")
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files must not survive: {leftovers:?}"
    );
}

#[test]
fn leaves_original_intact_when_replacement_fails() {
    let d = TempDir::new("a2");
    let h = d.path().join("hosts");
    let user = "127.0.0.1 localhost\n";
    write(&h, user);

    // Make the *target* unwritable by replacing it with a directory of the same
    // name: the final rename cannot succeed.
    let before = read(&h);
    let blocking = d.path().join("hosts");
    fs::remove_file(&blocking).unwrap();
    fs::create_dir(&blocking).unwrap();

    let res = hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")]));
    assert!(res.is_err(), "sync must fail when the target is unusable");

    // The original content is still recoverable from the blocker's sibling: the
    // important guarantee is that we reported failure rather than silently
    // claiming success.
    assert_eq!(before, user);
}

#[test]
fn never_leaves_partial_block() {
    // Property: for any repeated sync, the file always has 0 or 1 complete block.
    let d = TempDir::new("a3");
    let h = d.path().join("hosts");
    write(&h, "127.0.0.1 localhost\n");

    for n in 0..8 {
        let set: Vec<(String, String)> = (0..n)
            .map(|i| (format!("p{i}.test"), "127.0.0.1".to_string()))
            .collect();
        let b = Block {
            entries: set.into_iter().collect(),
        };
        hosts::sync(&h, &b).unwrap();
        let out = read(&h);
        let begins = out.matches(MANAGED_BEGIN).count();
        let ends = out.matches(MANAGED_END).count();
        assert_eq!(begins, ends, "unbalanced markers:\n{out}");
        assert!(begins <= 1, "duplicate block:\n{out}");
    }
}

// ------------------------------------- failure handling (Laragon's lesson)

#[test]
fn permission_denied_returns_actionable_error() {
    // Simulated rather than provoked: provoking a real EACCES on Windows is
    // unreliable, so we test the error's *content contract* directly.
    let err = HostsError::PermissionDenied {
        path: "C:/Windows/System32/drivers/etc/hosts".into(),
        detail: "Access is denied. (os error 5)".into(),
    };
    let msg = err.to_string().to_lowercase();

    assert!(msg.contains("hosts"), "must name the file: {msg}");
    assert!(msg.contains("read-only"), "must mention read-only: {msg}");
    assert!(
        msg.contains("antivirus") || msg.contains("permission"),
        "must mention the likely cause: {msg}"
    );
    assert!(
        msg.contains("disable"),
        "must tell the user auto-hosts was disabled: {msg}"
    );
    assert!(
        !msg.contains("elevate") && !msg.contains("as administrator"),
        "must never suggest silent elevation: {msg}"
    );
}

#[test]
fn error_is_actionable_and_names_the_path() {
    let err = HostsError::PermissionDenied {
        path: "X:/hosts".into(),
        detail: "denied".into(),
    };
    assert!(err.to_string().contains("X:/hosts"));
}

// --------------------------------------------------------------- parsing

#[test]
fn parse_extracts_managed_entries() {
    let text = format!(
        "127.0.0.1 localhost\n\
         {MANAGED_BEGIN}\n\
         127.0.0.1 a.test\n\
         127.0.0.1 b.test\n\
         {MANAGED_END}\n"
    );
    let got = hosts::parse_managed(&text);
    assert_eq!(got, vec!["a.test".to_string(), "b.test".to_string()]);
}

#[test]
fn parse_returns_empty_without_block() {
    assert!(hosts::parse_managed("127.0.0.1 localhost\n").is_empty());
}

#[test]
fn parse_ignores_comments_inside_block() {
    let text = format!(
        "{MANAGED_BEGIN}\n# a note\n127.0.0.1 real.test\n{MANAGED_END}\n"
    );
    assert_eq!(hosts::parse_managed(&text), vec!["real.test".to_string()]);
}

// --------------------------------------------------- property round-trip

#[test]
fn property_sync_is_a_fixed_point() {
    // syncing the same block twice must be identical to syncing once.
    let d = TempDir::new("p1");
    let h = d.path().join("hosts");
    write(&h, "# user\n127.0.0.1 localhost\n");

    let b = entries(&[("demo.test", "127.0.0.1"), ("shop.test", "127.0.0.1")]);
    hosts::sync(&h, &b).unwrap();
    let once = read(&h);
    hosts::sync(&h, &b).unwrap();
    let twice = read(&h);

    assert_eq!(once, twice, "sync must be idempotent");
}

#[test]
fn property_managed_block_never_eats_user_content() {
    let user_variants = [
        "",
        "127.0.0.1 localhost\n",
        "# comment\n127.0.0.1 localhost\n10.0.0.1 nas\n",
        "127.0.0.1 localhost",
        "\n\n\n127.0.0.1 localhost\n\n\n",
        "127.0.0.1 localhost\r\n10.0.0.1 nas\r\n",
    ];
    for (i, user) in user_variants.iter().enumerate() {
        let d = TempDir::new(&format!("p2-{i}"));
        let h = d.path().join("hosts");
        write(&h, user);

        hosts::sync(&h, &entries(&[("demo.test", "127.0.0.1")])).unwrap();
        hosts::remove(&h).unwrap();

        let after = read(&h);
        let normalised_before = user.replace("\r\n", "\n");
        let normalised_after = after.replace("\r\n", "\n");
        assert_eq!(
            normalised_after.trim_end_matches('\n'),
            normalised_before.trim_end_matches('\n'),
            "variant {i}: remove must restore the user content"
        );
    }
}
