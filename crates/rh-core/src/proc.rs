//! Process supervision (PRD §4.6).
//!
//! Services are ordinary child processes with a PID record, not Windows
//! services — that is what keeps the workspace portable.
//!
//! The record exists so a later `rh` invocation can find a service started by an
//! earlier one. That makes PID validation the critical safety property:
//!
//! * a recorded PID that is no longer alive is **stale** → cleaned silently;
//! * a recorded PID that is alive but now belongs to a **different executable**
//!   is also **stale** → we must never signal it, because the OS recycles PIDs
//!   and killing it would hit an unrelated process.
//!
//! Both cases are tested, the second one deliberately, because it is the one
//! that can destroy someone's work.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What we know about a running service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRecord {
    /// Service name (`apache`, `mariadb`, …).
    pub service: String,
    /// Operating-system process id.
    pub pid: u32,
    /// Executable path we launched, used to detect PID reuse.
    pub exe: String,
    /// Seconds since the Unix epoch when we started it.
    pub started_at: u64,
    /// Hash of the argv we launched, an extra fingerprint against PID reuse.
    pub cmdline_sha256: String,
    /// Where the child's stdout is being written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_log: Option<PathBuf>,
    /// Where the child's stderr is being written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_log: Option<PathBuf>,
}

/// What a service looks like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    /// No record on disk.
    Stopped,
    /// Recorded and the process is alive and matches our fingerprint.
    Running,
    /// Recorded but the process is gone, or the PID now belongs to something else.
    Stale,
}

impl fmt::Display for ServiceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceState::Stopped => write!(f, "stopped"),
            ServiceState::Running => write!(f, "running"),
            ServiceState::Stale => write!(f, "stale"),
        }
    }
}

/// Why a process operation failed.
#[derive(Debug)]
pub enum ProcError {
    /// Could not spawn.
    Spawn { exe: String, detail: String },
    /// Could not write or read a record.
    Record { path: String, detail: String },
    /// The process exited before becoming ready.
    DiedEarly { service: String, detail: String },
    /// The process did not become ready in time.
    NotReady { service: String, timeout_ms: u64 },
    /// Could not terminate.
    Stop { service: String, detail: String },
}

impl fmt::Display for ProcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcError::Spawn { exe, detail } => {
                write!(f, "could not start {exe}: {detail}")
            }
            ProcError::Record { path, detail } => {
                write!(f, "could not write state file {path}: {detail}")
            }
            ProcError::DiedEarly { service, detail } => write!(
                f,
                "`{service}` stopped immediately after starting: {detail}"
            ),
            ProcError::NotReady { service, timeout_ms } => write!(
                f,
                "`{service}` did not become ready within {timeout_ms} ms; check its log"
            ),
            ProcError::Stop { service, detail } => {
                write!(f, "could not stop `{service}`: {detail}")
            }
        }
    }
}

impl std::error::Error for ProcError {}

/// Owns the run directory and is the only way to touch PID records.
#[derive(Debug, Clone)]
pub struct Supervisor {
    run_dir: PathBuf,
    logs_dir: PathBuf,
}

