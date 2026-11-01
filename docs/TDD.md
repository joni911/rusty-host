# TDD — Rusty Host

**Test-Driven Development plan** for the implementation described in `PRD.md`.

Rules of engagement:

1. **Red → Green → Refactor.** No production line is written before a failing test
   exists for it.
2. One commit per red-green cycle, message `test:` then `feat:` (or `fix:`).
3. Tests are **fast** (whole suite < 10 s), **hermetic** (no network, no real
   downloads), and **deterministic**.
4. Network/version-resolution code is tested against **recorded fixtures**, never
   live upstream.
5. Real-download and real-process paths are tagged `#[ignore]` and run in a
   dedicated `--features live` job.

---

## 0. Test taxonomy

| Layer | Location | Speed | Network | Purpose |
|---|---|---|---|---|
| Unit | `crates/rh-core/src/**/tests` (inline) | ms | no | pure logic |
| Contract | `crates/rh-core/tests/contract_*.rs` | ms | no | descriptor parsing, strategies vs fixtures |
| Integration | `crates/rh-cli/tests/*.rs` | 10–100 ms | no | CLI end-to-end against a temp workspace |
| Live | `tests/live/*.rs` (`#[ignore]`) | minutes | **yes** | real downloads, real service start |
| Bench | `benches/*.rs` (criterion) | s | no | NFR-1…3 |

Golden-file policy: any function producing text (config render, hosts block, table
output) is tested by **snapshot**, with snapshots committed under `tests/snapshots/`.

---

## 1. TDD-01 — Workspace layout & discovery

**FR:** PRD §4.2, NFR-9, NFR-11

### Tests (write first)

```rust
// crates/rh-core/src/workspace.rs

#[test] fn init_creates_exact_directory_set()
// asserts bin/ conf/ data/ tmp/ www/ and rh.toml all exist

#[test] fn init_is_idempotent_and_preserves_existing_rh_toml()
// init twice; second run must not overwrite a user-edited rh.toml

#[test] fn discover_walks_up_to_nearest_workspace_root()
// root/  root/sub/deep/  -> discover(deep) == root

#[test] fn discover_fails_cleanly_outside_any_workspace()
// returns Err(WorkspaceNotFound), never panics

#[test] fn all_paths_are_relative_to_root()
// after init, rh.toml must contain zero absolute paths

#[test] fn workspace_survives_relocation()
// init + install fake component, copy whole tree to new temp dir,
// reopen -> index loads, component dir resolves, no absolute path leaks
```

### Implementation notes
- `Workspace { root: PathBuf }`, everything else derived.
- `rh.toml` contains only relative paths and settings.
- Acceptance gate: `workspace_survives_relocation` is the NFR-9 proof; it must fail
  loudly if any absolute path sneaks in.

---

## 2. TDD-02 — Descriptor parsing (`components/*.toml`)

**FR:** PRD §4.3 — the extensibility core

### Tests

```rust
// crates/rh-core/src/registry.rs

#[test] fn parses_minimal_runtime_descriptor()
#[test] fn parses_database_descriptor_with_html_scrape()
#[test] fn parses_tool_descriptor_with_github_release()
#[test] fn rejects_descriptor_missing_name()
#[test] fn rejects_unknown_resolve_strategy()
// error must name the offending value and the file path

#[test] fn rejects_descriptor_with_empty_artifacts()
#[test] fn every_shipped_descriptor_parses()
// iterate components/*.toml -> all must validate (guards the shipped set)

#[test] fn descriptor_requires_no_code_to_add_a_component()
// simulate: write a brand-new components/foo.toml in a temp workspace,
// load it, assert it appears in the catalogue with zero Rust changes
```

### Snapshot
`tests/snapshots/catalogue.txt` — sorted list of shipped component names + kinds.

---

## 3. TDD-03 — Version resolution against fixtures

**FR:** PRD §4.4, NFR-12

**Rules:** never hit the network. Every strategy gets a recorded fixture.

```
tests/fixtures/
├── php_releases.json          (trimmed real response, 3 versions)
├── apache_lounge.html         (trimmed listing page)
├── nginx_download.html        (trimmed listing page)
├── mariadb_archive.html       (trimmed listing page)
├── github_dbx.json            (trimmed releases API payload)
└── github_mailpit.json
```

