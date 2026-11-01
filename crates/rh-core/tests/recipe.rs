//! TDD-11 — Project scaffolding recipes.
//!
//! Refs: PRD §3.7 items 56, 57, 58.
//!
//! The recipe format is the cheapest possible plugin mechanism: one line,
//! `Name=<command or URL>`, with `%s` standing for the project name. A value that
//! looks like a URL is downloaded and unpacked; anything else is run as a
//! command. That is the whole model.

mod common;
use common::TempDir;

use rh_core::recipe::{self, Recipe, RecipeError};
use std::fs;

// ----------------------------------------------------------------- parsing

#[test]
fn parses_line_based_recipe_list() {
    let text = "\
# comment
Blank=
WordPress=https://wordpress.org/latest.tar.gz
Laravel=composer create-project laravel/laravel %s --prefer-dist
";
    let list = recipe::parse_list(text);
    assert_eq!(list.len(), 3);

    assert!(matches!(list[0].action, recipe::Action::Blank));
    assert!(matches!(list[1].action, recipe::Action::Url(_)));
    assert!(matches!(list[2].action, recipe::Action::Command(_)));
    assert_eq!(list[2].name, "Laravel");
}

#[test]
fn parse_list_skips_comments_and_blanks() {
    let text = "\n# only a comment\n\n   \nBlank=\n";
    let list = recipe::parse_list(text);
    assert_eq!(list.len(), 1, "only the real entry survives: {list:?}");
}