impl Supervisor {
    /// Create a supervisor whose records live under `run_dir`.
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        let run_dir = run_dir.into();
        // Sibling `logs/` next to `run/`.
        let logs_dir = run_dir
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("logs"));
        Self { run_dir, logs_dir }
    }

    /// Path of a service's record file.
    pub fn record_path(&self, service: &str) -> PathBuf {
        self.run_dir.join(format!("{service}.json"))
    }

    /// Directory holding captured output.
    pub fn logs_dir(&self) -> &Path {
        &self.logs_dir
    }

    /// Persist a record.
    pub fn save(&self, rec: &ProcessRecord) -> Result<(), ProcError> {
        fs::create_dir_all(&self.run_dir).map_err(|e| ProcError::Record {
            path: self.run_dir.display().to_string(),
            detail: e.to_string(),
        })?;
        let path = self.record_path(&rec.service);
        let body = serde_json::to_string_pretty(rec).map_err(|e| ProcError::Record {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        // Atomic: a crash must not leave a torn record that would later look
        // like a live service.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, body).map_err(|e| ProcError::Record {
            path: tmp.display().to_string(),
            detail: e.to_string(),
        })?;
        fs::rename(&tmp, &path).map_err(|e| ProcError::Record {
            path: path.display().to_string(),
            detail: e.to_string(),
        })
    }

    /// Read a record. Corrupt records are reported as absent rather than fatal:
    /// a broken state file must not block the whole tool.
    pub fn load(&self, service: &str) -> Result<Option<ProcessRecord>, ProcError> {
        let path = self.record_path(service);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).ok()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ProcError::Record {
                path: path.display().to_string(),
                detail: e.to_string(),
            }),
        }
    }

    /// Classify a service.
    pub fn state(&self, service: &str) -> ServiceState {
        let Ok(Some(rec)) = self.load(service) else {
            return ServiceState::Stopped;
        };
        if is_ours(&rec) {
            ServiceState::Running
        } else {
            ServiceState::Stale
        }
    }

    /// Spawn a service, refusing to double-start one that is already running.
    pub fn spawn(
        &self,
        service: &str,
        exe: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<ProcessRecord, ProcError> {
        // If something is genuinely running, this is a no-op returning its record.
        if let Ok(Some(existing)) = self.load(service) {
            if is_ours(&existing) {
                return Ok(existing);
            }
            // Stale: clear it so we can start cleanly.
            let _ = fs::remove_file(self.record_path(service));
        }

        let mut cmd = Command::new(exe);
        cmd.args(args).current_dir(cwd).stdin(Stdio::null());

        let child = cmd.spawn().map_err(|e| ProcError::Spawn {
            exe: exe.to_string(),
            detail: e.to_string(),
        })?;

        let rec = ProcessRecord {
            service: service.to_string(),
            pid: child.id(),
            exe: resolved_exe(exe),
            started_at: now_secs(),
            cmdline_sha256: cmdline_hash(exe, args),
            stdout_log: None,
            stderr_log: None,
        };
        self.save(&rec)?;

        // The child must outlive this call; detach it so it is not killed when
        // our `Child` handle drops.
        std::mem::forget(child);
        Ok(rec)
    }

    /// Terminate a service, if it is genuinely ours.
    ///
    /// A stale record is simply removed — we never signal a PID we do not own.
    pub fn stop(&self, service: &str) -> Result<(), ProcError> {
        let Some(rec) = self.load(service)? else {
            return Ok(());
        };

        if is_ours(&rec) {
            terminate(rec.pid).map_err(|e| ProcError::Stop {
                service: service.to_string(),
                detail: e.to_string(),
            })?;
            wait_gone(rec.pid, Duration::from_secs(5));
        }

        fs::remove_file(self.record_path(service)).map_err(|e| ProcError::Record {
            path: self.record_path(service).display().to_string(),
            detail: e.to_string(),
        })?;
        Ok(())
    }

    /// Spawn and wait until the service is observably alive.
    ///
    /// `ready` is an optional TCP port to wait on; without it we simply require
    /// the process to still be running after a short settle period.
    pub fn start_and_wait(
        &self,
        service: &str,
        exe: &str,
        args: &[&str],
        cwd: &Path,
        timeout: Duration,
        ready: Option<u16>,
    ) -> Result<ProcessRecord, ProcError> {
        let rec = self.spawn(service, exe, args, cwd)?;
        let deadline = Instant::now() + timeout;

        loop {
            if !pid_alive(rec.pid) {
                let _ = fs::remove_file(self.record_path(service));
                return Err(ProcError::DiedEarly {
                    service: service.to_string(),
                    detail: format!(
                        "process {} exited; see {}",
                        rec.pid,
                        rec.stderr_log
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "the service log".to_string())
                    ),
                });
            }

            match ready {
                Some(port) => {
                    if crate::ports::probe(port).is_free() {
                        // Port free means nothing is listening yet.
                    } else {
                        return Ok(rec);
                    }
                }
                None => {
                    // No port to watch: a brief settle is enough to catch a
                    // process that exits instantly.
                    std::thread::sleep(Duration::from_millis(200));
                    if pid_alive(rec.pid) {
                        return Ok(rec);
                    }
                }
            }

            if Instant::now() >= deadline {
                let _ = terminate(rec.pid);
                let _ = fs::remove_file(self.record_path(service));
                return Err(ProcError::NotReady {
                    service: service.to_string(),
                    timeout_ms: timeout.as_millis() as u64,
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Spawn a service with stdout/stderr captured to `data/logs/<service>.log`.
pub fn spawn_captured(
    sup: &Supervisor,
    service: &str,
    exe: &str,
    args: &[&str],
    cwd: &Path,
) -> Result<ProcessRecord, ProcError> {
    fs::create_dir_all(sup.logs_dir()).map_err(|e| ProcError::Record {
        path: sup.logs_dir().display().to_string(),
        detail: e.to_string(),
    })?;

    let out_path = sup.logs_dir().join(format!("{service}.out.log"));
    let err_path = sup.logs_dir().join(format!("{service}.err.log"));

    let out = fs::File::create(&out_path).map_err(|e| ProcError::Record {
        path: out_path.display().to_string(),
        detail: e.to_string(),
    })?;
    let err = fs::File::create(&err_path).map_err(|e| ProcError::Record {
        path: err_path.display().to_string(),
        detail: e.to_string(),
    })?;

    let child = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| ProcError::Spawn {
            exe: exe.to_string(),
            detail: e.to_string(),
        })?;

    let rec = ProcessRecord {
        service: service.to_string(),
        pid: child.id(),
        exe: resolved_exe(exe),
        started_at: now_secs(),
        cmdline_sha256: cmdline_hash(exe, args),
        stdout_log: Some(out_path),
        stderr_log: Some(err_path),
    };
    sup.save(&rec)?;
    std::mem::forget(child);
    Ok(rec)
}

/// Whether a recorded process is still alive *and* still the one we started.
fn is_ours(rec: &ProcessRecord) -> bool {
    if !pid_alive(rec.pid) {
        return false;
    }
    match live_exe(rec.pid) {
        Some(live) => same_exe(&live, &rec.exe),
        // If we cannot read the live executable (permissions, race), be
        // conservative: treat it as not ours so we never signal it.
        None => false,
    }
}

fn same_exe(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.replace('\\', "/")
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_ascii_lowercase()
    };
    let (a, b) = (norm(a), norm(b));
    if a == b {
        return true;
    }
    // Compare file names too: one side may be resolved to an absolute path.
    let name = |s: &str| s.rsplit('/').next().unwrap_or(s).to_string();
    name(&a) == name(&b)
}

fn resolved_exe(exe: &str) -> String {
    let p = Path::new(exe);
    if p.is_absolute() {
        return exe.replace('\\', "/");
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p).display().to_string().replace('\\', "/"))
        .unwrap_or_else(|_| exe.to_string())
}

/// A stable fingerprint of the command we launched.
pub fn cmdline_hash(exe: &str, args: &[&str]) -> String {
    let mut h = Sha256::new();
    h.update(exe.as_bytes());
    for a in args {
        h.update([0u8]);
        h.update(a.as_bytes());
    }
    let digest = h.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn wait_gone(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A PID that is very unlikely to be in use, for testing stale-record handling.
pub fn find_dead_pid() -> u32 {
    // Scan the high end of the range for a PID that is not alive.
    (40000u32..65000).find(|p| !pid_alive(*p)).unwrap_or(65000)
}

// ---------------------------------------------------------------------------
// Platform specifics
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(h: isize) -> i32;
        fn GetExitCodeProcess(h: isize, code: *mut u32) -> i32;
        fn TerminateProcess(h: isize, code: u32) -> i32;
        fn QueryFullProcessImageNameW(h: isize, flags: u32, buf: *mut u16, size: *mut u32) -> i32;
        fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_TERMINATE: u32 = 0x0001;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const STILL_ACTIVE: u32 = 259;
    /// Bound the reap wait so a wedged process cannot hang the CLI.
    const REAP_TIMEOUT_MS: u32 = 2000;

    /// Whether `pid` is alive and still running.
    pub fn alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // SAFETY: OpenProcess/CloseHandle are balanced; the handle is only used
        // for a read of the exit code.
        unsafe {
            let h = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h == 0 {
                return false;
            }
            // A live-but-signalled process still has a handle; combine with the
            // wait check so we do not report zombie children as alive.
            let waited = WaitForSingleObject(h, 0);
            let mut code: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            // WAIT_TIMEOUT (258) means still running.
            waited == 258 && ok != 0 && code == STILL_ACTIVE
        }
    }

    /// The executable path of a live process.
    pub fn exe_of(pid: u32) -> Option<String> {
        // SAFETY: buffer is sized before the call; length is written back.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h == 0 {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            buf.truncate(size as usize);
            Some(OsString::from_wide(&buf).to_string_lossy().to_string())
        }
    }

    /// Ask a process to terminate.
    pub fn kill(pid: u32) -> std::io::Result<()> {
        // SAFETY: handle is closed on every path.
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let ok = TerminateProcess(h, 1);
            CloseHandle(h);
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    /// Block briefly so a killed process is reaped before we return.
    pub fn reap(pid: u32) {
        // SAFETY: as above.
        unsafe {
            let h = OpenProcess(SYNCHRONIZE, 0, pid);
            if h != 0 {
                WaitForSingleObject(h, REAP_TIMEOUT_MS);
                CloseHandle(h);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// Whether `pid` is alive.
    pub fn alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        Path::new(&format!("/proc/{pid}")).exists()
            || std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }

    /// The executable path of a live process.
    pub fn exe_of(pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.display().to_string())
    }

    /// Ask a process to terminate.
    pub fn kill(pid: u32) -> std::io::Result<()> {
        let status = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("kill failed"))
        }
    }

    /// Block briefly so a killed process is reaped before we return.
    pub fn reap(pid: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if !alive(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// Whether `pid` refers to a live process.
pub fn pid_alive(pid: u32) -> bool {
    imp::alive(pid)
}

/// The executable path of a live process, if readable.
pub fn live_exe(pid: u32) -> Option<String> {
    imp::exe_of(pid)
}

fn terminate(pid: u32) -> std::io::Result<()> {
    imp::kill(pid)?;
    imp::reap(pid);
    Ok(())
}

/// Keep the unused-import warning away on non-Windows builds.
#[allow(dead_code)]
fn _assert_child_used(_c: Child) {}
