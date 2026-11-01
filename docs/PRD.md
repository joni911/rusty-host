# PRD — Rusty Host

**A portable, always-current local web development environment in Rust.**

| | |
|---|---|
| **Product** | Rusty Host |
| **Repository** | https://github.com/joni911/rusty-host |
| **Binary** | `rusty-host` (`rh`) |
| **Version** | 0.1.0 (MVP) |
| **Status** | Draft — awaiting approval |
| **Language** | Rust 2021, edition 2021, MSRV 1.80 |
| **Platforms** | Windows 10/11 x64 (MVP), Linux/macOS (Phase 3) |
| **Licence** | MIT |
| **Author** | joni911 |

---

## 1. Vision

> **One folder. Double-click. Full PHP stack. No installer, no admin, no stale components.**

Rusty Host is a single-binary local development environment: PHP, a web server, a
database, database admin, and mail capture — all portable, all downloaded fresh from
upstream on demand, all managed from a tiny CLI plus a red Rust-themed tray GUI.

### Problem

Laragon solved portable local dev well, but is architecturally frozen:

| Pain | Evidence |
|---|---|
| Bundled components go stale | Shipped PHP 8.4.12 / MySQL 8.0.30 / Apache 2.4.54 — MySQL is ~4 years old |
| Adding a tool needs source changes | Every component has hardcoded `Mnu*`/`CheckBox*`/`Edit*Port` wiring |
| GUI-only, not scriptable | No CLI surface in the binary |
| Windows-only | Delphi VCL throughout |
| Last free version is final | Laragon 7.x requires a paid licence |
| Heavy DB tool | HeidiSQL is a large Delphi app |

### Solution

| Rusty Host approach | Result |
|---|---|
| **Live version resolution** from upstream at install time | Never stale |
| **Declarative `component.toml`** | Add a tool with zero code change |
| **CLI-first**, GUI is a thin shell | Scriptable; CI-able |
| **SHA-256 verified** before extraction | Supply-chain safety |
| **Portable by design** | Copy the folder anywhere; no registry, no service install |
| **`dbx` portable** (42 MB, Rust) instead of HeidiSQL | Small, modern, multi-DB |

---

## 2. Goals & non-goals

### Goals (MVP)

1. `rh init` creates a portable workspace in any folder.
2. `rh install php@8.4` resolves the **latest** matching release from upstream,
   downloads, verifies SHA-256, extracts.
3. `rh link <project>` writes a `.test` vhost + hosts entry (atomic), idempotent.
4. `rh start` / `rh stop` / `rh status` manage server + database processes.
5. `rh upgrade` checks all components for newer versions and installs on confirm.
6. Red Rust-themed GUI tray: service toggles + open-in-browser.
7. **phpMyAdmin** auto-installed and served at `http://localhost:8080`.
8. Works with **zero admin rights** for everything except hosts-file editing, which
   degrades gracefully.

### Non-goals (MVP)

- Redis / Memcached / MongoDB / PostgreSQL / Node / Python / Go / Java tooling
  (Phase 3 — the component descriptor system must make these cheap, but they are not built now)
- Windows services mode
- macOS/Linux support
- Bundled Composer/Git
- IDE integrations
- Multi-user / shared installs
- Mail *sending* (only capture)

---

## 3. Feature inventory — Laragon parity matrix

Derived from the binary analysis (`docs/laragon-architecture.md`). Every Laragon
feature is listed with an explicit decision. **Scope discipline: MVP ships the
P0 column only.**

### 3.1 Core environment

