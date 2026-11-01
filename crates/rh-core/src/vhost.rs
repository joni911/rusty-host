//! Vhost rendering and reconciliation (PRD §3.2).
//!
//! `link` writes one server config per project. `sync` reconciles the whole
//! `www/` directory against what is on disk.
//!
//! The rule that keeps `sync` safe: a generated file carries a `Managed-by`
//! marker, and **only marked files may ever be pruned**. A hand-written config a
//! user dropped into `sites-enabled/` is theirs, and deleting it would be a
//! destructive surprise.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::project::Project;
use crate::template::render;

/// Marker that identifies a file as ours.
pub const MANAGED_MARKER: &str = "Managed-by: rusty-host";

/// Which web server a config is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    Apache,
    Nginx,
}

impl Server {
    /// Directory (relative to the workspace `conf/`) this server's vhosts live in.
    pub fn conf_subdir(self) -> &'static str {
        match self {
            Server::Apache => "apache/sites-enabled",
            Server::Nginx => "nginx/sites-enabled",
        }
    }

    /// Extension of generated files.
    pub fn conf_ext(self) -> &'static str {
        "conf"
    }

    fn template(self, ssl: bool) -> &'static str {
        match (self, ssl) {
            (Server::Apache, false) => include_str!("../../../templates/vhost.apache.tpl"),
            (Server::Apache, true) => include_str!("../../../templates/vhost.apache.ssl.tpl"),
            (Server::Nginx, false) => include_str!("../../../templates/vhost.nginx.tpl"),
            (Server::Nginx, true) => include_str!("../../../templates/vhost.nginx.ssl.tpl"),
        }
    }
}

/// Where and how to write vhosts.
pub struct Context<'a> {
    /// Workspace `conf/` directory.
    pub conf_dir: &'a Path,
    /// HTTP port the server listens on.
    pub port: u16,
    /// PHP FastCGI port (Nginx).
    pub php_port: u16,
}

impl Context<'_> {
    fn sites_dir(&self, server: Server) -> PathBuf {
        self.conf_dir.join(server.conf_subdir())
    }
}

/// A rendered config ready to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Full path of the config file.
    pub path: PathBuf,
    /// Hostname it serves.
    pub hostname: String,
    /// File contents.
    pub body: String,
}

/// One change `sync` made or would make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAction {
    /// Path affected.
    pub path: PathBuf,
    /// Hostname affected.
    pub hostname: String,
    /// New contents (empty when pruning).
    pub body: String,
}

impl SyncAction {
    /// Human-readable one-liner.
    pub fn display(&self) -> String {
        let verb = if self.body.is_empty() {
            "prune"
        } else {
            "write"
        };
        format!("{verb} {} ({})", self.hostname, self.path.display())
    }
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Configs created or updated.
    pub written: Vec<SyncAction>,
    /// Orphaned managed configs removed.
    pub pruned: Vec<SyncAction>,
}

impl SyncReport {
    /// Whether nothing needed doing.
    pub fn is_clean(&self) -> bool {
        self.written.is_empty() && self.pruned.is_empty()
    }
}

impl fmt::Display for SyncReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_clean() {
            return write!(f, "nothing to do");
        }
        for a in &self.written {
            writeln!(f, "  {}", a.display())?;
        }
        for a in &self.pruned {
            writeln!(f, "  {}", a.display())?;
        }
        Ok(())
    }
}

/// Why a vhost operation failed.
#[derive(Debug)]
pub enum VhostError {
    /// Writing a config failed.
    Io { path: String, detail: String },
    /// A template could not be rendered.
    Render { detail: String },
}

impl fmt::Display for VhostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VhostError::Io { path, detail } => write!(f, "cannot write {path}: {detail}"),
            VhostError::Render { detail } => write!(f, "cannot render vhost: {detail}"),
        }
    }
}

impl std::error::Error for VhostError {}