### Tests

```rust
#[test] fn php_resolver_picks_latest_patch_for_minor()
// input "8.4" against php_releases.json -> "8.4.27"

#[test] fn php_resolver_picks_latest_when_no_version_given()

#[test] fn php_resolver_prefers_ts_vs17_x64_variant()
// asserts chosen URL ends with php-8.4.27-1-Win32-vs17-x64.zip

#[test] fn apache_resolver_parses_lounge_listing()
// -> 2.4.69 + correct relative path

#[test] fn nginx_resolver_parses_download_page()

#[test] fn mariadb_resolver_prefers_lts()
// given 11.8 (LTS) and 13.0 (stable non-LTS) both present -> 11.8

#[test] fn github_resolver_matches_asset_by_pattern()
// dbx -> DBX_0.6.62_x64-portable.zip  (NOT the .exe, NOT .deb)

#[test] fn resolver_returns_exact_version_when_fully_specified()
// "8.4.27" -> 8.4.27, no "newest" logic applied

#[test] fn resolver_fails_with_actionable_error_when_version_absent()
// "9.9" -> Err listing available majors

#[test] fn resolver_is_deterministic()
// same fixture, 10 runs -> identical result (no hashmap ordering leak)

#[test] fn malformed_upstream_payload_yields_no_partial_result()
// truncated JSON -> Err, not an empty/默认 version
```

### Fixture policy
Record with a tiny helper binary (`xtask record <component>`) which saves the raw
body **and** the parsed expectation. Refresh fixtures quarterly or when
`rh doctor` reports resolver drift.

---

## 4. TDD-04 — Download, verify, unpack

**FR:** PRD §4.4, §7, NFR-7, NFR-8

### Tests

```rust
// crates/rh-core/src/fetch.rs

#[test] fn verify_accepts_matching_sha256()
#[test] fn verify_rejects_mismatch_and_deletes_artifact()
#[test] fn verify_is_case_insensitive()
#[test] fn verify_rejects_malformed_digest()
#[test] fn verify_reports_both_digests_in_error()
// error text must contain expected and actual

// crates/rh-core/src/unpack.rs
#[test] fn extracts_zip_into_target()
#[test] fn strips_single_top_level_directory()
#[test] fn preserves_nested_structure_when_not_single_root()

#[test] fn rejects_absolute_path_entry()          // "/etc/passwd"
#[test] fn rejects_parent_traversal_entry()       // "../../evil"
#[test] fn rejects_windows_backslash_traversal()  // "..\\..\\evil"
#[test] fn rejects_symlink_escaping_target()
#[test] fn names_the_offending_entry_in_error()

#[test] fn partial_extract_is_rolled_back_on_failure()
// simulate failure mid-extract -> bin/<comp>/<ver> must not exist afterwards

#[test] fn extract_is_idempotent_on_success()
```

### Property tests (proptest)
```rust
#[proptest] fn unpack_never_writes_outside_target(arbitrary_zip_entries)
// the single most important safety property (NFR-8)
```

### Live tests (`#[ignore]`)
```rust
#[test] #[ignore] fn downloads_real_php_and_verifies()
#[test] #[ignore] fn downloads_real_dbX_portable()
```

---

## 5. TDD-05 — Template rendering

**FR:** PRD §3.1 item 6, §4.3

### Tests

```rust
// crates/rh-core/src/template.rs

#[test] fn replaces_known_tokens()
#[test] fn leaves_unknown_tokens_verbatim()
// so a broken template is visible, never silently wrong

#[test] fn token_values_are_not_reparsed()
// "{{a}}" as a *value* must survive: render("<<X>>", x="{{a}}") == "{{a}}"
// (prevents injection-style double expansion — a real Laragon-class bug)

#[test] fn paths_are_normalised_to_forward_slashes()
// Apache/Nginx on Windows accept both, but forward slashes are echo-safe

#[test] fn windows_drive_paths_render_correctly()
// C:\x\y -> C:/x/y, never C:\x\y inside a Lua/JSON-adjacent context

#[test] fn render_is_deterministic_across_runs()
```

