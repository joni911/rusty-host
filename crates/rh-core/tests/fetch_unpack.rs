//! TDD-04 — Download, integrity verification, and safe extraction.
//!
//! Refs: PRD §4.4, §7, NFR-7, NFR-8.
//!
//! NFR-8 is the important one: **extraction must be incapable of writing outside
//! the target directory**. That is enforced by a property test at the bottom.

mod common;
use common::TempDir;

use rh_core::fetch::{sha256_file, verify};
use rh_core::unpack::{extract_zip, UnpackError};
use std::fs;
use std::io::Write;
use std::path::Path;

// ------------------------------------------------------------------ hashing

#[test]
fn sha256_matches_known_vector() {
    // sha256("abc")
    let d = TempDir::new("sha");
    let p = d.path().join("abc.txt");
    fs::write(&p, b"abc").unwrap();
    assert_eq!(
        sha256_file(&p).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn verify_accepts_matching_digest() {
    let d = TempDir::new("vok");
    let p = d.path().join("f.bin");
    fs::write(&p, b"abc").unwrap();
    assert!(verify(&p, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
}

#[test]
fn verify_is_case_insensitive() {
    let d = TempDir::new("vci");
    let p = d.path().join("f.bin");
    fs::write(&p, b"abc").unwrap();
    assert!(verify(&p, "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD").is_ok());
}

#[test]
fn verify_rejects_mismatch_and_reports_both_digests() {
    let d = TempDir::new("vbad");
    let p = d.path().join("f.bin");
    fs::write(&p, b"abc").unwrap();
    let err = verify(&p, &"0".repeat(64)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("0000000"), "error must show expected: {msg}");
    assert!(msg.contains("ba7816bf"), "error must show actual: {msg}");
}

#[test]
fn verify_rejects_malformed_digest() {
    let d = TempDir::new("vmal");
    let p = d.path().join("f.bin");
    fs::write(&p, b"abc").unwrap();
    assert!(verify(&p, "not-a-digest").is_err());
    assert!(verify(&p, "abc").is_err());
}

// ------------------------------------------------------------- extraction

/// Build a zip in memory with the given (name, contents) entries.
fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let f = fs::File::create(path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(data).unwrap();
    }
    zw.finish().unwrap();
}

#[test]
fn extracts_zip_into_target() {
    let d = TempDir::new("ex");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("hello.txt", b"hi"), ("sub/nested.txt", b"deep")]);

    let out = d.path().join("out");
    extract_zip(&z, &out).unwrap();

    assert_eq!(fs::read_to_string(out.join("hello.txt")).unwrap(), "hi");
    assert_eq!(
        fs::read_to_string(out.join("sub").join("nested.txt")).unwrap(),
        "deep"
    );
}

#[test]
fn strips_single_top_level_directory() {
    // php-8.4.27-Win32-vs17-x64/php.exe  ->  <target>/php.exe
    let d = TempDir::new("strip");
    let z = d.path().join("a.zip");
    make_zip(
        &z,
        &[
            ("pkg-1.0/readme.txt", b"r"),
            ("pkg-1.0/bin/tool.exe", b"e"),
        ],
    );

    let out = d.path().join("out");
    extract_zip(&z, &out).unwrap();

    assert!(
        out.join("readme.txt").is_file(),
        "single top-level dir must be stripped"
    );
    assert!(out.join("bin").join("tool.exe").is_file());
    assert!(!out.join("pkg-1.0").exists(), "wrapper dir must be gone");
}

#[test]
fn preserves_nested_structure_when_not_single_root() {
    let d = TempDir::new("multi");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("a/x.txt", b"1"), ("b/y.txt", b"2")]);

    let out = d.path().join("out");
    extract_zip(&z, &out).unwrap();

    assert!(out.join("a").join("x.txt").is_file());
    assert!(out.join("b").join("y.txt").is_file());
}

#[test]
fn rejects_absolute_path_entry() {
    let d = TempDir::new("abs");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("/etc/passwd", b"evil")]);

    let out = d.path().join("out");
    let err = extract_zip(&z, &out).unwrap_err();
    assert!(
        matches!(err, UnpackError::UnsafePath { .. }),
        "got {err:?}"
    );
    assert!(
        !Path::new("/etc/passwd").starts_with(&out),
        "sanity: target must not be a prefix of an absolute escape"
    );
}

#[test]
fn rejects_parent_traversal_entry() {
    let d = TempDir::new("trav");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("../../evil.txt", b"evil")]);

    let out = d.path().join("out");
    let err = extract_zip(&z, &out).unwrap_err();
    assert!(matches!(err, UnpackError::UnsafePath { .. }), "got {err:?}");
}

