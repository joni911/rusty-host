//! Port probing and conflict reporting (PRD §3.3 items 29, 30).
//!
//! A pre-flight bind test is cheap and turns "the server won't start" into a
//! specific, actionable message before we spawn anything.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};

/// What we found out about a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortState {
    /// Nothing is listening; we can bind.
    Free,
    /// Something holds the port.
    InUse { port: u16 },
}

impl PortState {
    /// The port this state refers to.
    pub fn port(&self) -> u16 {
        match self {
            PortState::Free => 0,
            PortState::InUse { port } => *port,
        }
    }

    /// Whether binding should succeed.
    pub fn is_free(&self) -> bool {
        matches!(self, PortState::Free)
    }
}

/// A detected conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// Service that wanted the port.
    pub service: String,
    /// The port in question.
    pub port: u16,
}

/// Why a port could not be described.
#[derive(Debug)]
pub struct PortError {
    /// Service involved.
    pub service: String,
    /// Port involved.
    pub port: u16,
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "port {} is in use, so `{}` cannot start.\n\
             Run `rh ports` to see what is holding it, or change the port in rh.toml.",
            self.port, self.service
        )
    }
}

impl std::error::Error for PortError {}

/// Probe whether `port` can be bound on loopback.
///
/// Port 0 always reports `Free`: it means "let the OS pick", so it can never
/// conflict.
pub fn probe(port: u16) -> PortState {
    if port == 0 {
        return PortState::Free;
    }
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            PortState::Free
        }
        Err(_) => PortState::InUse { port },
    }
}

/// Build the error for a conflicted service port.
pub fn conflict_error(service: &str, port: u16) -> Result<(), PortError> {
    if probe(port).is_free() {
        return Ok(());
    }
    Err(PortError {
        service: service.to_string(),
        port,
    })
}

/// First free port at or after `from`, searching a bounded range.
pub fn next_free_port_from(from: u16) -> Option<u16> {
    let start = from.max(1);
    let end = start.saturating_add(1000);
    (start..end).find(|p| probe(*p).is_free())
}

/// Check every (service, port) pair and return the conflicts.
pub fn preflight(wanted: &[(String, u16)]) -> Vec<Conflict> {
    wanted
        .iter()
        .filter(|(_, port)| !probe(*port).is_free())
        .map(|(service, port)| Conflict {
            service: service.clone(),
            port: *port,
        })
        .collect()
}

/// The default port set written into `rh.toml`. Distinct by construction.
pub fn default_ports() -> Vec<(String, u16)> {
    vec![
        ("http".to_string(), 80),
        ("https".to_string(), 443),
        ("mysql".to_string(), 3306),
        ("phpmyadmin".to_string(), 8080),
        ("mailpit-smtp".to_string(), 1025),
        ("mailpit-ui".to_string(), 8025),
    ]
}
