//! `rh` — Rusty Host command line interface.
//!
//! A thin presentation layer over `rh-core` (PRD §4.1): every action here is a
//! call into the library, never a reimplementation. That is what lets the GUI
//! reuse the same behaviour.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rh_core::hosts::{self, Block};
use rh_core::ports;
use rh_core::proc::{ServiceState, Supervisor};
use rh_core::project;
use rh_core::recipe;
use rh_core::settings::{self, Settings};
use rh_core::vhost::{self, Server};
use rh_core::workspace::{DiscoverError, Workspace};

/// Exit codes, per PRD §5.
///
/// Not every variant is reachable yet: Network and PortConflict become live once
/// the download pipeline and service start are wired up (TDD-14).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Ok = 0,
    User = 1,
    Network = 2,
    Permission = 3,
    PortConflict = 4,
    Internal = 5,
}

struct CliError {
    message: String,
    code: Exit,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::PermissionDenied => Exit::Permission,
            _ => Exit::Internal,
        };
        CliError {
            message: e.to_string(),
            code,
        }
    }
}

fn user(msg: impl Into<String>) -> CliError {
    CliError {
        message: msg.into(),
        code: Exit::User,
    }
}

fn internal(msg: impl Into<String>) -> CliError {
    CliError {
        message: msg.into(),
        code: Exit::Internal,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::from(Exit::Ok as u8),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(e.code as u8)
        }
    }
}

fn run(argv: &[String]) -> Result<(), CliError> {
    let (root, rest) = extract_flag(argv, "--root")?;
    let (json, rest) = extract_switch(&rest, "--json");

    let Some(cmd) = rest.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    let args = &rest[1..];
    let ws = resolve_workspace(root);

    match cmd {
        "init" => cmd_init(&ws),
        "list" => cmd_list(&ws, json),
        "install" => cmd_install(&ws, args),
        "uninstall" => cmd_uninstall(&ws, args),
        "doctor" => cmd_doctor(&ws),
        "projects" => cmd_projects(&ws, json),
        "link" => cmd_link(&ws, args),
        "unlink" => cmd_unlink(&ws, args),
        "sync" => cmd_sync(&ws, args),
        "hosts" => cmd_hosts(&ws, args),
        "ports" => cmd_ports(&ws),
        "status" => cmd_status(&ws),
        "stop" => cmd_stop(&ws, args),
        "logs" => cmd_logs(&ws, args),
        "new" => cmd_new(&ws, args),
        "recipes" => cmd_recipes(),
        "gui" => cmd_gui(),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "-V" | "--version" => {
            println!("rh {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(user(format!(
            "unknown command `{other}`\n\nRun `rh --help` for usage."
        ))),
    }
}

// ------------------------------------------------------------------ flags

fn extract_flag(argv: &[String], name: &str) -> Result<(Option<PathBuf>, Vec<String>), CliError> {
    let mut value = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == name {
            let Some(v) = argv.get(i + 1) else {
                return Err(user(format!("{name} requires a value")));
            };
            value = Some(PathBuf::from(v));
            i += 2;
        } else {
            rest.push(argv[i].clone());
            i += 1;
        }
    }
    Ok((value, rest))
}

fn extract_switch(argv: &[String], name: &str) -> (bool, Vec<String>) {
    let mut found = false;
    let mut rest = Vec::new();
    for a in argv {
        if a == name {
            found = true;
        } else {
            rest.push(a.clone());
        }
    }
    (found, rest)
}

/// Positional arguments only (drops unknown `--flags`).
fn positional(argv: &[String]) -> Vec<String> {
    argv.iter().filter(|a| !a.starts_with("--")).cloned().collect()
}