| # | Laragon feature | Rusty Host | Priority | Notes |
|---|---|---|---|---|
| 1 | Portable folder layout (`bin/ etc/ data/ www/`) | ✅ Same idea, cleaner names | **P0** | Proven design |
| 2 | `usr/laragon.ini` settings file | ✅ `rh.toml` | **P0** | TOML not INI |
| 3 | Multi-version components (one dir per version) | ✅ Same | **P0** | |
| 4 | Switch PHP/Apache/MySQL version from menu | ✅ `rh use php@8.3` | **P0** | Per-project pinning too |
| 5 | `usr/` overrides `bin/` for templates | ✅ `conf/tpl/` overrides built-ins | **P0** | User customisation survives upgrades |
| 6 | Template engine `<<TOKEN>>` | ✅ Same, but with **escape-safe** substitution | **P0** | |
| 7 | Process `Procfile.bat` hook | ✅ `hooks.toml` (lifecycle commands) | **P1** | Safer than raw .bat |
| 8 | Windows-service mode | ❌ Out of scope | — | Breaks portability |
| 9 | Bundled component versions | ❌ Replaced by live resolution | — | Core differentiator |
| 10 | Auto-create database for new project | ✅ `rh link --db` | **P1** | Rewrite `.env` like Laragon |
| 11 | Auto-add to user PATH | ✅ `rh env --install` (opt-in, reversible) | **P1** | |
| 12 | Run at Windows startup | ✅ Shortcut creation (opt-in) | **P2** | No registry service |
| 13 | Multi-language UI | ❌ English only (MVP) | — | |

### 3.2 Auto-virtual-host (the signature feature)

| # | Laragon behaviour | Rusty Host | Priority |
|---|---|---|---|
| 14 | Folder in `www/` → auto hostname `.test` | ✅ | **P0** |
| 15 | Write Windows `hosts` file | ✅ **atomic** (temp + rename) | **P0** |
| 16 | Render Apache vhost from template | ✅ | **P0** |
| 17 | Render Nginx server block from template | ✅ | **P1** |
| 18 | Detect new project, prompt to create vhost | ✅ | **P0** |
| 19 | Prune vhosts when folder deleted | ✅ `rh sync --prune` | **P0** |
| 20 | Self-disable on permission failure | ✅ **copy this exactly** | **P0** |
| 21 | Manual "edit hosts file" escape hatch | ✅ `rh hosts --edit` | **P1** |
| 22 | Wildcard DNS (`*.test` without hosts edits) | ✅ optional via bundled DNS resolver | **P2** |
| 23 | Custom TLD (`.local`, `.localhost`) | ✅ `--tld` | **P1** |
| 24 | SSL per-site (self-signed / mkcert) | ✅ `rh ssl --enable` | **P1** |
| 25 | Trust-store automation | ⚠️ Document manual step; no silent cert install | **P2** |

### 3.3 Services

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 26 | Start/stop all services | ✅ `rh start` / `rh stop` | **P0** |
| 27 | Start/stop individual service | ✅ `rh start apache` | **P0** |
| 28 | Reload (graceful) vs restart | ✅ `rh reload` — distinct verb, like Laragon | **P0** |
| 29 | Port configuration per service | ✅ `rh.toml [ports]` | **P0** |
| 30 | Port-conflict detection | ✅ pre-flight probe | **P0** |
| 31 | Orphan process cleanup after crash | ✅ PID file + validate-on-start | **P0** |
| 32 | Service log viewer | ✅ `rh logs <svc> --follow` | **P1** |
| 33 | Tray icon with service state | ✅ GUI | **P1** |
| 34 | Auto-start services on launch | ✅ `[boot] autostart` | **P1** |

### 3.4 Web servers

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 35 | Apache with mod_php / FCGI | ✅ Apache + PHP-FPM | **P0** |
| 36 | Nginx + PHP-FPM | ✅ | **P1** |
| 37 | Switch Apache ↔ Nginx | ✅ `rh use server=nginx` | **P1** |
| 38 | Apache alias dirs (`etc/apache2/alias`) | ✅ `conf/apache/alias/` | **P1** |
| 39 | Per-version Apache config dir | ✅ | **P0** |
| 40 | Dot-folder-friendly (`.test` on `.htaccess`) | ✅ default `AllowOverride All` | **P0** |

