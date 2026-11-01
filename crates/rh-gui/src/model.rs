//! GUI model — the testable half of the tray app.
//!
//! Everything here calls into `rh-core`; nothing reimplements it. The Slint
//! layer only reads this struct and reports clicks back, so all the logic worth
//! testing lives in plain Rust.

use std::path::PathBuf;
use std::process::Command;

use rh_core::ports;
use rh_core::proc::{ServiceState, Supervisor};
use rh_core::project;
use rh_core::settings::Settings;
use rh_core::workspace::{DiscoverError, Workspace};

/// How one service should be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceView {
    /// Whether it reports as running.
    pub running: bool,
    /// One-line detail for the row (port, or why it is not running).
    pub detail: String,
}

impl ServiceView {
    fn stopped(detail: impl Into<String>) -> Self {
        Self {
            running: false,
            detail: detail.into(),
        }
    }
}

/// One project row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectView {
    /// Directory name.
    pub name: String,
    /// Clickable URL.
    pub url: String,
}

/// Everything the window renders.
#[derive(Debug, Clone)]
pub struct GuiState {
    /// Workspace root in use.
    pub workspace: String,
    /// Whether the workspace is initialised.
    pub ready: bool,
    /// Default TLD.
    pub tld: String,
    /// Status line text.
    pub status: String,
    /// Apache state.
    pub apache: ServiceView,
    /// MariaDB state.
    pub mariadb: ServiceView,
    /// Projects on disk.
    pub projects: Vec<ProjectView>,
    /// Path to `www`.
    pub www_dir: PathBuf,
    /// phpMyAdmin URL.
    pub phpmyadmin_url: String,

    root: Option<PathBuf>,
}

impl GuiState {
    /// Read the current state from disk.
    pub fn load(root: Option<PathBuf>) -> Self {
        let ws = match &root {
            Some(r) => Workspace::at(r),
            None => match std::env::current_dir() {
                Ok(cwd) => match Workspace::discover(&cwd) {
                    Ok(w) => w,
                    Err(DiscoverError::NotFound { .. }) => Workspace::at(cwd),
                },
                Err(_) => Workspace::at("."),
            },
        };

        let ready = ws.lock_path().is_file() && ws.www().is_dir();
        let settings = Settings::load(&ws).unwrap_or_default();
        let sup = Supervisor::new(ws.data().join("run"));

        let apache = service_view(&sup, "apache", settings.port("http"));
        let mariadb = service_view(&sup, "mariadb", settings.port("mysql"));
        // Compute these before any field is moved into the struct below.
        let pma_url = phpmyadmin_url(&settings);
        let tld = settings.tld.clone();

        let projects = if ready {
            match project::scan(&ws.www(), &settings.tld) {
Ok(list) => list
                    .into_iter()
                    .map(|p| ProjectView {
                        url: p.url("http"),
                        name: p.name,
                    })
                    .collect(),
                Err(e) => {
                    return Self {
                        workspace: ws.root().display().to_string(),
                        ready,
                        tld: tld.clone(),
                        status: format!("project scan failed: {e}"),
                        apache,
                        mariadb,
                        projects: Vec::new(),
                        www_dir: ws.www(),
                        phpmyadmin_url: pma_url.clone(),
                        root,
                    };
                }
            }
        } else {
            Vec::new()
        };

        let status = if ready {
            format!(
                "{} project(s) · http :{} · mysql :{}",
                projects.len(),
                settings.port("http"),
                settings.port("mysql")
            )
        } else {
            "No workspace here. Run `rh init` in your chosen folder, then reopen.".to_string()
        };

        Self {
            workspace: ws.root().display().to_string(),
            ready,
            tld,
            status,
            apache,
            mariadb,
            projects,
            www_dir: ws.www(),
            phpmyadmin_url: pma_url,
            root,
        }
    }

    fn workspace(&self) -> Workspace {
        match &self.root {
            Some(r) => Workspace::at(r),
            None => Workspace::at("."),
        }
    }

    /// Start every managed service.
    pub fn start_all(&mut self) {
        let mut started = Vec::new();
        for name in ["apache", "mariadb"] {
            if self.start_service(name) {
                started.push(name);
            }
        }
        self.status = if started.is_empty() {
            "nothing to start — install a component first".to_string()
        } else {
            format!("started: {}", started.join(", "))
        };
        self.refresh_services();
    }

    /// Stop every managed service.
    pub fn stop_all(&mut self) {
        let sup = Supervisor::new(self.workspace().data().join("run"));
        for name in ["apache", "mariadb"] {
            if let Err(e) = sup.stop(name) {
                self.status = format!("could not stop {name}: {e}");
                self.refresh_services();
                return;
            }
        }
        self.status = "all services stopped".to_string();
        self.refresh_services();
    }

    /// Toggle one service.
    pub fn toggle(&mut self, service: &str) {
        let sup = Supervisor::new(self.workspace().data().join("run"));
        match sup.state(service) {
            ServiceState::Running => {
                match sup.stop(service) {
                    Ok(()) => self.status = format!("stopped {service}"),
                    Err(e) => self.status = format!("could not stop {service}: {e}"),
                }
            }
            _ => {
                if self.start_service(service) {
                    self.status = format!("started {service}");
                }
            }
        }
        self.refresh_services();
    }