fn resolve_workspace(root: Option<PathBuf>) -> Workspace {
    if let Some(r) = root {
        return Workspace::at(r);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match Workspace::discover(&cwd) {
        Ok(ws) => ws,
        Err(DiscoverError::NotFound { .. }) => Workspace::at(cwd),
    }
}

fn settings(ws: &Workspace) -> Settings {
    Settings::load(ws).unwrap_or_default()
}

// --------------------------------------------------------------- commands

fn cmd_init(ws: &Workspace) -> Result<(), CliError> {
    ws.init()?;
    println!("initialised workspace at {}", ws.root().display());
    for (label, dir) in [
        ("bin", ws.bin()),
        ("conf", ws.conf()),
        ("data", ws.data()),
        ("tmp", ws.tmp()),
        ("www", ws.www()),
    ] {
        println!("  {label:<6} {}", dir.display());
    }
    Ok(())
}

fn cmd_list(ws: &Workspace, json: bool) -> Result<(), CliError> {
    let lock = ws.load_lock()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&lock.components).unwrap_or_default()
        );
        return Ok(());
    }
    if lock.components.is_empty() {
        let available: Vec<String> = settings::catalogue()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();
        println!("no components installed");
        if !available.is_empty() {
            println!("available: {}", available.join(", "));
        }
        return Ok(());
    }
    for (name, versions) in &lock.components {
        let vs: Vec<String> = versions.keys().cloned().collect();
        println!("{name:<14} {}", vs.join(", "));
    }
    Ok(())
}