### 3.5 Database

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 41 | MySQL / MariaDB | ✅ **MariaDB** (portable zip, no installer) | **P0** |
| 42 | Set root password | ✅ `rh db --set-password` | **P1** |
| 43 | Create database | ✅ `rh db --create <name>` | **P0** |
| 44 | Reset root password | ✅ documented recovery | **P2** |
| 45 | MySQL datadir under `data/` | ✅ | **P0** |
| 46 | **HeidiSQL** bundled | ❌ **Replaced by `dbx`** | — | Per requirement |
| 47 | — | ✅ **`dbx` portable** (42 MB, Rust, 100+ DBs) | **P0** | Downloader + launcher |
| 48 | — | ✅ **phpMyAdmin** at `localhost:8080` | **P0** | Explicit requirement |
| 49 | MySQL / MariaDB version switch | ✅ | **P1** |

### 3.6 PHP

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 50 | Multi-version PHP | ✅ | **P0** |
| 51 | php.ini per version | ✅ template-generated | **P0** |
| 52 | Enable/disable extensions | ✅ `rh php --ext` | **P1** |
| 53 | PHP error log | ✅ | **P1** |
| 54 | PHP-FPM process count tuning | ✅ `[php] workers` | **P2** |
| 55 | `php -S` built-in server mode | ✅ `rh start php --dev` | **P2** |

### 3.7 Project scaffolding

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 56 | `create-project` menu (Laravel/Lumen/WordPress) | ✅ `rh new <name> --from <recipe>` | **P1** |
| 57 | `sites.manifest.conf` line format `Name=<cmd or URL>` | ✅ `recipes.toml` + same URL/command duality | **P1** |
| 58 | Auto-rewrite `.env` (`DB_DATABASE=` etc.) | ✅ | **P1** |
| 59 | Detect framework, set DocumentRoot to `public/` | ✅ | **P0** |

### 3.8 Mail

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 60 | MailCatcher (capture) | ✅ **Mailpit** (Rust, 1 binary, modern UI) | **P1** |
| 61 | Mail sender (`sendmail.exe` replacement) | ⚠️ Configure PHP to SMTP→Mailpit | **P2** |
| 62 | Mail analyzer UI | ✅ Mailpit's built-in web UI | **P1** |

### 3.9 Tooling / misc

| # | Laragon | Rusty Host | Priority |
|---|---|---|---|
| 63 | Embedded console (cmder) | ✅ **Replaced by `rh-term`** — portable, component-aware, multi-window | **P0** | See PRD §12 |
| 64 | ngrok / Quick Cloud Server | ❌ Out of scope (privacy, account needed) | — |
| 65 | NetStat / port viewer | ✅ `rh ports` | **P1** |
| 66 | Path variables editor | ✅ `rh env` | **P1** |
| 67 | ngrok-style sharing | ❌ | — |
| 68 | WinSCP / PuTTYgen / Notepad++ / Sublime integration | ❌ | — |
| 69 | Node/Python/Go/Java/Ruby version menus | ⏳ Phase 3 via descriptors | — |
| 70 | SSL certificate manager GUI | ✅ `rh ssl` CLI | **P1** |

### MVP scope summary

**P0 (must ship):** items 1,2,3,4,5,6,14,15,16,18,19,20,26,27,28,29,30,31,35,39,40,
41,43,45,47,48,50,51,59 — **29 features**

**P1 (ship if time):** 7,10,11,17,21,23,24,32,33,34,36,37,38,42,49,52,53,56,57,58,60,62,65,66,70 — **25 features**

**P2 / Phase 3:** 7 features

**Out of scope (deliberate):** 9 features — Windows-service mode, bundled component
versions, multi-language UI, HeidiSQL, embedded console, ngrok/Quick Cloud, site
sharing, WinSCP/PuTTY/editor integrations, and the Node/Python/Go/Java/Ruby version
menus (deferred to Phase 3 via descriptors).

*29 + 25 + 7 + 9 = 70 features total.*

---

## 4. Architecture

### 4.1 Crate layout