### Snapshot
```rust
#[test] fn renders_apache_vhost_golden()
#[test] fn renders_nginx_server_golden()
#[test] fn renders_php_ini_golden()
#[test] fn renders_my_ini_golden()
```
Snapshots: `tests/snapshots/vhost_apache.conf`, `vhost_nginx.conf`, `php.ini`, `my.ini`.

---

## 6. TDD-06 — Project scan & slugging

**FR:** PRD §3.2 items 14, 18, 59

### Tests

```rust
#[test] fn scans_www_and_ignores_hidden_and_files()
#[test] fn slugifies_spaces_underscores_and_unicode()
// "My Shop" -> my-shop ; "absen_smartwork_hpanel" -> absen-smartwork-hpanel
#[test] fn slugify_never_produces_empty()
// "" and "!!!" -> "app"
#[test] fn slugify_is_dns_safe()
// only [a-z0-9-], no leading/trailing dash, <= 63 chars
#[test] fn detects_laravel_style_public_dir()
// www/x/public + artisan -> document_root = public/
#[test] fn detects_public_html()
#[test] fn falls_back_to_project_root()
#[test] fn custom_tld_is_respected()
// --tld local -> demo.local
```

---

## 7. TDD-07 — Hosts file: the safety-critical module

**FR:** PRD §4.5, §7, NFR-6

This module can damage a system file. It gets the most tests.

### Tests

```rust
// crates/rh-core/src/hosts.rs

// --- managed-block isolation ---
#[test] fn appends_managed_block_with_markers()
#[test] fn preserves_all_user_lines_byte_for_byte()
// input unchanged except for our block; naive line-splitting on CRLF must not mangle
#[test] fn preserves_crlf_vs_lf_of_existing_file()
#[test] fn re_sync_replaces_block_without_duplicating()
// run twice -> exactly one managed block
#[test] fn removes_block_leaving_user_content_intact()
#[test] fn handles_file_without_trailing_newline()
#[test] fn handles_block_at_end_of_file()
#[test] fn handles_empty_hosts_file()

// --- atomicity ---
#[test] fn writes_to_tmp_then_replaces()
// assert original is untouched until the final rename/copy
#[test] fn leaves_original_intact_when_replacement_fails()
// simulate failure at write step -> original hosts unchanged
#[test] fn never_leaves_partial_block()
// property: for any failure point, result is either old or new, never mixed

// --- failure handling (copy Laragon's honesty) ---
#[test] fn permission_denied_returns_actionable_error()
// error contains: the path, "read-only", "antivirus", and the disable notice
#[test] fn permission_denied_disables_auto_hosts_in_settings()
#[test] fn permission_denied_cleans_previously_managed_entries()
#[test] fn permission_denied_never_attempts_elevation()
// assert no process spawn / no shell execute path is taken

// --- read-only probe ---
#[test] fn probe_reports_readonly_and_security_bits()
```

### Property tests
```rust
#[proptest] fn hosts_roundtrip_is_stable(arbitrary_user_content, arbitrary_entries)
// write(entries) -> parse -> write(entries) is a fixed point
#[proptest] fn managed_block_never_eats_user_content(...)
```

### Live (`#[ignore]`)
```rust
#[test] #[ignore] fn edits_real_hosts_when_permitted()
```

---

## 8. TDD-08 — Vhost rendering & sync

**FR:** PRD §3.2 items 16, 17, 19

### Tests

```rust
#[test] fn link_creates_apache_conf_and_returns_path()
#[test] fn link_is_idempotent()
#[test] fn link_updates_existing_conf_when_document_root_changes()
#[test] fn sync_prunes_conf_for_deleted_project()
#[test] fn sync_never_deletes_hand_written_conf()
// a conf without our `# managed-by: rusty-host` header is left alone
#[test] fn sync_dry_run_reports_without_touching_disk()
#[test] fn ssl_variant_includes_cert_directives()
#[test] fn nginx_variant_emits_server_block()
#[test] fn conflicting_slug_for_two_folders_is_reported()
// "My Shop" and "my_shop" both -> my-shop : must error, not silently overwrite
```

---

## 9. TDD-09 — Port probing & conflict

**FR:** PRD §3.3 items 29, 30

### Tests

```rust
#[test] fn free_port_is_reported_free()
#[test] fn bound_port_is_reported_in_use()
#[test] fn conflict_error_names_the_owning_process_when_available()
#[test] fn conflict_error_suggests_next_free_port()
#[test] fn preflight_checks_all_enabled_services()
```

---

## 10. TDD-10 — Process supervision

**FR:** PRD §4.6, §3.3 items 26–28, 31

### Tests

```rust
// crates/rh-core/src/proc.rs

