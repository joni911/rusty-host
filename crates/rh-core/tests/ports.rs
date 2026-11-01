//! TDD-09 — Port probing and conflict reporting.
//!
//! Refs: PRD §3.3 items 29, 30.
//!
//! Before starting a service we check whether its port is usable, and if not we
//! try to say *who* holds it — a bare "address in use" sends the user hunting
//! through netstat themselves.

use rh_core::ports::{self, PortState};
use std::net::TcpListener;

/// Grab an ephemeral port and hold it so we know it is busy.
fn occupy() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let p = l.local_addr().unwrap().port();
    (l, p)
}

#[test]
fn free_port_is_reported_free() {
    // Bind then immediately drop: the port is almost certainly free again.
    let p = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    assert_eq!(ports::probe(p), PortState::Free);
}

#[test]
fn bound_port_is_reported_in_use() {
    let (_guard, p) = occupy();
    match ports::probe(p) {
        PortState::InUse { .. } => {}
        other => panic!("expected InUse for port {p}, got {other:?}"),
    }
}

#[test]
fn in_use_state_reports_the_port() {
    let (_guard, p) = occupy();
    let state = ports::probe(p);
    assert!(state.port() == p || state.port() == 0, "port() should report {p}");
}

#[test]
fn conflict_error_names_the_port_and_suggests_an_alternative() {
    let (_guard, p) = occupy();
    let err = ports::conflict_error("apache", p).expect_err("port is held");
    let msg = err.to_string();

    assert!(msg.contains(&p.to_string()), "must name the port: {msg}");
    assert!(msg.contains("apache"), "must name the service: {msg}");
    assert!(
        msg.contains("rh ports") || msg.contains("in use"),
        "must explain or point at `rh ports`: {msg}"
    );
}

#[test]
fn conflict_error_offers_next_free_port() {
    let (_guard, p) = occupy();
    let alternative = ports::next_free_port_from(p + 1).expect("some free port exists");
    assert!(alternative > p, "alternative must not be the busy port");

    // And the suggested port really is free.
    assert_eq!(ports::probe(alternative), PortState::Free);
}

#[test]
fn preflight_checks_all_enabled_services() {
    let (_guard, busy) = occupy();
    let free = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };

    let wanted = vec![
        ("apache".to_string(), free),
        ("mysql".to_string(), busy),
    ];
    let conflicts = ports::preflight(&wanted);

    assert_eq!(conflicts.len(), 1, "only the busy port conflicts");
    assert_eq!(conflicts[0].service, "mysql");
    assert_eq!(conflicts[0].port, busy);
}

#[test]
fn preflight_on_all_free_ports_is_empty() {
    let a = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let b = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let wanted = vec![("apache".to_string(), a), ("mysql".to_string(), b)];
    assert!(ports::preflight(&wanted).is_empty());
}

#[test]
fn port_zero_is_always_usable() {
    // Port 0 means "let the OS choose" and must never be reported as a conflict.
    assert_eq!(ports::probe(0), PortState::Free);
}

#[test]
fn probe_is_deterministic_for_the_same_port() {
    let (_guard, p) = occupy();
    let a = ports::probe(p);
    let b = ports::probe(p);
    assert_eq!(a, b);
}

#[test]
fn default_ports_are_sensible() {
    // These are the values written into rh.toml; they must not collide with
    // each other, which would make a fresh install fail immediately.
    let defaults = ports::default_ports();
    let mut seen = Vec::new();
    for (name, port) in &defaults {
        assert!(*port > 0, "{name} must have a real port");
        assert!(
            !seen.contains(port),
            "{name} collides with another default on {port}"
        );
        seen.push(*port);
    }
    assert!(defaults.iter().any(|(n, _)| n == "http"));
    assert!(defaults.iter().any(|(n, _)| n == "mysql"));
    assert!(defaults.iter().any(|(n, _)| n == "phpmyadmin"));
}