```
rusty-host/
├── Cargo.toml                  workspace
├── crates/
│   ├── rh-core/                library: all logic, no I/O policy
│   │   ├── workspace.rs        layout, discovery, index
│   │   ├── registry.rs         component catalogue + version resolution
│   │   ├── fetch.rs            HTTP download + SHA-256 verify
│   │   ├── unpack.rs           zip/tar extract with path-traversal guard
│   │   ├── template.rs         <<TOKEN>> render
│   │   ├── vhost.rs            project scan, hosts+server rendering
│   │   ├── hosts.rs            atomic hosts-file edit, permission probe
│   │   ├── proc.rs             spawn/track/stop children, PID validation
│   │   ├── ports.rs            pre-flight bind probe
│   │   └── recipe.rs           project scaffolding
│   ├── rh-cli/                 binary `rh` (clap)
│   └── rh-gui/                 binary `rusty-host` (tray)
│   └── rh-term/                binary `rh-term` (portable terminal)
├── components/                 built-in descriptors (*.toml)
├── templates/                  built-in config fragments
└── tests/                      integration tests
```

**Rule:** `rh-core` has **no** `println!`. All user-facing output lives in the
bindings. This keeps the GUI and CLI honest and makes the library testable.

### 4.2 Workspace layout (portable contract)

```
<root>/
├── rh.toml                 settings
├── components.lock.json    installed versions + digests
├── bin/                    components, one dir per version
│   ├── php/8.4.27/
│   ├── mariadb/11.8.2/
│   ├── apache/2.4.69/
│   ├── phpmyadmin/5.2.3/
│   └── dbx/0.6.62/
├── conf/                   *generated* config
│   ├── apache/
│   ├── php/
│   ├── mariadb/
│   └── ssl/
├── data/                   mutable state (mariadb datadir, logs)
├── tmp/                    scratch (downloads, hosts.tmp)
└── www/                    document root — one dir per project
```

Portability contract: **no absolute paths stored**. Every path is relative to
`<root>` and resolved at runtime. Moving the folder must not break it — this is a
tested invariant (see TDD-13).

### 4.3 Component descriptor (the extensibility core)

Each component is a TOML file in `components/`. **No Rust code is involved in
adding a component.**

```toml
# components/php.toml
name        = "php"
kind        = "runtime"          # runtime | server | database | tool
provides    = ["php"]

[resolve]
# How to discover versions. Respected in order; first match wins.
strategy    = "json-index"
index_url   = "https://downloads.php.net/~windows/releases/releases.json"
# JSON path to {version -> variants} map; variant selector:
version_key = "version"          # field holding the version string
variant     = "ts-vs17-x64"      # preferred build flavour
url_field   = "zip.path"
base_url    = "https://downloads.php.net/~windows/releases/"
sha256_field = "zip.sha256"      # may be absent -> recorded, not enforced

[install]
strip_root  = true               # discard the single top-level dir in the zip
verify      = "sha256-if-present"

[[artifacts]]
executable  = "php.exe"
version_cmd = ["php.exe", "-r", "echo PHP_VERSION;"]
version_regex = '^(\d+\.\d+\.\d+)'

[config]
template    = "php.ini.tpl"
target      = "conf/php/{version}/php.ini"
```

```toml
# components/mariadb.toml
name     = "mariadb"
kind     = "database"

[resolve]
strategy   = "html-scrape"
index_url  = "https://archive.mariadb.org/"
file_regex = 'mariadb-(\d+\.\d+\.\d+)-winx64\.zip'
base_url   = "https://archive.mariadb.org/mariadb-{version}/winx64-packages/"
prefer_lts = true

[[artifacts]]
executable  = "bin/mariadb.exe"
version_cmd = ["bin/mariadb.exe", "--version"]
version_regex = 'Distrib (\d+\.\d+\.\d+)'

[config]
template = "my.ini.tpl"
target   = "conf/mariadb/{version}/my.ini"
```

Supported `resolve.strategy` values:

| Strategy | Use for |
|---|---|
| `json-index` | PHP releases.json, GitHub releases API |
| `html-scrape` | Apache Lounge, nginx.org download page |
| `github-release` | dbx, Mailpit (asset name pattern) |
| `direct` | phpMyAdmin (single fixed URL, no versioning needed) |

