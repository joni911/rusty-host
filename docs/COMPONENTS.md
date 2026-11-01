# Component registry — verified upstream sources

All URLs below were checked live at time of writing. Each component is a
declarative descriptor; **adding a component requires no Rust code** (PRD §4.3).

| Component | Strategy | Upstream index | Latest seen | Artifact |
|---|---|---|---|---|
| **php** | `json-index` | `downloads.php.net/~windows/releases/releases.json` | 8.5.12 / 8.4.27 / 8.3.36 | `php-{ver}-Win32-vs17-x64.zip` |
| **apache** | `html-scrape` | `apachelounge.com/download/` | 2.4.69-261002 | `httpd-2.4.69-261002-Win64-VS18.zip` |
| **nginx** | `html-scrape` | `nginx.org/en/download.html` | 1.31.8 | `nginx-1.31.8.zip` |
| **mariadb** | `html-scrape` | `archive.mariadb.org/` | 11.8 (LTS) | `mariadb-{ver}-winx64.zip` |
| **phpmyadmin** | `direct` | `phpmyadmin.net/home_page/version.json` | 5.2.3 | `phpMyAdmin-5.2.3-all-languages.zip` |
| **dbx** | `github-release` | `api.github.com/repos/t8y2/dbx/releases/latest` | v0.6.62 | `DBX_0.6.62_x64-portable.zip` (42.5 MB) |
| **mailpit** | `github-release` | `api.github.com/repos/axllent/mailpit/releases/latest` | v1.33.0 | `mailpit-windows-amd64.zip` |

## Notes per component

### php — JSON index is ideal
`releases.json` is a real API: `{ "<major>": { "version": "...", "<variant>": { "zip": { "path": "..." } } } }`.
Variant selection matters — we want `ts-vs17-x64` (thread-safe, VS17 x64) for
Apache `mod_php`, or `nts-vs17-x64` for FastCGI. **This is a descriptor field,
not code.**

⚠️ `sha256` is **not** published in `releases.json`. Policy: record the digest in
`components.lock.json` on first install, warn once, never claim verification we
did not perform (PRD §7).

### apache — HTML scrape, needs care
Lounge publishes no API. The listing page contains:
```
href="/download/VS18/binaries/httpd-2.4.69-261002-Win64-VS18.zip"
```
Regex must anchor on `Win64` (exclude `win32`), prefer newest `VS*`.
Response requires a browser `User-Agent` — the descriptor must set one.
Cache parsed results for 1 h to be a polite client.

### nginx — simple scrape
`nginx.org/en/download.html` lists `nginx-1.31.8.zip` plainly. Windows builds are
in that same zip. Mainline vs stable: descriptor uses `prefer_stable = false`
(mainline) with an option to switch.

### mariadb — portable zip is the point
MariaDB publishes `winx64-packages/mariadb-{ver}-winx64.zip` — **no installer
needed**, which is exactly the portability requirement. Prefer LTS (11.8) over
newer non-LTS (13.x) for stability. MySQL on Windows is installer-first, so
MariaDB is the correct default for a portable product.

⚠️ Requires MSVC runtime (VC++ redistributable). `rh doctor` must check and
report with a download link rather than fail cryptically.

### phpmyadmin — fixed, unversioned-in-practice
Ships as `-all-languages.zip`, extracted into `bin/phpmyadmin/<ver>/`, served by
the selected web server as an alias at `http://localhost:8080`.
Needs a generated `config.inc.php` with `$cfg['blowfish_secret']` (32 random
bytes) and the MariaDB socket/port. **Generate, never ship** a secret.

### dbx — replaces HeidiSQL (per requirement)
`t8y2/dbx` is a 25 MB Rust/Tauri database client supporting 100+ engines,
28k stars. The **portable** asset is `DBX_{ver}_x64-portable.zip` (42.5 MB);
there is also a `.sig` for signature verification and `.sha256` on some assets.

Critical asset-selection test: must pick `-portable.zip` and **must not** pick
`-setup.exe`, `-offline-setup.exe`, `.deb`, `.rpm`, `.AppImage`, or `.dmg`.

⚠️ Tauri ⇒ needs WebView2 (present on Win10 21H2+ / Win11). `rh doctor` checks;
fallback message points to phpMyAdmin, which needs nothing.

### mailpit — replaces MailCatcher
Single Rust binary, `mailpit-windows-amd64.zip`, modern web UI, SMTP on :1025,
UI on :8025. Strictly better than the old `mailcatcher` gem.

## Adding your own component

Drop a TOML file in `components/` and run `rh list --available`:

```toml
name     = "redis"
kind     = "tool"

[resolve]
strategy    = "github-release"
repo        = "redis-windows/redis-windows"
asset_regex = 'Redis-[\d.]+-Windows-x64-[\d.]+\.zip'

[[artifacts]]
executable    = "redis-server.exe"
version_cmd   = ["redis-server.exe", "--version"]
version_regex = 'v=(\d+\.\d+\.\d+)'
```

No rebuild, no code change. That is the whole difference from Laragon, where
each tool needed `Mnu*`, `CheckBox*` and `Edit*Port` wiring compiled into the
binary (see `laragon-architecture.md` §6).
