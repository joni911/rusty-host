//! TDD-10 — Process supervision.
//!
//! Refs: PRD §4.6, §3.3 items 26–28, 31.
//!
//! The dangerous case this module exists to prevent: a stale PID file whose PID
//! has been recycled by an unrelated process. Killing that would be a serious
//! bug, so every recorded PID is validated against the executable path *and* a
//! start-time fingerprint before we ever signal it.
//!
//! Tests use the OS's own long-lived and short-lived processes rather than
//! spawning services, so they are fast and hermetic.

mod common;
use common::TempDir;

use rh_core::proc::{self, ServiceState, Supervisor};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

// A trivially available long-lived process on Windows and Unix alike.
#[cfg(windows)]
const SLEEPER: (&str, &[&str]) = ("cmd.exe", &["/C", "ping -n 60 127.0.0.1 >NUL"]);
#[cfg(not(windows))]
const SLEEPER: (&str, &[&str]) = ("sh", &["-c", "sleep 60"]);

fn supervisor(root: &Path) -> Supervisor {
    Supervisor::new(root.join("data").join("run"))
}

#[test]
fn spawn_records_pid_and_metadata() {
    let d = TempDir::new("p1");
    let sup = supervisor(d.path());

    let rec = sup.spawn("test", SLEEPER.0, SLEEPER.1, d.path()).unwrap();
    assert!(rec.pid > 0, "must record a real pid");
    assert_eq!(rec.service, "test");
    assert!(rec.exe.ends_with(SLEEPER.0) || rec.exe.contains(SLEEPER.0));

    let loaded = sup.load("test").unwrap().expect("record persisted");
    assert_eq!(loaded.pid, rec.pid);
    assert_eq!(loaded.service, "test");

    sup.stop("test").ok();
}

#[test]
fn record_is_written_to_disk_and_removed_on_stop() {
    let d = TempDir::new("p2");
    let sup = supervisor(d.path());

    sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();
    assert!(sup.record_path("svc").is_file(), "record must exist while running");

    sup.stop("svc").unwrap();
    assert!(
        !sup.record_path("svc").is_file(),
        "record must be removed after stop"
    );
}

#[test]
fn state_reports_running_for_a_live_child() {
    let d = TempDir::new("p3");
    let sup = supervisor(d.path());
    sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();

    assert_eq!(sup.state("svc"), ServiceState::Running);

    sup.stop("svc").ok();
}

#[test]
fn state_reports_stopped_when_nothing_recorded() {
    let d = TempDir::new("p4");
    let sup = supervisor(d.path());
    assert_eq!(sup.state("never-started"), ServiceState::Stopped);
}

#[test]
fn second_start_is_a_noop_when_running() {
    let d = TempDir::new("p5");
    let sup = supervisor(d.path());

    let first = sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();
    let second = sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();

    assert_eq!(first.pid, second.pid, "must not spawn a second instance");

    sup.stop("svc").ok();
}

#[test]
fn stop_terminates_and_removes_record() {
    let d = TempDir::new("p6");
    let sup = supervisor(d.path());
    let rec = sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();

    sup.stop("svc").unwrap();

    // Give the OS a moment to reap.
    thread::sleep(Duration::from_millis(300));
    assert!(!proc::pid_alive(rec.pid), "process must be gone");
    assert_eq!(sup.state("svc"), ServiceState::Stopped);
}

#[test]
fn stop_on_never_started_is_not_an_error() {
    let d = TempDir::new("p7");
    let sup = supervisor(d.path());
    assert!(sup.stop("nothing").is_ok(), "stopping nothing must be benign");
}

#[test]
fn stale_pid_file_is_detected_and_cleaned() {
    let d = TempDir::new("p8");
    let sup = supervisor(d.path());

    // A PID that certainly no longer exists.
    let dead = proc::find_dead_pid();
    let rec = proc::ProcessRecord {
        service: "ghost".into(),
        pid: dead,
        exe: "does-not-exist.exe".into(),
        started_at: 0,
        cmdline_sha256: "0".repeat(64),
        stdout_log: None,
        stderr_log: None,
    };
    sup.save(&rec).unwrap();

    assert_eq!(sup.state("ghost"), ServiceState::Stale);

    // A fresh spawn over a stale record must clean it and succeed.
    let fresh = sup.spawn("ghost", SLEEPER.0, SLEEPER.1, d.path()).unwrap();
    assert_ne!(fresh.pid, dead);

    sup.stop("ghost").ok();
}