#[test]
fn parse_list_keeps_equality_signs_in_values() {
    let text = "Weird=run --opt=a=b %s\n";
    let list = recipe::parse_list(text);
    assert_eq!(list.len(), 1);
    match &list[0].action {
        recipe::Action::Command(c) => assert!(c.contains("--opt=a=b"), "{c}"),
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn url_recipe_is_detected() {
    for url in [
        "https://wordpress.org/latest.tar.gz",
        "http://example.com/x.zip",
    ] {
        let r = Recipe::parse_line(&format!("X={url}")).unwrap();
        assert!(matches!(r.action, recipe::Action::Url(_)), "{url}");
    }
}

#[test]
fn command_recipe_is_detected() {
    let r = Recipe::parse_line("Laravel=composer create-project laravel/laravel %s").unwrap();
    assert!(matches!(r.action, recipe::Action::Command(_)));
}

#[test]
fn blank_recipe_is_detected() {
    let r = Recipe::parse_line("Blank=").unwrap();
    assert!(matches!(r.action, recipe::Action::Blank));
}

#[test]
fn malformed_line_is_rejected_with_the_text() {
    let err = Recipe::parse_line("no equals sign here").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("no equals sign"), "error must quote input: {msg}");
}

// ------------------------------------------------------------- validation

#[test]
fn recipe_rejects_name_with_path_separators() {
    // A recipe name becomes a directory, so it must not contain a path.
    for bad in ["../evil", "a/b", "..\\evil", "a\\b"] {
        let err = Recipe::parse_line(&format!("{bad}=echo hi")).unwrap_err();
        assert!(
            matches!(err, RecipeError::UnsafeName { .. }),
            "{bad} must be rejected, got {err:?}"
        );
    }
}

#[test]
fn recipe_rejects_empty_name() {
    assert!(Recipe::parse_line("=echo hi").is_err());
}

#[test]
fn recipe_name_is_trimmed() {
    let r = Recipe::parse_line("  Spaced  =echo hi").unwrap();
    assert_eq!(r.name, "Spaced");
}

// ------------------------------------------------------- command building

#[test]
fn command_substitutes_percent_s_with_name() {
    let r = Recipe::parse_line("Laravel=composer create-project laravel/laravel %s").unwrap();
    let cmd = r.build_command("myapp").unwrap();
    assert_eq!(cmd, "composer create-project laravel/laravel myapp");
}

#[test]
fn command_without_placeholder_still_works() {
    let r = Recipe::parse_line("X=echo hello").unwrap();
    assert_eq!(r.build_command("ignored").unwrap(), "echo hello");
}

#[test]
fn command_substitutes_every_placeholder() {
    let r = Recipe::parse_line("X=cp %s %s.bak").unwrap();
    assert_eq!(r.build_command("a").unwrap(), "cp a a.bak");
}

#[test]
fn blank_recipe_builds_no_command() {
    let r = Recipe::parse_line("Blank=").unwrap();
    assert!(r.build_command("x").is_err(), "blank has nothing to run");
}

#[test]
fn url_recipe_builds_no_command() {
    let r = Recipe::parse_line("W=https://example.com/a.zip").unwrap();
    assert!(r.build_command("x").is_err(), "url recipes are downloaded");
}

// ------------------------------------------------------------ .env rewrite

#[test]
fn env_rewrite_updates_db_keys_only() {
    let d = TempDir::new("env1");
    let p = d.path().join(".env");
    fs::write(
        &p,
        "APP_NAME=Demo\nDB_DATABASE=homestead\nDB_USERNAME=homestead\nDB_PASSWORD=secret\nQUEUE=x\n",
    )
    .unwrap();

    recipe::rewrite_env(&p, "shop", "root", "").unwrap();
    let out = fs::read_to_string(&p).unwrap();

    assert!(out.contains("DB_DATABASE=shop"), "{out}");
    assert!(out.contains("DB_USERNAME=root"), "{out}");
    assert!(out.contains("DB_PASSWORD="), "{out}");
    // Untouched keys must be byte-identical.
    assert!(out.contains("APP_NAME=Demo"), "{out}");
    assert!(out.contains("QUEUE=x"), "{out}");
}

#[test]
fn env_rewrite_preserves_crlf_and_comments() {
    let d = TempDir::new("env2");
    let p = d.path().join(".env");
    fs::write(&p, "# comment\r\nDB_DATABASE=old\r\nOTHER=1\r\n").unwrap();

    recipe::rewrite_env(&p, "new", "root", "pw").unwrap();
    let out = fs::read_to_string(&p).unwrap();

    assert!(out.contains("\r\n"), "CRLF must be preserved");
    assert!(out.contains("# comment"), "comments must survive");
    assert!(out.contains("DB_DATABASE=new"));
    assert!(out.contains("OTHER=1"));
}

#[test]
fn env_rewrite_is_noop_when_key_absent() {
    let d = TempDir::new("env3");
    let p = d.path().join(".env");
    let original = "APP_NAME=Demo\n";
    fs::write(&p, original).unwrap();

    recipe::rewrite_env(&p, "shop", "root", "").unwrap();
    assert_eq!(
        fs::read_to_string(&p).unwrap(),
        original,
        "absent keys must not be invented"
    );
}

#[test]
fn env_rewrite_missing_file_is_not_an_error() {
    let d = TempDir::new("env4");
    let p = d.path().join("nope.env");
    assert!(recipe::rewrite_env(&p, "x", "root", "").is_ok());
    assert!(!p.exists(), "must not create a file that was not there");
}

#[test]
fn env_rewrite_does_not_touch_quoted_values_format() {
    // Laravel allows DB_DATABASE="name"; adding the space before '=' would break it.
    let d = TempDir::new("env5");
    let p = d.path().join(".env");
    fs::write(&p, "DB_DATABASE = \"quoted\"\n").unwrap();

    recipe::rewrite_env(&p, "fresh", "root", "").unwrap();
    let out = fs::read_to_string(&p).unwrap();
    assert!(
        out.contains("DB_DATABASE = \"fresh\""),
        "must keep the original spacing style: {out}"
    );
}

// --------------------------------------------------------- scaffolding

#[test]
fn scaffold_creates_project_directory() {
    let d = TempDir::new("sc1");
    let www = d.path().join("www");
    let r = Recipe::parse_line("Blank=").unwrap();

    let made = recipe::scaffold(&r, "demo", &www, &[]).unwrap();
    assert!(made.is_dir());
    assert!(made.ends_with("demo"));
}

#[test]
fn scaffold_refuses_existing_directory() {
    let d = TempDir::new("sc2");
    let www = d.path().join("www");
    fs::create_dir_all(www.join("demo")).unwrap();
    let r = Recipe::parse_line("Blank=").unwrap();

    let err = recipe::scaffold(&r, "demo", &www, &[]).unwrap_err();
    assert!(
        matches!(err, RecipeError::AlreadyExists { .. }),
        "must not clobber an existing project: {err:?}"
    );
}

#[test]
fn scaffold_rejects_unsafe_project_name() {
    let d = TempDir::new("sc3");
    let www = d.path().join("www");
    let r = Recipe::parse_line("Blank=").unwrap();

    for bad in ["../escape", "a/b", "..\\x"] {
        let err = recipe::scaffold(&r, bad, &www, &[]).unwrap_err();
        assert!(
            matches!(err, RecipeError::UnsafeName { .. }),
            "{bad} must be refused, got {err:?}"
        );
    }
}

#[test]
fn shipped_recipes_parse() {
    let text = include_str!("../../../recipes.toml");
    let list = recipe::parse_list(text);
    assert!(list.len() >= 3, "expected a few shipped recipes, got {}", list.len());

    let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
    for expected in ["Blank", "WordPress", "Laravel"] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
}