### 4.4 Version resolution flow

```
rh install php@8.4
      │
      ▼
  read components/php.toml
      │
      ▼
  fetch index_url (cached 1h in tmp/index-cache/)
      │
      ▼
  filter versions matching "8.4"        ──▶ 8.4.27
      │
      ▼
  build download URL (+ expected sha256 if published)
      │
      ▼
  download → tmp/download/<file>        (progress, resumable)
      │
      ▼
  verify sha256                          ──▶ mismatch = ABORT, delete file
      │
      ▼
  extract → bin/php/8.4.27/              (zip-slip guarded)
      │
      ▼
  run version_cmd, confirm version       ──▶ mismatch = ABORT, rollback
      │
      ▼
  render config from template
      │
      ▼
  update components.lock.json            (atomic temp+rename)
```

### 4.5 The hosts-file strategy (learned from Laragon, improved)

Laragon shells out to a Node.js script to merge and write `tmp/hosts.tmp`, then
`copy /y` over the real hosts file. Good idea (atomic replace, logic as data);
but the Node dependency is unfortunate.

**Rusty Host:** same guarantees, no external runtime.

```
1. Read current hosts file            (if unreadable → permission diagnostic)
2. Strip our managed block            markers: # >>> rusty-host >>> / # <<< rusty-host <<<
3. Append freshly generated block
4. Write to  <root>/tmp/hosts.tmp
5. copy /Y  tmp/hosts.tmp  →  %SystemRoot%\drivers\etc\hosts   (or rename on non-Windows)
6. On ANY failure:
     a. disable auto-hosts in rh.toml  (so we stop retrying)
     b. report exactly what failed + how to fix
     c. NEVER attempt elevation
```

Managed-block markers mean **user entries are never touched** and cleanup is a
pure string operation. This is strictly better than Laragon's approach because
the merge logic lives in Rust (tested) rather than in a shipped JS file.

### 4.6 Process supervision

```
rh start apache
  ├─ probe port 80 (bind test)  ──▶ if taken: report owner via netstat, abort
  ├─ check PID file → is that PID alive and still ours?
  │     ├─ alive → already running, no-op
  │     └─ stale → remove PID file, continue
  ├─ render config from template (always, so edits apply)
  ├─ CreateProcess with CREATE_NEW_PROCESS_GROUP + redirected stdout/stderr
  ├─ write PID + start time + cmdline hash to data/run/<svc>.json
  └─ wait for port to accept a connection (timeout 30s) → report ready or fail
```

`data/run/<svc>.json` stores `{pid, started_at, exe_path, cmdline_sha256}`. On
start, a stale entry whose PID now belongs to a different exe is treated as
stale (prevents killing an unrelated process after PID reuse).

### 4.7 GUI

Red Rust-themed tray application (`eframe`/`egui`, single window + tray icon).

- **Colour system:** Rust brand red `#CE422B` primary, `#2B1B17` panel,
  `#F5F1EE` text, monospace accents.
- Tray menu: Start all / Stop all / Open www / Open phpMyAdmin / Open dbx /
  Preferences / Quit.
- Status dots per service (green running / grey stopped / red failed).
- Deliberately thin: **every action calls `rh-core`, never reimplements it.**

---

## 5. CLI surface (MVP)

