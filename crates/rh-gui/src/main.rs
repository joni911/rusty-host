//! Rusty Host tray application.
//!
//! Deliberately thin (PRD §4.7): every piece of logic lives in `rh-core`, and
//! this binary only presents it and translates clicks into library calls. That
//! keeps the GUI honest — it cannot drift from the CLI's behaviour because it
//! does not have its own.

mod model;
pub mod theme;

slint::include_modules!();

use std::path::PathBuf;

use model::{GuiState, ServiceView};
use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());

    let window = MainWindow::new()?;

    // Initial state.
    let state = GuiState::load(root.clone());
    apply(&window, &state);

    // ---- callbacks
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_start_all(move || {
            let mut st = GuiState::load(root.clone());
            st.start_all();
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_stop_all(move || {
            let mut st = GuiState::load(root.clone());
            st.stop_all();
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_toggle_apache(move || {
            let mut st = GuiState::load(root.clone());
            st.toggle("apache");
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_toggle_mariadb(move || {
            let mut st = GuiState::load(root.clone());
            st.toggle("mariadb");
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_rescan(move || {
            let st = GuiState::load(root.clone());
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let w = window.as_weak();
        let root = root.clone();
        window.on_open_www(move || {
            let st = GuiState::load(root.clone());
            st.open_path(&st.www_dir);
            if let Some(w) = w.upgrade() {
                apply(&w, &st);
            }
        });
    }
    {
        let root = root.clone();
        window.on_open_phpmyadmin(move || {
            let st = GuiState::load(root.clone());
            st.open_url(&st.phpmyadmin_url);
        });
    }
    {
        let root = root.clone();
        window.on_open_dbx(move || {
            let st = GuiState::load(root.clone());
            st.launch_dbx();
        });
    }
    {
        let root = root.clone();
        window.on_open_terminal(move || {
            let st = GuiState::load(root.clone());
            st.open_terminal();
        });
    }

    window.run()
}

/// Push model state into the view.
fn apply(w: &MainWindow, st: &GuiState) {
    w.set_workspace_ready(st.ready);
    w.set_workspace(st.workspace.clone().into());
    w.set_tld(st.tld.clone().into());
    w.set_status_line(st.status.clone().into());

    set_service(w, "apache", &st.apache);
    set_service(w, "mariadb", &st.mariadb);

    let names: Vec<slint::SharedString> = st
        .projects
        .iter()
        .map(|p| p.name.clone().into())
        .collect();
    let urls: Vec<slint::SharedString> = st
        .projects
        .iter()
        .map(|p| p.url.clone().into())
        .collect();
    w.set_project_names(slint::ModelRc::new(slint::VecModel::from(names)));
    w.set_project_urls(slint::ModelRc::new(slint::VecModel::from(urls)));
}

fn set_service(w: &MainWindow, which: &str, view: &ServiceView) {
    match which {
        "apache" => {
            w.set_apache_running(view.running);
            w.set_apache_detail(view.detail.clone().into());
        }
        "mariadb" => {
            w.set_mariadb_running(view.running);
            w.set_mariadb_detail(view.detail.clone().into());
        }
        _ => {}
    }
}