#[test] fn spawn_records_pid_and_metadata()
#[test] fn second_start_is_a_noop_when_running()
#[test] fn stop_terminates_and_removes_pid_file()
#[test] fn stale_pid_file_is_detected_and_cleaned()
#[test] fn pid_reuse_is_detected_by_exe_path()
// pid file claims 1234, live 1234 is a *different* exe -> treat as stale,
// must NOT kill it. This is the dangerous case.
#[test] fn start_waits_for_port_before_reporting_ready()
#[test] fn start_times_out_with_log_tail_on_failure()
// failure error must include last N lines of the service's stderr
#[test] fn reload_sends_signal_not_restart()
#[test] fn outputs_are_captured_to_data_logs()
```

### Live (`#[ignore]`)
```rust
#[test] #[ignore] fn starts_and_stops_real_mariadb()
#[test] #[ignore] fn kill9_then_start_cleans_orphan()
```

---

## 11. TDD-11 — Recipe / project scaffolding

**FR:** PRD §3.7

### Tests

```rust
#[test] fn parses_line_based_recipe_list()
// "WordPress=https://..." and "Laravel=composer create-project ... %s"
#[test] fn url_recipe_is_downloaded_and_unpacked()
#[test] fn command_recipe_substitutes_percent_s_with_name()
#[test] fn recipe_rejects_name_with_path_separators()
#[test] fn env_rewrite_updates_db_keys_only()
// DB_DATABASE/DB_USERNAME/DB_PASSWORD replaced; other keys byte-identical
#[test] fn env_rewrite_preserves_crlf_and_comments()
#[test] fn env_rewrite_is_noop_when_key_absent()
```

---

## 12. TDD-12 — CLI surface (integration)

**FR:** PRD §5

Uses `assert_cmd` + `tempfile`. Each test gets a fresh workspace.

```rust
#[test] fn init_then_list_shows_empty()
#[test] fn list_json_is_stable_schema()          // snapshot
#[test] fn unknown_component_exits_1_with_suggestion()
#[test] fn network_failure_exits_2()
#[test] fn permission_denied_exits_3()
#[test] fn port_conflict_exits_4()
#[test] fn every_command_supports_json()
// iterate the clap command tree, assert --json accepted
#[test] fn help_lists_all_subcommands()          // snapshot
#[test] fn cli_never_prints_to_stdout_on_error()
// errors go to stderr; machine-parseable
```

### Exit-code contract test (important)
```rust
#[test] fn exit_codes_match_documentation()
// single source of truth: an enum used by both CLI and this test
```

---

## 13. TDD-13 — Portability guarantee (NFR-9)

The requirement that makes this product what it is.

```rust
#[test] fn relocate_workspace_and_keep_working()
// 1. init workspace at A
// 2. install a FAKE component (local fixture archive, no network)
// 3. link a project
// 4. move entire tree A -> B
// 5. reopen at B: list, status, link all succeed
// 6. assert generated configs contain B's paths, not A's

#[test] fn no_absolute_paths_in_rh_toml_or_lock()
#[test] fn generated_configs_are_rewritten_after_relocation()
```

---

## 14. TDD-14 — Non-functional gates (CI)

```rust
// tests/nfr.rs — runs in CI, fails the build
#[test] fn cli_binary_under_4_mb()
#[test] fn gui_binary_under_15_mb()
```
```rust
// benches/startup.rs (criterion)
// NFR-2: median startup < 15 ms -> assert in CI with a generous 40 ms ceiling
// NFR-3: peak RSS < 12 MB via peak-RSS sampler
```

CI steps:
```
cargo fmt --check
cargo clippy -- -D warnings
cargo test                     # hermetic suite
cargo test --features live     # nightly/on-demand only
cargo bench --test             # NFR gates
cargo deny check               # licence + advisory audit
```

---

## 15. Implementation order (red-green cycles)

Each row is one PR. Do not start a row until the previous row is green.