```
rh init [--root <dir>]               create workspace
rh install <component>[@<ver>]       resolve latest (or matching) + install
rh uninstall <component>[@<ver>]
rh list                              installed components + versions
rh use <component>@<ver> [--project] pin active / per-project version
rh upgrade [--dry-run] [--yes]       check + install newer versions
rh doctor                            health + diagnostics

rh new <name> [--from <recipe>]      scaffold a project
rh link <dir> [--tld test] [--ssl]   create vhost + hosts entry
rh unlink <dir>
rh sync [--prune] [--dry-run]        reconcile www/ ↔ vhosts

rh start [service...]                start (all if omitted)
rh stop [service...]
rh restart [service...]
rh reload [service...]               graceful config reload
rh status                            table of service state + ports
rh logs <service> [--follow]
rh ports                             who owns which listening port

rh hosts [--show|--edit|--sync]      hosts-file management
rh ssl [--enable <site>|--list]      self-signed certs
rh env [--install|--remove|--show]   user PATH management
rh open [www|phpmyadmin|dbx|site]    launch helper
rh gui                               launch tray app
rh term [--here|--project <x>]       portable terminal, components on PATH
         [--shell <pwsh|cmd|bash>] [--new-window] [--php <ver>]
rh term --list | --kill <id>         list / reap terminal sessions
```

Every command supports `--json` for scripting. Exit codes: `0` ok,
`1` user error, `2` network/verify failure, `3` permission denied,
`4` port conflict, `5` internal.

---

## 6. Non-functional requirements

| ID | Requirement | Target |
|---|---|---|
| NFR-1 | CLI binary size (release, LTO, strip) | **< 4 MB** |
| NFR-2 | CLI cold start | **< 15 ms median** |
| NFR-3 | Peak RSS for typical command | **< 12 MB** |
| NFR-4 | GUI binary size | < 15 MB |
| NFR-5 | GUI idle RSS | < 60 MB |
| NFR-6 | No admin required for install/start/stop/link | verified in tests |
| NFR-7 | Every downloaded artifact SHA-256 verified when published | enforced |
| NFR-8 | Zip-slip / path traversal impossible | unit-tested |
| NFR-9 | Workspace relocatable (copy folder → still works) | integration-tested |
| NFR-10 | No writes outside `<root>` except hosts file | audited + tested |
| NFR-11 | Idempotent operations | every `link`/`install`/`sync` re-runnable |
| NFR-12 | Offline behaviour: clear error, no partial state | tested |
| NFR-13 | No telemetry, no phone-home except version indexes | by design |

---

## 7. Safety & compliance

- **Never** silently elevate privileges. Hosts-file failure → explain + self-disable.
- **Never** extract outside `<root>/bin`. Reject absolute paths, `..`, and
  symlinks escaping the target (zip-slip guard).
- **Never** execute a downloaded binary before checksum verification (when the
  upstream publishes one). When it does not, record the digest in
  `components.lock.json` and print a notice.
- **Never** modify `PATH` without explicit `rh env --install`, and store the
  exact previous value in `rh.toml` for one-command reversal.
- Rusty Host is an independent implementation. It does **not** contain Laragon
  code or assets; the architecture document records observed *behaviour* only.

---

## 8. Milestones

| Milestone | Deliverable | Exit criteria |
|---|---|---|
| **M0** Skeleton | workspace, `rh init`, `rh doctor`, CI | `cargo test` green; binary < 4 MB |
| **M1** Components | registry, fetch, verify, unpack, `install`/`list`/`use` | install real PHP 8.4 + verify digest |
| **M2** Servers | templates, vhosts, hosts file, `link`/`sync`, ports | `http://demo.test` serves PHP |
| **M3** Database | MariaDB + init datadir + phpMyAdmin + dbx | phpMyAdmin reachable at :8080 |
| **M4** Lifecycle | `start/stop/restart/reload/status/logs`, PID safety | no orphan after kill -9 |
| **M5** GUI | Slint tray app, red Rust theme | toggles work from tray |
| **M6** Terminal | `rh-term` — PTY + VT grid + multi-window, env injection | `php -v` works in a fresh terminal with no setup |
| **M7** Polish | `upgrade`, `new`, docs, installer-free zip | fresh-machine smoke test passes |

---

## 9. Success criteria

A user with **only** a Windows machine and internet access can:

1. Download a ~4 MB zip, extract anywhere
2. Run `rh init && rh install php apache mariadb phpmyadmin dbx`
3. Run `rh start`
4. Create `www\demo\index.php`
5. Run `rh link www\demo`
6. Open `http://demo.test` → their PHP runs
7. Open `http://localhost:8080` → phpMyAdmin logs into MariaDB
8. Run `rh gui` → red tray icon, services green
9. Run `rh term` → a terminal opens where `php -v`, `mysql --version` and
   `httpd -v` **all work immediately** with no PATH setup
