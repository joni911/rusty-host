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

**Design phase.** The documents above are complete and reviewed; implementation
has not started.

Implementation proceeds through 16 red-green cycles (see `docs/TDD.md` §15),
milestones M0–M6 (see `docs/PRD.md` §8).

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

## Licence

MIT.

Rusty Host is an independent implementation. It contains no Laragon code or
assets. The architecture notes in this repository record observed *behaviour* of
a locally installed copy, for interoperability and learning.