#[test]
fn rejects_windows_backslash_traversal() {
    let d = TempDir::new("trav2");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("..\\..\\evil.txt", b"evil")]);

    let out = d.path().join("out");
    let err = extract_zip(&z, &out).unwrap_err();
    assert!(matches!(err, UnpackError::UnsafePath { .. }), "got {err:?}");
}

#[test]
fn names_the_offending_entry_in_error() {
    let d = TempDir::new("name");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("../../escape-me.txt", b"x")]);

    let out = d.path().join("out");
    let err = extract_zip(&z, &out).unwrap_err();
    assert!(
        err.to_string().contains("escape-me.txt"),
        "error must name the entry: {err}"
    );
}

#[test]
fn nothing_is_written_when_first_entry_is_unsafe() {
    // The target must not be left half-populated.
    let d = TempDir::new("nocreate");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("../bad.txt", b"x"), ("good.txt", b"ok")]);

    let out = d.path().join("out");
    assert!(extract_zip(&z, &out).is_err());
    // Either the dir was never created, or it is empty — never half-written.
    if out.exists() {
        let n = fs::read_dir(&out).unwrap().count();
        assert_eq!(n, 0, "partial extraction must be rolled back");
    }
}

#[test]
fn extract_is_idempotent_on_success() {
    let d = TempDir::new("idem");
    let z = d.path().join("a.zip");
    make_zip(&z, &[("f.txt", b"v1")]);

    let out = d.path().join("out");
    extract_zip(&z, &out).unwrap();
    // Re-extracting over an existing dir must succeed and overwrite.
    extract_zip(&z, &out).unwrap();
    assert_eq!(fs::read_to_string(out.join("f.txt")).unwrap(), "v1");
}

#[test]
fn rejects_entry_escaping_via_symlink_target() {
    // A symlink whose target points outside the destination must be refused.
    let d = TempDir::new("link");
    let z = d.path().join("a.zip");
    let f = fs::File::create(&z).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::FileOptions::default();
    zw.add_symlink("escape", "/etc/passwd", opts).unwrap();
    zw.finish().unwrap();

    let out = d.path().join("out");
    let err = extract_zip(&z, &out).unwrap_err();
    assert!(
        matches!(err, UnpackError::UnsafePath { .. } | UnpackError::UnsupportedEntry { .. }),
        "symlinks must not be extracted: got {err:?}"
    );
}

// ------------------------------------------------- property tests (NFR-8)

/// Generate adversarial entry names that try to escape the destination.
fn adversarial_names() -> Vec<String> {
    let mut out = vec![
        "../x".to_string(),
        "../../x".to_string(),
        "../../../etc/passwd".to_string(),
        "..\\x".to_string(),
        "..\\..\\windows\\system32\\x".to_string(),
        "/abs/path".to_string(),
        "\\abs\\path".to_string(),
        "C:\\Windows\\x".to_string(),
        "c:/windows/x".to_string(),
        "a/../../b".to_string(),
        "./../x".to_string(),
        "\\\\server\\share\\x".to_string(),
        "..".to_string(),
        "a/..".to_string(),
        "\\..\\..\\x".to_string(),
    ];
    // mixed separators
    out.push("a\\..\\..\\x".to_string());
    out.push("a/..\\..\\x".to_string());
    out.push("....//....//x".to_string());
    out.push("%2e%2e%2fx".to_string()); // literal, should be treated as a name
    out
}

#[test]
fn property_no_escape_for_any_adversarial_name() {
    // For every adversarial name: either extraction is refused, or whatever was
    // written stays inside `out`. It must never touch the parent directory.
    for name in adversarial_names() {
        let d = TempDir::new("prop");
        let z = d.path().join("a.zip");
        make_zip(&z, &[(name.as_str(), b"payload")]);

        let out = d.path().join("anchor").join("out");
        fs::create_dir_all(out.parent().unwrap()).unwrap();

        let _ = extract_zip(&z, &out);

        // Nothing may appear next to (outside) the destination.
        let siblings: Vec<_> = fs::read_dir(out.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "out")
            .collect();
        assert!(
            siblings.is_empty(),
            "entry {name:?} escaped the destination; found {siblings:?}"
        );
    }
}

#[test]
fn property_valid_names_always_extract() {
    // Multi-entry archives so the single-wrapper-directory rule does not apply.
    for name in ["a.txt", "dir/a.txt", "dir/sub/deep.txt", "with space.txt", "üñï.txt"] {
        let d = TempDir::new("propok");
        let z = d.path().join("a.zip");
        make_zip(&z, &[(name, b"ok"), ("second.txt", b"2")]);
        let out = d.path().join("out");
        extract_zip(&z, &out).unwrap_or_else(|e| panic!("{name} should extract: {e}"));
        let on_disk = out.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(on_disk.is_file(), "{name} missing after extraction");
    }
}