/// Render a project's config for `server` without touching disk.
pub fn render_project(
    project: &Project,
    server: Server,
    ctx: &Context<'_>,
    ssl: Option<(&Path, &Path)>,
) -> Result<Rendered, VhostError> {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    vars.insert("HOSTNAME".into(), project.hostname.clone());
    vars.insert("PORT".into(), ctx.port.to_string());
    vars.insert("PROJECT_DIR".into(), project.document_root.display().to_string());
    vars.insert("PHP_PORT".into(), ctx.php_port.to_string());

    if let Some((cert, key)) = ssl {
        vars.insert("SSL_CERT".into(), cert.display().to_string());
        vars.insert("SSL_KEY".into(), key.display().to_string());
    }

    let body = render(server.template(ssl.is_some()), &vars).map_err(|e| VhostError::Render {
        detail: e.to_string(),
    })?;

    Ok(Rendered {
        path: ctx
            .sites_dir(server)
            .join(format!("{}.{}", project.hostname, server.conf_ext())),
        hostname: project.hostname.clone(),
        body,
    })
}

/// Write a project's config.
pub fn link(
    project: &Project,
    server: Server,
    ctx: &Context<'_>,
) -> Result<Rendered, VhostError> {
    let rendered = render_project(project, server, ctx, None)?;
    write_rendered(&rendered)?;
    Ok(rendered)
}

/// Write a project's config with TLS directives.
pub fn link_ssl(
    project: &Project,
    server: Server,
    ctx: &Context<'_>,
    cert: &Path,
    key: &Path,
) -> Result<Rendered, VhostError> {
    let rendered = render_project(project, server, ctx, Some((cert, key)))?;
    write_rendered(&rendered)?;
    Ok(rendered)
}

fn write_rendered(rendered: &Rendered) -> Result<(), VhostError> {
    if let Some(dir) = rendered.path.parent() {
        fs::create_dir_all(dir).map_err(|e| VhostError::Io {
            path: dir.display().to_string(),
            detail: e.to_string(),
        })?;
    }
    fs::write(&rendered.path, &rendered.body).map_err(|e| VhostError::Io {
        path: rendered.path.display().to_string(),
        detail: e.to_string(),
    })
}

/// Reconcile `projects` against the configs on disk.
pub fn sync(
    projects: &[Project],
    server: Server,
    ctx: &Context<'_>,
) -> Result<SyncReport, VhostError> {
    let (report, writes, prunes) = plan(projects, server, ctx)?;

    // Create the target directory before writing: a fresh workspace may not
    // have conf/apache/sites-enabled yet.
    if !writes.is_empty() {
        let sites = ctx.sites_dir(server);
        fs::create_dir_all(&sites).map_err(|e| VhostError::Io {
            path: sites.display().to_string(),
            detail: e.to_string(),
        })?;
    }

    for (path, body) in writes {
        fs::write(&path, body).map_err(|e| VhostError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
    }
    for path in prunes {
        fs::remove_file(&path).map_err(|e| VhostError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
    }
    Ok(report)
}

/// Report what `sync` would do, without changing anything.
pub fn sync_dry_run(
    projects: &[Project],
    server: Server,
    ctx: &Context<'_>,
) -> Result<SyncReport, VhostError> {
    let (report, _, _) = plan(projects, server, ctx)?;
    Ok(report)
}

type Plan = (SyncReport, Vec<(PathBuf, String)>, Vec<PathBuf>);

fn plan(projects: &[Project], server: Server, ctx: &Context<'_>) -> Result<Plan, VhostError> {
    let sites = ctx.sites_dir(server);
    let mut report = SyncReport::default();
    let mut writes: Vec<(PathBuf, String)> = Vec::new();
    let mut prunes: Vec<PathBuf> = Vec::new();

    // Desired state.
    let mut desired: Vec<PathBuf> = Vec::new();
    for project in projects {
        let rendered = render_project(project, server, ctx, None)?;
        desired.push(rendered.path.clone());
        writes.push((rendered.path.clone(), rendered.body.clone()));
        report.written.push(SyncAction {
            path: rendered.path,
            hostname: rendered.hostname,
            body: rendered.body,
        });
    }

    // Actual state: only our own files are candidates for pruning.
    if sites.is_dir() {
        let entries = fs::read_dir(&sites).map_err(|e| VhostError::Io {
            path: sites.display().to_string(),
            detail: e.to_string(),
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if desired.contains(&path) {
                continue;
            }
            if !is_managed(&path) {
                continue; // hand-written config: leave it alone
            }
            let hostname = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            report.pruned.push(SyncAction {
                path: path.clone(),
                hostname,
                body: String::new(),
            });
            prunes.push(path);
        }
    }

    Ok((report, writes, prunes))
}

/// Whether `path` is a config we generated.
fn is_managed(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|s| s.contains(MANAGED_MARKER))
        .unwrap_or(false)
}