| # | Cycle | Tests written | Production code |
|---|---|---|---|
| 1 | Workspace skeleton | TDD-01 (6) | `workspace.rs`, `rh init`, `rh doctor` |
| 2 | Descriptors | TDD-02 (7) | `registry.rs` parse + catalogue |
| 3 | Resolvers | TDD-03 (11) | 4 strategies + fixtures |
| 4 | Fetch & verify | TDD-04 (7) | `fetch.rs` |
| 5 | Unpack & safety | TDD-04 (9 + proptest) | `unpack.rs` |
| 6 | Templates | TDD-05 (9) | `template.rs` + snapshots |
| 7 | Scan & slugs | TDD-06 (8) | scan + slug + docroot |
| 8 | **Hosts** | TDD-07 (17 + proptest) | `hosts.rs` — highest scrutiny |
| 9 | Vhosts & sync | TDD-08 (9) | `vhost.rs` |
| 10 | Ports | TDD-09 (5) | `ports.rs` |
| 11 | Processes | TDD-10 (9) | `proc.rs` |
| 12 | Recipes | TDD-11 (7) | `recipe.rs` |
| 13 | CLI wiring | TDD-12 (10) | `rh-cli` |
| 14 | Portability | TDD-13 (3) | fixes for anything caught |
| 15 | GUI | GUI tests (6) | `rh-gui` red theme |
| 16 | NFR gates | TDD-14 | size/perf tuning |

**Total: ~124 tests before the GUI.** Roughly 25 are property/snapshot tests.

---

## 16. GUI test plan (light, but real)

GUI is thin by design, so tests focus on **model**, not pixels.

```rust
#[test] fn service_state_maps_to_icon_colour()
#[test] fn tray_action_calls_core_not_shell()
// assert actions are dispatched through rh-core functions
#[test] fn theme_constants_match_rust_brand()
// #CE422B etc., so the red is verifiable not vibes
#[test] fn app_renders_without_workspace()
// graceful "not initialised" state
```

Plus one **manual** smoke checklist per release:
1. Fresh extract → `rh init` → GUI opens with "not installed" state
2. Install PHP → GUI shows it
3. Start all → dots green, `http://demo.test` works
4. Stop all → dots grey
5. Kill GUI → services keep running (documented behaviour)

---

## 17. Definition of Done (per PR)

- [ ] Failing test committed **first** (visible in history)
- [ ] Test passes; no other test broken
- [ ] `cargo fmt` + `clippy -D warnings` clean
- [ ] Public items documented (`///`)
- [ ] Snapshot updated only with justification in the commit body
- [ ] No `unwrap()`/`expect()` on user-input paths
- [ ] Error messages name the thing that failed and what to do

## 18. Definition of Done (MVP)

- [ ] All P0 features implemented and tested
- [ ] ~124 hermetic tests green
- [ ] Live suite green on a clean Windows 10 VM
- [ ] NFR-1…3 measured and passing
- [ ] Fresh-machine smoke test (PRD §9) passes with **no UAC prompt**
- [ ] Relocation test passes
- [ ] `rh doctor` green on a machine with nothing installed
- [ ] README with 60-second quickstart

---

## 19. Traceability matrix

| PRD feature # | Coverage |
|---|---|
| 1,2,3,5 | TDD-01, TDD-02 |
| 4, 50, 51 | TDD-03, TDD-04, TDD-05 |
| 6, 39 | TDD-05 |
| 14, 18, 59 | TDD-06 |
| 15, 20, 21 | **TDD-07** |
| 16, 17, 19 | TDD-08 |
| 29, 30, 31 | TDD-09 |
| 26, 27, 28 | TDD-10 |
| 41, 43, 45, 48, 49 | TDD-04 (install) + TDD-10 (start) + live |
| 47 | TDD-03 (`github-resolver`), TDD-04 |
| 56, 57, 58 | TDD-11 |
| 65, 66 | TDD-12 |
| NFR-1…3 | TDD-14 |
| NFR-7, NFR-8 | TDD-04 (incl. proptest) |
| NFR-9 | **TDD-13** |
| NFR-6, §7 safety | TDD-07 (`never_attempts_elevation`) |

---

*Companion document: `PRD.md`.*