#[test]
fn pid_reuse_is_detected_by_exe_path() {
    // THE dangerous case. A live PID that belongs to a different executable must
    // never be treated as ours, and must never be killed.
    let d = TempDir::new("p9");
    let sup = supervisor(d.path());

    let rec = sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();

    // Forge a record claiming this live PID belongs to something else.
    let forged = proc::ProcessRecord {
        service: "svc".into(),
        pid: rec.pid,
        exe: "totally-different-binary.exe".into(),
        started_at: rec.started_at,
        cmdline_sha256: "f".repeat(64),
        stdout_log: None,
        stderr_log: None,
    };
    sup.save(&forged).unwrap();

    assert_eq!(
        sup.state("svc"),
        ServiceState::Stale,
        "a PID now owned by another exe must be treated as stale, not ours"
    );

    // Stopping it must NOT kill the unrelated process.
    sup.stop("svc").ok();
    thread::sleep(Duration::from_millis(200));
    assert!(
        proc::pid_alive(rec.pid),
        "stop must never kill a process we do not own"
    );

    // Clean up for real.
    sup.spawn("svc", SLEEPER.0, SLEEPER.1, d.path()).ok();
}

#[test]
fn start_times_out_and_reports_when_process_dies_immediately() {
    let d = TempDir::new("p10");
    let sup = supervisor(d.path());

    // A command that exits at once: waiting for readiness must fail, not hang.
    #[cfg(windows)]
    let (exe, args): (&str, &[&str]) = ("cmd.exe", &["/C", "exit 1"]);
    #[cfg(not(windows))]
    let (exe, args): (&str, &[&str]) = ("sh", &["-c", "exit 1"]);

    let err = sup
        .start_and_wait("fast", exe, args, d.path(), Duration::from_millis(800), None)
        .unwrap_err();

    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("exit") || msg.contains("stopped") || msg.contains("failed"),
        "error must explain the process did not stay up: {msg}"
    );
}

#[test]
fn start_and_wait_succeeds_when_process_stays_up() {
    let d = TempDir::new("p11");
    let sup = supervisor(d.path());

    let rec = sup
        .start_and_wait(
            "lived",
            SLEEPER.0,
            SLEEPER.1,
            d.path(),
            Duration::from_secs(5),
            None,
        )
        .unwrap();
    assert!(rec.pid > 0);
    assert_eq!(sup.state("lived"), ServiceState::Running);

    sup.stop("lived").ok();
}

#[test]
fn outputs_are_captured_to_log_files() {
    let d = TempDir::new("p12");
    let sup = supervisor(d.path());

    #[cfg(windows)]
    let (exe, args): (&str, &[&str]) =
        ("cmd.exe", &["/C", "echo hello-from-stdout && echo err-line 1>&2"]);
    #[cfg(not(windows))]
    let (exe, args): (&str, &[&str]) =
        ("sh", &["-c", "echo hello-from-stdout; echo err-line 1>&2"]);

    let rec = proc::spawn_captured(&sup, "logger", exe, args, d.path()).unwrap();

    // Wait for the output to land.
    for _ in 0..40 {
        if let Some(p) = &rec.stdout_log {
            if let Ok(out) = fs::read_to_string(p) {
                if out.contains("hello-from-stdout") {
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    let out = rec.stdout_log.as_ref().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    let err = rec.stderr_log.as_ref().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
    assert!(out.contains("hello-from-stdout"), "stdout not captured: {out:?}");
    assert!(err.contains("err-line"), "stderr not captured: {err:?}");
}

#[test]
fn log_paths_live_under_data_logs() {
    let d = TempDir::new("p13");
    let sup = supervisor(d.path());
    let rec = proc::spawn_captured(&sup, "svc", SLEEPER.0, SLEEPER.1, d.path()).unwrap();

    let out = rec.stdout_log.expect("stdout log path recorded");
    assert!(
        out.to_string_lossy().contains("logs"),
        "logs belong under data/logs: {}",
        out.display()
    );
    assert!(out.to_string_lossy().contains("svc"));

    sup.stop("svc").ok();
}

#[test]
fn record_roundtrips_through_json() {
    let d = TempDir::new("p14");
    let sup = supervisor(d.path());
    let rec = proc::ProcessRecord {
        service: "x".into(),
        pid: 4242,
        exe: "C:/bin/x.exe".into(),
        started_at: 1234567890,
        cmdline_sha256: "a".repeat(64),
        stdout_log: None,
        stderr_log: None,
    };
    sup.save(&rec).unwrap();
    let back = sup.load("x").unwrap().unwrap();
    assert_eq!(back, rec);
}

#[test]
fn corrupt_record_is_treated_as_absent_not_fatal() {
    let d = TempDir::new("p15");
    let sup = supervisor(d.path());
    fs::create_dir_all(d.path().join("data").join("run")).unwrap();
    fs::write(sup.record_path("broken"), "{ not json").unwrap();

    assert_eq!(sup.state("broken"), ServiceState::Stopped);
    assert!(sup.load("broken").is_ok());
}

#[test]
fn cmdline_hash_is_stable_and_distinguishes_commands() {
    let a = proc::cmdline_hash("apache", &["-f", "conf/httpd.conf"]);
    let b = proc::cmdline_hash("apache", &["-f", "conf/httpd.conf"]);
    let c = proc::cmdline_hash("apache", &["-f", "other.conf"]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.len(), 64);
}