10. Open a second `rh term --new-window` → independent session, same components
11. **Never** see a UAC prompt (except optionally for hosts editing)
12. Run `rh upgrade` a month later → gets genuinely newer components

---

## 10. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Upstream URL/format changes break resolution | install fails | Per-strategy parser tests with **recorded fixtures**; `rh doctor` reports resolver health |
| Windows hosts editing often blocked (AV/EDR) | core feature degrades | Self-disable + clear remediation (copy Laragon's honesty); wildcard-DNS fallback in P2 |
| MariaDB portable zip needs MSVC runtime | database won't start | `rh doctor` checks VC++ runtime; document download link |
| `dbx` 42 MB portable zip is a Tauri app | needs WebView2 | Check WebView2 presence; fall back to phpMyAdmin |
| Apache Lounge has no version API | brittle scraping | Cache parsed index; allow `rusty-host.components` remote override |
| GUI binary bloat vs NFR-4 | misses target | Slint + `opt-level="z"`; measure in CI |

---

## 11. Decisions (resolved)

All five open questions are now **decided** and binding for implementation.

| # | Question | **Decision** | Rationale |
|---|---|---|---|
| 1 | Default database | **MariaDB** | Portable zip, no installer. MySQL on Windows is installer-first, which breaks the portability promise |
| 2 | Default web server | **Apache** | `.htaccess` compatibility — most PHP apps assume it. Nginx via `rh use server=nginx` |
| 3 | Default TLD | **`.test`** | RFC 6761 reserved, never resolves publicly, no mDNS collision like `.local` |
| 4 | VC++ runtime handling | **Hard-fail with instructions** | Detect, print the exact download link, do not bundle. Keeps the zip small, avoids redistributing MS binaries |
| 5 | GUI framework | **Slint** (v1.18) | Smaller binary than egui, declarative `.slint` markup, one toolkit for tray + terminal |

---

## 12. Portable terminal (`rh-term`)

### 12.1 Why

Laragon bundles **cmder** as its embedded console. cmder is a ConEmu fork: a large
C++ package, and it is **not aware of Laragon's components** — the user still has to
know where PHP lives and activate versions by hand.

Rusty Host ships its own terminal with three properties cmder cannot offer:

1. **Component-aware.** `PATH`, `PHPRC`, `MYSQL_HOME` and friends are injected from
the active component set. Typing `php -v` always runs the selected version — no
manual `PATH` surgery, ever.
2. **Tiny.** Rust + system ConPTY. No bundled emulator baggage.
3. **Multi-window.** Independent windows and tabs, each with its own shell session,
   working directory and environment snapshot.

### 12.2 Feature requirements

| ID | Requirement | Priority |
|---|---|---|
| TERM-1 | Open a real shell (PowerShell / cmd / bash / custom) in a window | **P0** |
| TERM-2 | **Multiple windows** simultaneously, each fully independent | **P0** |
| TERM-3 | **Tabs** within a window | **P1** |
| TERM-4 | Inject active component dirs into `PATH` + set component env vars | **P0** |
| TERM-5 | Respect per-project `.rh.toml` version pins | **P0** |
| TERM-6 | Start in a chosen directory (project, workspace root, or cwd) | **P0** |
| TERM-7 | Correct VT/ANSI rendering, 256-colour and truecolour | **P0** |
| TERM-8 | Resize propagates to the child PTY | **P0** |
| TERM-9 | Copy / paste — Ctrl+Shift+C/V **and** right-click | **P0** |
| TERM-10 | Scrollback buffer ≥ 5 000 lines | **P0** |
| TERM-11 | Search in scrollback | **P1** |
| TERM-12 | Full-screen TUI apps work (vim, htop, composer interactive) | **P0** |
| TERM-13 | Unicode + emoji + wide CJK glyphs correct | **P1** |
| TERM-14 | Per-window env snapshot stable across `rh use` until reopened | **P1** |
| TERM-15 | CLI launch: `rh term [--here] [--project x] [--shell pwsh]` | **P0** |
| TERM-16 | Launch from GUI tray and project context menu | **P1** |
| TERM-17 | Theme follows the Rusty Host red palette | **P2** |
| TERM-18 | Hyperlink detection, Ctrl+click to open | **P2** |
| TERM-19 | Split panes inside a tab | **P2** |

### 12.3 Technical approach

```
rh-term  (binary)
   │
   ├── PTY layer    portable-pty v0.9         ConPTY on Windows, openpty elsewhere
   │      └─ spawns pwsh/cmd/bash with an injected environment
   │
   ├── VT layer     alacritty_terminal v0.26  Term parser + grid + scrollback
   │      └─ ANSI/VT parsing, damage tracking, selection model
   │
   └── UI layer     Slint v1.18               same toolkit as the tray app
          └─ custom `TerminalView`: blits the grid, routes input
```

| Crate | Role | Why |
|---|---|---|
| `portable-pty` | PTY + spawn | 25 M downloads, de-facto Rust PTY abstraction, uses ConPTY (required for TUIs) |
| `alacritty_terminal` | VT parsing + grid | Battle-tested in Alacritty; covers the escape-sequence long tail we must not reinvent |
| `slint` | Rendering + windows | Already chosen for GUI; one toolkit keeps total size down |

**Deliberately not used:** `wezterm` (huge dependency tree), `conpty` crate
(Windows-only, redundant), any WebView/Electron-style embedded terminal.

### 12.4 Environment injection — the differentiator

On spawn the child receives:

```
base env (inherited)
  + PATH        ← active component bin dirs, precedence order:
                    bin/php/<active>/            → php
                    bin/mariadb/<active>/bin/    → mysql, mariadb, mysqldump
                    bin/apache/<active>/bin/     → httpd
                    bin/nginx/<active>/          → nginx
  + PHPRC       ← conf/php/<active>/             so php.ini is found
  + MYSQL_HOME  ← bin/mariadb/<active>/
  + RH_ROOT     ← workspace root
  + RH_PROJECT  ← project dir when launched in project context
  + RH_SHELL    ← "1" — lets scripts detect they run under Rusty Host
```

Version precedence (highest first):

1. Explicit flag — `rh term --php 8.3`
2. Project pin — `.rh.toml` in cwd or an ancestor under `www/`
3. Workspace default — `rh.toml [defaults]`
4. Newest installed

Because `php` resolves through an injected `PATH`, **every component Laragon ships
is immediately usable** — `php`, `mysql`, `mariadb`, `mysqldump`, `httpd`, `nginx` —
with zero manual configuration. That is the entire point of the feature.

### 12.5 Multi-window model

- A **window** is a separate OS window owning its own Slint state.
- A **tab** owns one PTY session and one environment snapshot (TERM-14).
- `rh term --new-window` spawns a **separate process**, not a child window, so a
  crash in one terminal cannot take down the others.
- Sessions are recorded in `data/run/term/*.json`, so `rh term --list` shows them and
  `rh term --kill <id>` reaps orphans — the same discipline as service PIDs.

### 12.6 Terminal non-functional targets

| ID | Requirement | Target |
|---|---|---|
| TERM-NFR-1 | `rh-term` binary size | **< 12 MB** |
| TERM-NFR-2 | Cold start to first prompt | **< 400 ms** |
| TERM-NFR-3 | Idle RSS per window (80×24, 5 000 scrollback) | **< 45 MB** |
| TERM-NFR-4 | Input-to-echo latency | **< 16 ms** |
| TERM-NFR-5 | Scroll throughput | 60 fps on a full 5 000-line buffer |
| TERM-NFR-6 | No orphans after window close or hard kill | verified by test |

---

*Companion document: `TDD.md` — test-driven implementation plan.*
