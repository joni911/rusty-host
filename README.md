# Rusty Host

**A portable, always-current local web development environment in Rust.**

One folder. Double-click. Full PHP stack. No installer, no admin, no stale components.

<p align="center">
  <img alt="status" src="https://img.shields.io/badge/status-design%20phase-orange">
  <img alt="language" src="https://img.shields.io/badge/rust-2021-CE422B">
  <img alt="licence" src="https://img.shields.io/badge/licence-MIT-blue">
</p>

---

## Why

Laragon proved that a portable local dev environment is the right shape for Windows
web development. This project keeps what worked and fixes what did not:

| Laragon | Rusty Host |
|---|---|
| PHP 8.4.12, MySQL 8.0.30, Apache 2.4.54 — frozen at release | **Resolved from upstream at install time.** Never stale |
| Adding a tool meant Delphi code changes | **Declarative `component.toml`** — no code, no rebuild |
| GUI only, not scriptable | **CLI first** (`rh`), GUI is a thin shell |
| Windows only | Windows now, portable core |
| HeidiSQL (large Delphi app) | **`dbx` portable** (42 MB, Rust, 100+ databases) |
| Last free version is final | MIT, forever |

## What you get

- **PHP** — multiple versions, resolved live from `php.net`
- **Apache** / **Nginx** — whichever you prefer
- **MariaDB** — portable zip, no installer
- **phpMyAdmin** at `http://localhost:8080`
- **dbx** — desktop database client, portable
- **Mailpit** — local mail capture with a modern UI
- **Auto virtual hosts** — folder in `www/` becomes `name.test`
- **Red Rust-themed tray app**

## Documents

| File | Contents |
|---|---|
| [`docs/PRD.md`](docs/PRD.md) | Product requirements, full Laragon feature parity matrix (70 features), architecture, CLI surface, NFRs, risks |
| [`docs/TDD.md`](docs/TDD.md) | Test-driven plan: ~124 tests before the GUI, red-green cycle order, safety properties, traceability |
| [`docs/COMPONENTS.md`](docs/COMPONENTS.md) | Component registry with verified upstream URLs, and how to add your own in 10 lines of TOML |

## Status

**Implementation in progress.** Cycles 1–9 of 17 are complete (144 tests green).

| Cycle | Module | Tests | State |
|---|---|---|---|
| TDD-01 | `workspace` — layout, discovery, lock file | 17 | done |
| TDD-02 | `component` — declarative descriptors | 11 | done |
| TDD-03 | `resolve` — version resolution (4 strategies) | 18 | done |
| TDD-04 | `fetch` + `unpack` — verify, safe extraction | 17 | done |
| TDD-05 | `template` — `<<TOKEN>>` rendering | 19 | done |
| TDD-06 | `project` — scan, slug, document root | 18 | done |
| TDD-07 | `hosts` — atomic managed-block editing | 20 | done |
| TDD-08 | `vhost` — render and reconcile | 14 | done |
| TDD-09 | `ports` — pre-flight conflict detection | 10 | done |
| TDD-10 | `proc` — process supervision | — | next |
| TDD-11 | `recipe` — project scaffolding | — | planned |
| TDD-12 | CLI wiring | — | planned |
| TDD-13 | Portability guarantee | — | planned |
| TDD-15 | `rh-term` portable terminal | — | planned |
| — | GUI (Slint, red theme) | — | planned |
| — | NFR gates | — | planned |

Currently usable from the CLI: `rh init`, `rh list`, `rh doctor`.

The library underneath is complete for install-through-vhost; wiring the
remaining subcommands is TDD-12.

## Design in one picture

```
rusty-host/
├── crates/
│   ├── rh-core/     library — all logic, no I/O policy, fully tested
│   ├── rh-cli/      binary `rh`
│   └── rh-gui/      binary `rusty-host` (tray, red theme)
├── components/      declarative *.toml descriptors — add tools here
├── templates/       built-in config fragments
└── tests/           integration + fixtures
```

Every workspace is relocatable: copy the folder anywhere and it still works.
No absolute paths are ever stored. This is enforced by a test (`TDD-13`).

## Safety commitments

- **Never** silently elevate privileges. If the hosts file cannot be written we
  say so, disable the feature, and clean up — exactly like Laragon does, because
  it is the right behaviour.
- **Never** extract outside `bin/`. Zip-slip is rejected by a property test.
- **Never** run a downloaded binary before checksum verification, when upstream
  publishes one.
- **Never** modify `PATH` without an explicit command, and make it reversible.
- **No telemetry.** Version indexes are the only network calls.

## Building

Windows needs a complete mingw-w64 for linking (Rust's bundled `rust-mingw` is
missing `crtbegin.o`). `scripts/build.sh` finds one automatically:

```bash
scripts/build.sh test          # 144 tests
scripts/build.sh build --release
scripts/build.sh clippy --all-targets
```

If no toolchain is detected, install one (`scoop install mingw-winlibs`) or set
`MINGW_ROOT`.

## Licence

MIT.

Rusty Host is an independent implementation. It contains no Laragon code or
assets. The architecture notes in this repository record observed *behaviour* of
a locally installed copy, for interoperability and learning.