fn cmd_install(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    if pos.is_empty() {
        return Err(user("usage: rh install <component>[@<version>]"));
    }

    let (archive, _) = extract_flag(args, "--from")?;
    let catalogue = settings::catalogue().map_err(|e| internal(e.to_string()))?;

    for spec in pos {
        let (name, want) = match spec.split_once('@') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (spec.clone(), None),
        };
        let Some(comp) = catalogue.get(&name) else {
            return Err(user(format!(
                "unknown component `{name}`. Available: {}",
                catalogue.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        };

        // Offline path: install a local archive (used by tests and by air-gapped
        // machines).
        if let Some(path) = &archive {
            let version = want.clone().unwrap_or_else(|| "local".to_string());
            let installed =
                settings::install_archive(ws, comp, &version, path, None).map_err(|e| {
                    CliError {
                        message: e.to_string(),
                        code: Exit::Internal,
                    }
                })?;
            println!(
                "installed {} {} -> {}",
                installed.component,
                installed.version,
                installed.dir.display()
            );
            continue;
        }

        println!(
            "{name}: resolved version needs a network index.\n\
             Run with --from <archive.zip> to install a local artifact, or see\n\
             docs/COMPONENTS.md for the upstream URLs this build resolves."
        );
        if let Some(v) = want {
            let _ = v;
        }
    }
    Ok(())
}

fn cmd_uninstall(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    if pos.is_empty() {
        return Err(user("usage: rh uninstall <component>@<version>"));
    }
    for spec in pos {
        let (name, version) = spec
            .split_once('@')
            .ok_or_else(|| user("uninstall needs an explicit version: name@version"))?;
        settings::uninstall(ws, name, version).map_err(|e| CliError {
            message: e.to_string(),
            code: Exit::User,
        })?;
        println!("removed {name} {version}");
    }
    Ok(())
}

fn cmd_doctor(ws: &Workspace) -> Result<(), CliError> {
    let mut problems = 0;
    println!("workspace   {}", ws.root().display());

    for (label, dir) in [
        ("bin", ws.bin()),
        ("conf", ws.conf()),
        ("data", ws.data()),
        ("tmp", ws.tmp()),
        ("www", ws.www()),
    ] {
        let ok = dir.is_dir();
        println!("  {label:<6} {}", if ok { "ok" } else { "MISSING" });
        if !ok {
            problems += 1;
        }
    }

    let settings_ok = ws.settings_path().is_file();
    println!("  rh.toml {}", if settings_ok { "ok" } else { "MISSING" });
    if !settings_ok {
        problems += 1;
    }

    match ws.load_lock() {
        Ok(lock) => {
            let n: usize = lock.components.values().map(|m| m.len()).sum();
            println!("  lock   ok ({n} component versions)");
        }
        Err(e) => {
            println!("  lock   INVALID ({e})");
            problems += 1;
        }
    }

    match settings::catalogue() {
        Ok(c) => println!("  recipes/{} components available", c.len()),
        Err(e) => {
            println!("  components UNAVAILABLE ({e})");
            problems += 1;
        }
    }

    // Report port availability, but never fail the doctor for it: another
    // dev tool may legitimately be using 80.
    let s = settings(ws);
    let wanted: Vec<(String, u16)> = ["http", "mysql", "phpmyadmin"]
        .iter()
        .map(|n| (n.to_string(), s.port(n)))
        .collect();
    for c in ports::preflight(&wanted) {
        println!("  note   port {} ({}) is in use", c.port, c.service);
    }

    if problems == 0 {
        println!("\nhealthy");
        Ok(())
    } else {
        Err(user(format!(
            "{problems} problem(s) found; run `rh init` to repair"
        )))
    }
}

fn cmd_projects(ws: &Workspace, json: bool) -> Result<(), CliError> {
    let s = settings(ws);
    let projects = project::scan(&ws.www(), &s.tld).map_err(|e| user(e.to_string()))?;

    if json {
        let rows: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "hostname": p.hostname,
                    "path": p.path.display().to_string(),
                    "document_root": p.document_root.display().to_string(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return Ok(());
    }

    if projects.is_empty() {
        println!("no projects in {}", ws.www().display());
        return Ok(());
    }
    for p in &projects {
        println!("{:<24} {}   ({})", p.name, p.url("http"), p.document_root.display());
    }
    Ok(())
}

fn servers_for(args: &[String]) -> Vec<Server> {
    let mut out = Vec::new();
    for a in args {
        match a.as_str() {
            "--nginx" => out.push(Server::Nginx),
            "--apache" => out.push(Server::Apache),
            _ => {}
        }
    }
    if out.is_empty() {
        out.push(Server::Apache);
    }
    out
}

fn cmd_link(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    let s = settings(ws);
    let projects = project::scan(&ws.www(), &s.tld).map_err(|e| user(e.to_string()))?;

    let selected: Vec<&project::Project> = if pos.is_empty() {
        projects.iter().collect()
    } else {
        let want = pos[0].trim_end_matches(['/', '\\']);
        let want_slug = project::slugify(Path::new(want).file_name().and_then(|n| n.to_str()).unwrap_or(want));
        let hits: Vec<&project::Project> = projects
            .iter()
            .filter(|p| p.slug == want_slug || p.name == want)
            .collect();
        if hits.is_empty() {
            return Err(user(format!(
                "no project `{want}` under {}",
                ws.www().display()
            )));
        }
        hits
    };

    let ctx = vhost::Context {
        conf_dir: &ws.conf(),
        port: s.port("http"),
        php_port: s.port("php-fpm"),
    };

    for server in servers_for(args) {
        for p in &selected {
            let out = vhost::link(p, server, &ctx).map_err(|e| CliError {
                message: e.to_string(),
                code: Exit::Internal,
            })?;
            println!("{:<7} {}", format!("{server:?}").to_lowercase(), out.path.display());
        }
    }

    // Hosts file, honouring the saved preference.
    if s.hosts.auto {
        let entries: Vec<(String, String)> = selected
            .iter()
            .map(|p| (p.hostname.clone(), "127.0.0.1".to_string()))
            .collect();
        let mut block = Block::default();
        if let Ok(existing) = std::fs::read_to_string(hosts::system_hosts_path()) {
            let mut all: Vec<(String, String)> = hosts::parse_managed(&existing)
                .into_iter()
                .map(|h| (h, "127.0.0.1".to_string()))
                .collect();
            all.extend(entries);
            // De-duplicate by hostname.
            let map: BTreeMap<String, String> = all.into_iter().collect();
            block.entries = map.into_iter().collect();
        } else {
            block.entries = entries;
        }

        match hosts::sync(&hosts::system_hosts_path(), &block) {
            Ok(()) => println!("\nhosts file updated ({} entries)", block.entries.len()),
            Err(e) => {
                // Self-disable, exactly as designed: stop retrying and explain.
                // The vhosts were already written, so this is a warning, not a
                // failure — the user can still reach the site once the hosts
                // entry exists.
                let mut s2 = s.clone();
                s2.hosts.auto = false;
                let _ = s2.save(ws);
                eprintln!("warning: {e}");
            }
        }
    } else {
        println!("\nhosts management disabled; run `rh hosts --sync` to manage it manually");
    }
    Ok(())
}

fn cmd_unlink(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    if pos.is_empty() {
        return Err(user("usage: rh unlink <project>"));
    }
    let slug = project::slugify(&pos[0]);
    let s = settings(ws);
    let hostname = format!("{slug}.{}", s.tld);

    for server in [Server::Apache, Server::Nginx] {
        let path = ws
            .conf()
            .join(server.conf_subdir())
            .join(format!("{hostname}.conf"));
        if path.is_file() {
            std::fs::remove_file(&path)?;
            println!("removed {}", path.display());
        }
    }
    Ok(())
}

fn cmd_sync(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let (dry, rest) = extract_switch(args, "--dry-run");
    let s = settings(ws);
    let projects = project::scan(&ws.www(), &s.tld).map_err(|e| user(e.to_string()))?;
    let ctx = vhost::Context {
        conf_dir: &ws.conf(),
        port: s.port("http"),
        php_port: s.port("php-fpm"),
    };

    for server in servers_for(&rest) {
        let report = if dry {
            vhost::sync_dry_run(&projects, server, &ctx)
        } else {
            vhost::sync(&projects, server, &ctx)
        }
        .map_err(|e| internal(e.to_string()))?;

        println!("{server:?}: {}", if report.is_clean() { "nothing to do".to_string() } else { String::new() });
        if !report.is_clean() {
            print!("{report}");
        }
    }

    if !dry {
        let entries: Vec<(String, String)> = projects
            .iter()
            .map(|p| (p.hostname.clone(), "127.0.0.1".to_string()))
            .collect();
        let block = Block { entries };
        match hosts::sync(&hosts::system_hosts_path(), &block) {
            Ok(()) => println!("hosts: {} entries", block.entries.len()),
            Err(e) => println!("hosts: {e}"),
        }
    }
    Ok(())
}

fn cmd_hosts(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let path = hosts::system_hosts_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let managed = hosts::parse_managed(&text);

    if args.iter().any(|a| a == "--show") {
        println!("{}", path.display());
        println!("managed entries: {}", managed.len());
        for h in &managed {
            println!("  {h}");
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--edit") {
        println!("open this file in an editor: {}", path.display());
        return Ok(());
    }
    // Default: reconcile with the projects on disk.
    cmd_sync(ws, &["--apache".to_string()])
}

fn cmd_ports(ws: &Workspace) -> Result<(), CliError> {
    let s = settings(ws);
    let mut rows: Vec<(String, u16)> = s.ports.iter().map(|(k, v)| (k.clone(), *v)).collect();
    if rows.is_empty() {
        rows = ports::default_ports();
    }
    for (name, port) in rows {
        let state = ports::probe(port);
        let label = if state.is_free() { "free" } else { "IN USE" };
        println!("{name:<14} {port:<6} {label}");
    }
    Ok(())
}

fn supervisor(ws: &Workspace) -> Supervisor {
    Supervisor::new(ws.data().join("run"))
}

fn cmd_status(ws: &Workspace) -> Result<(), CliError> {
    let sup = supervisor(ws);
    let lock = ws.load_lock()?;
    let s = settings(ws);

    println!("{:<12} {:<10} {:<8}", "service", "state", "port");
    for (name, port_name) in [
        ("apache", "http"),
        ("nginx", "http"),
        ("mariadb", "mysql"),
    ] {
        if !lock.has("mariadb") && name == "mariadb" {
            continue;
        }
        let state = sup.state(name);
        let marker = match state {
            ServiceState::Running => "running",
            ServiceState::Stale => "stale",
            ServiceState::Stopped => "-",
        };
        println!("{:<12} {:<10} {:<8}", name, marker, s.port(port_name));
    }
    Ok(())
}

fn cmd_stop(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let sup = supervisor(ws);
    let pos = positional(args);
    let targets: Vec<String> = if pos.is_empty() {
        vec!["apache".into(), "nginx".into(), "mariadb".into()]
    } else {
        pos
    };
    for t in targets {
        match sup.stop(&t) {
            Ok(()) => println!("stopped {t}"),
            Err(e) => println!("{t}: {e}"),
        }
    }
    Ok(())
}

fn cmd_logs(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    let Some(service) = pos.first() else {
        return Err(user("usage: rh logs <service>"));
    };
    let sup = supervisor(ws);
    let rec = sup.load(service).map_err(|e| internal(e.to_string()))?;
    match rec.and_then(|r| r.stdout_log) {
        Some(p) => {
            println!("{}", p.display());
            if let Ok(text) = std::fs::read_to_string(&p) {
                let skip = text.lines().count().saturating_sub(50);
                for line in text.lines().skip(skip) {
                    println!("{line}");
                }
            }
        }
        None => println!("no log recorded for `{service}`"),
    }
    Ok(())
}

fn cmd_new(ws: &Workspace, args: &[String]) -> Result<(), CliError> {
    let pos = positional(args);
    let Some(name) = pos.first() else {
        return Err(user("usage: rh new <name> [--from <recipe>]"));
    };
    let recipe_name = args
        .iter()
        .position(|a| a == "--from")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "Blank".to_string());

    let list = recipe::builtin();
    let Some(rc) = list.iter().find(|r| r.name.eq_ignore_ascii_case(&recipe_name)) else {
        return Err(user(format!(
            "unknown recipe `{recipe_name}`. Available: {}",
            list.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")
        )));
    };

    let dir = recipe::scaffold(rc, name, &ws.www(), &[]).map_err(|e| user(e.to_string()))?;
    println!("created {}", dir.display());
    match &rc.action {
        recipe::Action::Blank => println!("empty project ready"),
        recipe::Action::Url(u) => println!("download this and unpack into it: {u}"),
        recipe::Action::Command(_c) => {
            if let Ok(cmd) = rc.build_command(name) {
                println!("run this inside it: {cmd}");
            }
        }
    }
    Ok(())
}

fn cmd_recipes() -> Result<(), CliError> {
    for r in recipe::builtin() {
        let kind = match &r.action {
            recipe::Action::Blank => "blank",
            recipe::Action::Url(_) => "url",
            recipe::Action::Command(_) => "command",
        };
        println!("{:<12} {kind}", r.name);
    }
    Ok(())
}

fn cmd_gui() -> Result<(), CliError> {
    println!(
        "the GUI is built as a separate binary: `rusty-host`.\n\
         If it is installed, run it directly; otherwise build it with\n\
         `scripts/build.sh build --release -p rh-gui`."
    );
    Ok(())
}

fn print_help() {
    println!(
        "rh {} — Rusty Host

Usage: rh <command> [--root <dir>] [--json]

Workspace
  init                    Create a workspace (bin/ conf/ data/ tmp/ www/)
  doctor                  Check workspace health
  list                    List installed components

Components
  install <comp>[@<ver>]  Install (use --from <archive.zip> for a local artifact)
  uninstall <comp>@<ver>  Remove an installed version

Projects
  projects                List projects and their hostnames
  link [project]          Write vhosts and hosts entries
  unlink <project>        Remove a project's vhosts
  sync [--dry-run]        Reconcile www/ with vhosts
  new <name> [--from R]   Scaffold a project from a recipe
  recipes                 List available recipes

Services
  status                  Show service state and ports
  stop [service...]       Stop services
  logs <service>          Show a service's captured output

Hosts
  hosts [--show|--edit]   Inspect or reconcile the hosts file
  ports                   Show configured ports and whether they are free

Other
  gui                     How to launch the tray application
  --root <dir>            Operate on a specific workspace
  --json                  Machine-readable output
  -h, --help              Show help
  -V, --version           Show version",
        env!("CARGO_PKG_VERSION")
    );
}