    fn start_service(&self, service: &str) -> bool {
        let ws = self.workspace();
        let lock = match ws.load_lock() {
            Ok(l) => l,
            Err(_) => return false,
        };
        if !lock.has(service) {
            return false;
        }

        let settings = Settings::load(&ws).unwrap_or_default();
        let port = match service {
            "apache" => settings.port("http"),
            "mariadb" => settings.port("mysql"),
            _ => 0,
        };
        if port != 0 && !ports::probe(port).is_free() {
            return false;
        }

        let versions = lock.versions_of(service);
        let Some(version) = versions.last() else {
            return false;
        };
        let dir = ws.component_dir(service, version);
        let exe = match service {
            "apache" => dir.join("bin").join("httpd.exe"),
            "mariadb" => dir.join("bin").join("mariadbd.exe"),
            _ => return false,
        };
        if !exe.is_file() {
            return false;
        }

        let sup = Supervisor::new(ws.data().join("run"));
        match service {
            "apache" => sup
                .start_and_wait(
                    "apache",
                    &exe.display().to_string(),
                    &["-f", &ws.conf().join("apache").join("httpd.conf").display().to_string()],
                    ws.root(),
                    std::time::Duration::from_secs(20),
                    Some(port),
                )
                .is_ok(),
            "mariadb" => sup
                .start_and_wait(
                    "mariadb",
                    &exe.display().to_string(),
                    &[&format!(
                        "--defaults-file={}",
                        ws.conf().join("mariadb").join("my.ini").display()
                    )],
                    ws.root(),
                    std::time::Duration::from_secs(30),
                    Some(port),
                )
                .is_ok(),
            _ => false,
        }
    }

    fn refresh_services(&mut self) {
        let settings = Settings::load(&self.workspace()).unwrap_or_default();
        let sup = Supervisor::new(self.workspace().data().join("run"));
        self.apache = service_view(&sup, "apache", settings.port("http"));
        self.mariadb = service_view(&sup, "mariadb", settings.port("mysql"));
    }

    /// Open a path with the OS file manager.
    pub fn open_path(&self, path: &std::path::Path) {
        open_target(&path.display().to_string());
    }

    /// Open a URL in the default browser.
    pub fn open_url(&self, url: &str) {
        open_target(url);
    }

    /// Launch the portable database client if it is installed.
    pub fn launch_dbx(&self) {
        let ws = self.workspace();
        let Ok(lock) = ws.load_lock() else { return };
        let Some(version) = lock.versions_of("dbx").last().cloned() else {
            return;
        };
        let exe = ws.component_dir("dbx", &version).join("dbx.exe");
        if exe.is_file() {
            open_target(&exe.display().to_string());
        }
    }

    /// Open a terminal in the workspace.
    ///
    /// Until `rh-term` exists (TDD-15), this falls back to the OS terminal so the
    /// button is never dead.
    pub fn open_terminal(&self) {
        let dir = self.workspace().root().to_path_buf();
        #[cfg(windows)]
        {
            let _ = Command::new("cmd.exe")
                .arg("/C")
                .arg("start")
                .arg("cmd.exe")
                .current_dir(&dir)
                .spawn();
        }
        #[cfg(not(windows))]
        {
            let term = std::env::var("TERMINAL").unwrap_or_else(|_| "x-terminal-emulator".into());
            let _ = Command::new(term).current_dir(&dir).spawn();
        }
    }
}

fn phpmyadmin_url(settings: &Settings) -> String {
    format!("http://localhost:{}/", settings.port("phpmyadmin"))
}

fn service_view(sup: &Supervisor, name: &str, port: u16) -> ServiceView {
    match sup.state(name) {
        ServiceState::Running => ServiceView {
            running: true,
            detail: format!("127.0.0.1:{port}"),
        },
        ServiceState::Stale => ServiceView::stopped("stale state file"),
        ServiceState::Stopped => {
            if port != 0 && !ports::probe(port).is_free() {
                ServiceView::stopped(format!("port {port} busy"))
            } else {
                ServiceView::stopped(format!("port {port}"))
            }
        }
    }
}

/// Hand a path or URL to the OS.
fn open_target(target: &str) {
    #[cfg(windows)]
    {
        let _ = Command::new("cmd.exe")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(target)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = Command::new(opener).arg(target).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rh-gui-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let ws = Workspace::at(&p);
        ws.init().unwrap();
        p
    }

    #[test]
    fn state_loads_for_a_fresh_workspace() {
        let p = tmp_ws("fresh");
        let st = GuiState::load(Some(p.clone()));
        assert!(st.ready, "an initialised workspace is ready");
        assert_eq!(st.tld, "test");
        assert!(st.projects.is_empty());
        assert!(!st.apache.running);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn state_is_not_ready_without_a_workspace() {
        let p = std::env::temp_dir().join(format!("rh-gui-nope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        let st = GuiState::load(Some(p.clone()));
        assert!(!st.ready);
        assert!(st.status.to_lowercase().contains("rh init"), "{}", st.status);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn projects_appear_as_urls() {
        let p = tmp_ws("proj");
        std::fs::create_dir_all(p.join("www").join("My Shop")).unwrap();
        let st = GuiState::load(Some(p.clone()));
        assert_eq!(st.projects.len(), 1);
        assert_eq!(st.projects[0].name, "My Shop");
        assert_eq!(st.projects[0].url, "http://my-shop.test");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn phpmyadmin_url_uses_the_configured_port() {
        let mut s = Settings::default();
        s.ports.insert("phpmyadmin".into(), 9090);
        assert_eq!(phpmyadmin_url(&s), "http://localhost:9090/");
    }

    #[test]
    fn stop_all_on_nothing_is_benign() {
        let p = tmp_ws("stop");
        let mut st = GuiState::load(Some(p.clone()));
        st.stop_all();
        assert!(st.status.contains("stopped"), "{}", st.status);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn start_all_without_components_reports_clearly() {
        let p = tmp_ws("start");
        let mut st = GuiState::load(Some(p.clone()));
        st.start_all();
        assert!(
            st.status.contains("install"),
            "must tell the user why nothing started: {}",
            st.status
        );
        let _ = std::fs::remove_dir_all(&p);
    }
}
