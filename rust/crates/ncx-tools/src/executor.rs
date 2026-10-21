//! Sandboxed command execution — Rust port of `nanocodex/sandbox/executor.py`.
//!
//! `PolicyExecutor` runs a command in a subprocess with a timeout. On Windows
//! the process tree is contained in a Win32 Job Object (kill-on-job-close,
//! active-process cap, job-wide committed-memory cap, UI restrictions) — the
//! whole descendant tree dies together on timeout/exit. Job-API failure
//! degrades to an un-contained run rather than failing the command.
//!
//! When built with [`PolicyExecutor::for_policy`] outside `danger-full-access`,
//! Windows commands additionally run at Low integrity so the OS itself denies
//! writes outside the writable roots (see `win_sandbox`). Network access and
//! reads are not isolated, and other platforms have no OS-level write
//! isolation yet: there the policy+approval layer is the only gate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use ncx_sandbox::{SandboxPolicy, DANGER_FULL_ACCESS, WORKSPACE_WRITE};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::process::Command;
use tokio::time::timeout;

use crate::text_encoding::decode_text_lossy;

const MAX_OUTPUT: usize = 16_000;

/// Default job-wide committed-memory cap (4 GiB).
pub const DEFAULT_JOB_MEMORY_LIMIT: u64 = 4 << 30;
/// Overrides [`DEFAULT_JOB_MEMORY_LIMIT`] in MiB; `0` disables the cap.
pub const JOB_MEMORY_LIMIT_ENV: &str = "NCX_SANDBOX_JOB_MEMORY_MB";

/// Outcome of a single sandboxed command.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub sandbox_denied: bool,
    pub denial_reason: String,
    /// Set when OS-level isolation was requested but could not be applied, so
    /// the command ran without it.
    pub sandbox_note: String,
}

impl ExecResult {
    pub fn ok(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && !self.sandbox_denied
    }

    /// Render the complete, untruncated command output (`stdout`, `stderr`,
    /// timeout notice, sandbox note, and `Exit code: N`).
    pub fn render_untruncated(&self) -> String {
        if self.sandbox_denied {
            return format!("Sandbox denied: {}", self.denial_reason);
        }
        let mut parts: Vec<String> = Vec::new();
        if !self.stdout.is_empty() {
            parts.push(self.stdout.clone());
        }
        if !self.stderr.trim().is_empty() {
            parts.push(format!("STDERR:\n{}", self.stderr));
        }
        if self.timed_out {
            parts.push("(command timed out)".to_string());
        }
        if !self.sandbox_note.is_empty() {
            parts.push(format!("[sandbox] {}", self.sandbox_note));
        }
        parts.push(format!("\nExit code: {}", self.exit_code));
        parts.join("\n")
    }

    /// Render for the model — mirrors the Python `ExecResult.render`.
    pub fn render(&self) -> String {
        let out = self.render_untruncated();
        if out.chars().count() > MAX_OUTPUT {
            let chars: Vec<char> = out.chars().collect();
            let half = MAX_OUTPUT / 2;
            let truncated = chars.len() - MAX_OUTPUT;
            let head: String = chars[..half].iter().collect();
            let tail: String = chars[chars.len() - half..].iter().collect();
            return format!("{head}\n\n... ({truncated} chars truncated) ...\n\n{tail}");
        }
        out
    }
}

/// OS-enforced write confinement: only `writable_roots` (and a per-run temp
/// directory that `TEMP`/`TMP` point at) are writable. An empty list confines
/// writes to the temp directory alone (`read-only` mode).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteIsolation {
    pub writable_roots: Vec<PathBuf>,
}

/// Run commands under policy-level enforcement plus (on Windows) Job-Object
/// process containment and, when configured, Low-integrity write isolation.
#[derive(Debug, Clone)]
pub struct PolicyExecutor {
    /// Generous fork-bomb backstop on Windows; 0 disables the active-process cap.
    pub active_process_limit: u32,
    /// Job-wide committed-memory cap in bytes on Windows; 0 disables it.
    pub job_memory_limit: u64,
    /// `None` runs unconfined (the policy/approval layer still applies).
    /// Ignored on non-Windows platforms.
    pub write_isolation: Option<WriteIsolation>,
}

impl Default for PolicyExecutor {
    fn default() -> Self {
        // 512 mirrors WindowsJobExecutor.ACTIVE_PROCESS_LIMIT.
        PolicyExecutor {
            active_process_limit: 512,
            job_memory_limit: job_memory_limit_from_env(),
            write_isolation: None,
        }
    }
}

fn job_memory_limit_from_env() -> u64 {
    std::env::var(JOB_MEMORY_LIMIT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|mib| mib.saturating_mul(1 << 20))
        .unwrap_or(DEFAULT_JOB_MEMORY_LIMIT)
}

impl PolicyExecutor {
    /// Unconfined executor: Job-Object containment only. Used for user hooks,
    /// fixed internal commands and user-approved escalations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Executor that OS-enforces `policy`'s write boundary where supported:
    /// `workspace-write` → workspace + writable roots, `read-only` → nothing
    /// but the per-run temp dir, `danger-full-access` → unconfined.
    pub fn for_policy(policy: &SandboxPolicy) -> Self {
        let write_isolation = if policy.mode == DANGER_FULL_ACCESS {
            None
        } else if policy.mode == WORKSPACE_WRITE {
            let mut writable_roots = vec![policy.workspace.clone()];
            writable_roots.extend(policy.writable_roots.iter().cloned());
            Some(WriteIsolation { writable_roots })
        } else {
            Some(WriteIsolation::default())
        };
        PolicyExecutor {
            write_isolation,
            ..Self::default()
        }
    }

    #[cfg(windows)]
    pub(crate) fn job_limits(&self) -> win_job::JobLimits {
        win_job::JobLimits {
            active_process_limit: self.active_process_limit,
            job_memory_limit: self.job_memory_limit,
        }
    }

    /// Run `command` in `cwd` with a wall-clock `timeout_s`.
    pub async fn run(&self, command: &str, cwd: &Path, timeout_s: u64) -> ExecResult {
        self.run_with_env(command, cwd, timeout_s, &HashMap::new())
            .await
    }

    /// Run `command` with additional environment variables layered on top of
    /// the minimal sandbox environment.
    pub async fn run_with_env(
        &self,
        command: &str,
        cwd: &Path,
        timeout_s: u64,
        extra_env: &HashMap<String, String>,
    ) -> ExecResult {
        let prepared = self.prepare_isolation().await;
        let spawned = match self.spawn_prepared(prepared, command, cwd, extra_env, false) {
            Ok(spawned) => spawned,
            Err(message) => {
                return ExecResult {
                    exit_code: 1,
                    stderr: message,
                    ..Default::default()
                }
            }
        };
        let SpawnedProcess {
            mut child,
            stdout: mut stdout_pipe,
            stderr: mut stderr_pipe,
            job: _job,
            sandbox_note,
            ..
        } = spawned;
        // Keep buffers outside the timeout future so cancellation retains the
        // output already read. Drain both pipes together: either pipe can fill
        // before the child closes the other one.
        let mut out = Vec::new();
        let mut err = Vec::new();
        let collect = async {
            let stdout = async {
                if let Some(p) = stdout_pipe.as_mut() {
                    let _ = p.read_to_end(&mut out).await;
                }
            };
            let stderr = async {
                if let Some(p) = stderr_pipe.as_mut() {
                    let _ = p.read_to_end(&mut err).await;
                }
            };
            let (_, _, status) = tokio::join!(stdout, stderr, child.wait());
            status
        };

        match timeout(Duration::from_secs(timeout_s), collect).await {
            Ok(status) => ExecResult {
                exit_code: status.unwrap_or(1),
                stdout: decode_text_lossy(&out),
                stderr: decode_text_lossy(&err),
                sandbox_note,
                ..Default::default()
            },
            Err(_) => {
                // Timed out: kill the tree.
                #[cfg(windows)]
                if let Some(j) = _job.as_ref() {
                    j.terminate();
                }
                child.start_kill();
                let _ = child.wait().await;
                // Output the child wrote just before the kill can still sit
                // unread in the pipes. Drain it, bounded in case an escaped
                // descendant keeps a pipe open.
                tokio::join!(
                    drain_remaining(stdout_pipe.as_mut(), &mut out),
                    drain_remaining(stderr_pipe.as_mut(), &mut err),
                );
                ExecResult {
                    exit_code: 124,
                    timed_out: true,
                    stdout: decode_text_lossy(&out),
                    stderr: decode_text_lossy(&err),
                    sandbox_note,
                    ..Default::default()
                }
            }
        }
    }

    /// Label the writable roots off the async thread: the first labeling of a
    /// large tree propagates to every existing file and can take a while.
    async fn prepare_isolation(&self) -> Option<Prepared> {
        #[cfg(windows)]
        {
            let roots = self.write_isolation.clone()?.writable_roots;
            let prepared =
                tokio::task::spawn_blocking(move || crate::win_sandbox::prepare(&roots)).await;
            Some(match prepared {
                Ok(result) => result,
                Err(error) => Err(format!("sandbox preparation failed: {error}")),
            })
        }
        #[cfg(not(windows))]
        None
    }

    /// Spawn `command` the same way for one-shot and managed execution:
    /// confined when isolation is configured and available, otherwise
    /// unconfined with an explanatory `sandbox_note`.
    pub(crate) fn spawn_process(
        &self,
        command: &str,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
        pipe_stdin: bool,
    ) -> Result<SpawnedProcess, String> {
        #[cfg(windows)]
        let prepared = self
            .write_isolation
            .as_ref()
            .map(|isolation| crate::win_sandbox::prepare(&isolation.writable_roots));
        #[cfg(not(windows))]
        let prepared = None;
        self.spawn_prepared(prepared, command, cwd, extra_env, pipe_stdin)
    }

    fn spawn_prepared(
        &self,
        prepared: Option<Prepared>,
        command: &str,
        cwd: &Path,
        extra_env: &HashMap<String, String>,
        pipe_stdin: bool,
    ) -> Result<SpawnedProcess, String> {
        #[allow(unused_mut)]
        let mut sandbox_note = String::new();
        #[cfg(windows)]
        if let Some(prepared) = prepared {
            let confined = prepared.and_then(|temp| {
                let mut env = build_env();
                let temp_dir = temp.path().to_string_lossy().into_owned();
                env.insert("TEMP".into(), temp_dir.clone());
                env.insert("TMP".into(), temp_dir);
                env.extend(extra_env.iter().map(|(k, v)| (k.clone(), v.clone())));
                let (application, command_line) = windows_command_line(command);
                crate::win_sandbox::spawn_low_integrity(
                    &application,
                    &command_line,
                    cwd,
                    &env,
                    pipe_stdin,
                    &self.job_limits(),
                    temp,
                )
            });
            match confined {
                Ok(spawn) => {
                    return Ok(SpawnedProcess {
                        child: RunningChild::LowIntegrity(spawn.child),
                        stdin: spawn.stdin.map(|pipe| Box::new(pipe) as BoxedWriter),
                        stdout: Some(Box::new(spawn.stdout)),
                        stderr: Some(Box::new(spawn.stderr)),
                        job: spawn.job,
                        sandbox_note,
                    })
                }
                Err(reason) => {
                    sandbox_note = format!(
                        "OS write isolation unavailable ({reason}); the command ran without it."
                    );
                }
            }
        }
        #[cfg(not(windows))]
        let _ = prepared;

        let mut cmd = command_with_env(command, cwd, extra_env);
        cmd.current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if pipe_stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        #[cfg(windows)]
        let job = child
            .id()
            .and_then(|pid| win_job::Job::contain(pid, &self.job_limits()));
        #[cfg(not(windows))]
        let job = None;
        let stdin = child.stdin.take().map(|pipe| Box::new(pipe) as BoxedWriter);
        let stdout = child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as BoxedReader);
        let stderr = child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as BoxedReader);
        Ok(SpawnedProcess {
            child: RunningChild::Tokio(child),
            stdin,
            stdout,
            stderr,
            job,
            sandbox_note,
        })
    }
}

#[cfg(windows)]
type Prepared = Result<crate::win_sandbox::RunTempDir, String>;
#[cfg(not(windows))]
type Prepared = std::convert::Infallible;

#[cfg(windows)]
pub(crate) type JobHandle = win_job::Job;
#[cfg(not(windows))]
pub(crate) type JobHandle = std::convert::Infallible;

pub(crate) type BoxedReader = Box<dyn AsyncRead + Unpin + Send>;
pub(crate) type BoxedWriter = Box<dyn AsyncWrite + Unpin + Send>;

/// A started command with its stdio, Job Object and isolation outcome.
pub(crate) struct SpawnedProcess {
    pub child: RunningChild,
    pub stdin: Option<BoxedWriter>,
    pub stdout: Option<BoxedReader>,
    pub stderr: Option<BoxedReader>,
    pub job: Option<JobHandle>,
    pub sandbox_note: String,
}

/// Either a regular tokio child or a Low-integrity child spawned directly.
pub(crate) enum RunningChild {
    Tokio(tokio::process::Child),
    #[cfg(windows)]
    LowIntegrity(crate::win_sandbox::LowIlChild),
}

impl RunningChild {
    pub(crate) async fn wait(&mut self) -> Option<i32> {
        match self {
            RunningChild::Tokio(child) => child.wait().await.ok().and_then(|s| s.code()),
            #[cfg(windows)]
            RunningChild::LowIntegrity(child) => child.wait().await.ok(),
        }
    }

    /// `Ok(None)` while running; the exit code (1 for signal deaths) after.
    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        match self {
            RunningChild::Tokio(child) => Ok(child.try_wait()?.map(|s| s.code().unwrap_or(1))),
            #[cfg(windows)]
            RunningChild::LowIntegrity(child) => child.try_wait(),
        }
    }

    pub(crate) fn start_kill(&mut self) {
        match self {
            RunningChild::Tokio(child) => {
                let _ = child.start_kill();
            }
            #[cfg(windows)]
            RunningChild::LowIntegrity(child) => child.kill(),
        }
    }
}

/// Upper bound for reading what is left in a pipe after a timeout kill.
const POST_KILL_DRAIN: Duration = Duration::from_millis(500);

/// Append whatever is still buffered in `pipe` to `buf`, giving up after
/// `POST_KILL_DRAIN` if a writer keeps the pipe open.
async fn drain_remaining<R: tokio::io::AsyncRead + Unpin>(pipe: Option<&mut R>, buf: &mut Vec<u8>) {
    if let Some(pipe) = pipe {
        let _ = timeout(POST_KILL_DRAIN, pipe.read_to_end(buf)).await;
    }
}

/// Build the same sandboxed command shape for one-shot and managed execution.
pub(crate) fn command_with_env(
    command: &str,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
) -> Command {
    let mut env = build_env();
    env.extend(
        extra_env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let mut command = base_command(command);
    command.current_dir(cwd).env_clear();
    for (key, value) in env {
        command.env(key, value);
    }
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// `cmd.exe` path and full command line for spawning `command` directly
/// (the Low-integrity path), quoted the same way as [`base_command`].
#[cfg(windows)]
fn windows_command_line(command: &str) -> (String, String) {
    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| {
        let sysroot = std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{sysroot}\System32\cmd.exe")
    });
    let line = format!("\"{comspec}\" /D /S /C \"chcp 65001>nul & {command}\"");
    (comspec, line)
}

/// Build the base subprocess command for this platform.
fn base_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut c = Command::new(comspec);
        c.arg("/D")
            .arg("/S")
            .arg("/C")
            .raw_arg(format!("\"chcp 65001>nul & {command}\""));
        c
    }
    #[cfg(not(windows))]
    {
        let bash = which_bash();
        let mut c = Command::new(bash);
        c.arg("-l").arg("-c").arg(command);
        c
    }
}

#[cfg(not(windows))]
fn which_bash() -> String {
    for p in ["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"] {
        if Path::new(p).exists() {
            return p.to_string();
        }
    }
    "bash".to_string()
}

/// Minimal environment for the child — mirrors `_build_env`.
pub(crate) fn build_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    let get = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    #[cfg(windows)]
    {
        let sysroot = get("SYSTEMROOT", r"C:\Windows");
        env.insert("SYSTEMROOT".into(), sysroot.clone());
        env.insert(
            "COMSPEC".into(),
            get("COMSPEC", &format!("{sysroot}\\system32\\cmd.exe")),
        );
        env.insert(
            "PATH".into(),
            get("PATH", &format!("{sysroot}\\system32;{sysroot}")),
        );
        env.insert("PATHEXT".into(), get("PATHEXT", ".COM;.EXE;.BAT;.CMD"));
        env.insert("USERPROFILE".into(), get("USERPROFILE", ""));
        env.insert("TEMP".into(), get("TEMP", &format!("{sysroot}\\Temp")));
        env.insert("TMP".into(), get("TMP", &format!("{sysroot}\\Temp")));
        env.insert("PYTHONUNBUFFERED".into(), "1".into());
        env.insert("PYTHONIOENCODING".into(), "utf-8".into());
        env.insert("PYTHONUTF8".into(), "1".into());
        env.insert("LANG".into(), "C.UTF-8".into());
    }
    #[cfg(not(windows))]
    {
        env.insert("HOME".into(), get("HOME", "/tmp"));
        env.insert("PATH".into(), get("PATH", "/usr/bin:/bin"));
        env.insert("LANG".into(), get("LANG", "C.UTF-8"));
        env.insert("TERM".into(), get("TERM", "dumb"));
        env.insert("PYTHONUNBUFFERED".into(), "1".into());
    }
    env
}

/// Win32 Job Object containment — mirrors `_WindowsJob` / `WindowsJobExecutor`.
#[cfg(windows)]
pub(crate) mod win_job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP,
        JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
        JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_READCLIPBOARD,
        JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Commands get no clipboard, desktop switching, global atoms, or
    /// system-wide settings/shutdown. USER handle isolation is left off so
    /// ordinary console tools keep working.
    const UI_RESTRICTIONS: u32 = JOB_OBJECT_UILIMIT_READCLIPBOARD
        | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
        | JOB_OBJECT_UILIMIT_DESKTOP
        | JOB_OBJECT_UILIMIT_GLOBALATOMS
        | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
        | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
        | JOB_OBJECT_UILIMIT_EXITWINDOWS;

    /// Resource limits applied to every job; 0 disables a limit.
    #[derive(Debug, Clone, Copy)]
    pub struct JobLimits {
        pub active_process_limit: u32,
        pub job_memory_limit: u64,
    }

    pub struct Job {
        job: HANDLE,
        proc: HANDLE,
    }

    // The handles are owned by this struct and only touched on the executor task.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Create a job with kill-on-close, the configured limits and UI
        /// restrictions. Returns None on any API failure for the limits
        /// (degrade to un-contained), mirroring the Python OSError fallback;
        /// a UI-restriction failure keeps the job.
        pub fn create(limits: &JobLimits) -> Option<Job> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if limits.active_process_limit > 0 {
                    flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                    info.BasicLimitInformation.ActiveProcessLimit = limits.active_process_limit;
                }
                if limits.job_memory_limit > 0 {
                    flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                    info.JobMemoryLimit =
                        usize::try_from(limits.job_memory_limit).unwrap_or(usize::MAX);
                }
                info.BasicLimitInformation.LimitFlags = flags;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(job);
                    return None;
                }
                let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                    UIRestrictionsClass: UI_RESTRICTIONS,
                };
                SetInformationJobObject(
                    job,
                    JobObjectBasicUIRestrictions,
                    &ui as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
                );
                Some(Job {
                    job,
                    proc: std::ptr::null_mut(),
                })
            }
        }

        /// Create a job and assign the already-running `pid` to it.
        pub fn contain(pid: u32, limits: &JobLimits) -> Option<Job> {
            let mut job = Job::create(limits)?;
            unsafe {
                let proc = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if proc.is_null() {
                    return None;
                }
                job.proc = proc;
                if !job.assign(proc) {
                    return None;
                }
            }
            Some(job)
        }

        /// Assign a process handle (with `PROCESS_SET_QUOTA |
        /// PROCESS_TERMINATE`) to this job.
        pub fn assign(&self, process: HANDLE) -> bool {
            unsafe { AssignProcessToJobObject(self.job, process) != 0 }
        }

        /// Kill every process in the job at once.
        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the last (kill-on-close) job handle reaps any survivors.
            unsafe {
                if !self.proc.is_null() {
                    CloseHandle(self.proc);
                }
                if !self.job.is_null() {
                    CloseHandle(self.job);
                }
            }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Use the test binary itself to exercise real pipe backpressure, without
    // depending on an installed interpreter or platform-specific shell loops.
    #[test]
    fn pipe_output_child() {
        use std::io::Write;
        let Ok(mode) = std::env::var("NCX_TEST_PIPE_MODE") else {
            return;
        };
        std::io::stdout().write_all(b"stdout-start\n").unwrap();
        std::io::stdout().flush().unwrap();
        std::io::stderr().write_all(b"stderr-start\n").unwrap();
        std::io::stderr().flush().unwrap();
        if mode == "timeout" {
            // Give the parent ample time to drain the flushed markers, then
            // record that this point was reached before blocking until killed.
            std::thread::sleep(Duration::from_secs(1));
            if let Ok(ready) = std::env::var("NCX_TEST_PIPE_READY") {
                std::fs::write(ready, b"ready").unwrap();
            }
            std::thread::sleep(Duration::from_secs(120));
        } else if mode == "write" {
            // `<temp>` resolves to the child's own TEMP directory.
            let target = std::env::var("NCX_TEST_WRITE_TARGET").unwrap();
            let target = if target == "<temp>" {
                std::env::temp_dir().join("ncx-probe.txt")
            } else {
                PathBuf::from(target)
            };
            match std::fs::write(&target, b"probe") {
                Ok(()) => println!("write-ok {}", target.display()),
                Err(error) => println!("write-denied {}: {error}", target.display()),
            }
        } else if mode == "alloc" {
            let mib: usize = std::env::var("NCX_TEST_ALLOC_MB").unwrap().parse().unwrap();
            let mut block: Vec<u8> = Vec::new();
            match block.try_reserve_exact(mib << 20) {
                Ok(()) => println!("alloc-ok"),
                Err(_) => println!("alloc-denied"),
            }
        } else {
            std::io::stderr().write_all(&vec![b'e'; 1_000_000]).unwrap();
            std::io::stdout().write_all(&vec![b'o'; 1_000_000]).unwrap();
        }
    }

    async fn run_pipe_child(mode: &str, timeout_s: u64) -> ExecResult {
        run_pipe_child_with_env(mode, timeout_s, HashMap::new()).await
    }

    async fn run_pipe_child_with_env(
        mode: &str,
        timeout_s: u64,
        env: HashMap<String, String>,
    ) -> ExecResult {
        run_pipe_child_on(&PolicyExecutor::new(), mode, timeout_s, env).await
    }

    async fn run_pipe_child_on(
        executor: &PolicyExecutor,
        mode: &str,
        timeout_s: u64,
        mut env: HashMap<String, String>,
    ) -> ExecResult {
        env.insert("NCX_TEST_PIPE_MODE".into(), mode.into());
        let exe = std::env::current_exe().unwrap();
        let prefix = if cfg!(windows) { ".\\" } else { "./" };
        let command = format!(
            "{prefix}{} --exact executor::tests::pipe_output_child --nocapture",
            exe.file_name().unwrap().to_str().unwrap()
        );
        executor
            .run_with_env(&command, exe.parent().unwrap(), timeout_s, &env)
            .await
    }

    #[tokio::test]
    async fn drains_stdout_and_stderr_concurrently() {
        let result = run_pipe_child("large", 10).await;
        assert!(result.ok(), "{}", result.render());
        assert!(result.stdout.contains(&"o".repeat(1_000_000)));
        assert!(result.stderr.contains(&"e".repeat(1_000_000)));
    }

    #[tokio::test]
    async fn post_kill_drain_recovers_unread_output() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        writer.write_all(b"last-words\n").await.unwrap();
        drop(writer);
        let mut buf = b"earlier\n".to_vec();
        drain_remaining(Some(&mut reader), &mut buf).await;
        assert_eq!(buf, b"earlier\nlast-words\n");
    }

    #[tokio::test]
    async fn post_kill_drain_is_bounded_when_a_writer_stays_open() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        writer.write_all(b"partial").await.unwrap();
        let started = std::time::Instant::now();
        let mut buf = Vec::new();
        drain_remaining(Some(&mut reader), &mut buf).await;
        assert!(started.elapsed() < POST_KILL_DRAIN * 4);
        assert_eq!(buf, b"partial");
        drop(writer);
    }

    #[tokio::test]
    async fn timeout_preserves_already_collected_output() {
        // The child writes a ready file once its markers have been flushed
        // and had time to drain. A timeout before that point only means the
        // child started slowly (e.g. under parallel test load), so retry with
        // a longer timeout instead of asserting on an inconclusive run.
        let ready =
            std::env::temp_dir().join(format!("ncx-executor-timeout-ready-{}", std::process::id()));
        let mut conclusive = None;
        for timeout_s in [3, 10, 30, 60] {
            let _ = std::fs::remove_file(&ready);
            let env = HashMap::from([(
                "NCX_TEST_PIPE_READY".to_string(),
                ready.to_string_lossy().into_owned(),
            )]);
            let result = run_pipe_child_with_env("timeout", timeout_s, env).await;
            if ready.exists() {
                conclusive = Some(result);
                break;
            }
            assert!(result.timed_out, "{}", result.render());
        }
        let _ = std::fs::remove_file(&ready);
        let result = conclusive.expect("child never reached its ready point");
        assert!(result.timed_out);
        assert_eq!(result.exit_code, 124);
        assert!(
            result.stdout.contains("stdout-start"),
            "{:?}",
            result.stdout
        );
        assert!(
            result.stderr.contains("stderr-start"),
            "{:?}",
            result.stderr
        );
    }

    #[test]
    fn ok_requires_zero_exit() {
        assert!(ExecResult::default().ok());
        assert!(!ExecResult {
            exit_code: 1,
            ..Default::default()
        }
        .ok());
        assert!(!ExecResult {
            timed_out: true,
            ..Default::default()
        }
        .ok());
    }

    #[test]
    fn render_includes_exit_code() {
        let r = ExecResult {
            exit_code: 0,
            stdout: "hello".into(),
            ..Default::default()
        };
        let out = r.render();
        assert!(out.contains("hello"));
        assert!(out.contains("Exit code: 0"));
    }

    #[test]
    fn render_includes_stderr_and_timeout() {
        let r = ExecResult {
            exit_code: 124,
            stderr: "boom".into(),
            timed_out: true,
            ..Default::default()
        };
        let out = r.render();
        assert!(out.contains("STDERR:\nboom"));
        assert!(out.contains("(command timed out)"));
        assert!(out.contains("Exit code: 124"));
    }

    #[test]
    fn render_sandbox_denied() {
        let r = ExecResult {
            sandbox_denied: true,
            denial_reason: "nope".into(),
            ..Default::default()
        };
        assert_eq!(r.render(), "Sandbox denied: nope");
    }

    #[test]
    fn render_truncates_huge_output() {
        let r = ExecResult {
            stdout: "x".repeat(40_000),
            ..Default::default()
        };
        let out = r.render();
        assert!(out.contains("chars truncated"));
        assert!(out.chars().count() < 40_000);
    }

    #[tokio::test]
    async fn run_echo_returns_stdout() {
        let exec = PolicyExecutor::new();
        let cwd = std::env::temp_dir();
        let result = exec.run("echo ncx_hello", &cwd, 30).await;
        assert!(result.ok(), "render: {}", result.render());
        assert!(
            result.stdout.contains("ncx_hello"),
            "stdout: {:?}",
            result.stdout
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_shell_preserves_chinese_output() {
        let result = PolicyExecutor::new()
            .run("echo 中文路径与工具输出", &std::env::temp_dir(), 30)
            .await;

        assert!(result.ok(), "{}", result.render());
        assert!(
            result.stdout.contains("中文路径与工具输出"),
            "stdout: {:?}",
            result.stdout
        );
        assert!(!result.stdout.contains('\u{FFFD}'), "{:?}", result.stdout);
    }

    #[tokio::test]
    async fn run_nonzero_exit_is_captured() {
        let exec = PolicyExecutor::new();
        let cwd = std::env::temp_dir();
        let result = exec.run("exit 3", &cwd, 30).await;
        assert_eq!(result.exit_code, 3);
        assert!(!result.ok());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_shell_preserves_double_quoted_python_c_arguments() {
        let result = PolicyExecutor::new()
            .run(
                "python -c \"import sys; print('quote_ok', 1 + 2)\"",
                &std::env::temp_dir(),
                30,
            )
            .await;
        assert!(result.ok(), "{}", result.render());
        assert!(result.stdout.contains("quote_ok 3"), "{:?}", result.stdout);
    }

    #[cfg(windows)]
    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ncx-exec-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(windows)]
    fn confined(roots: Vec<PathBuf>) -> PolicyExecutor {
        PolicyExecutor {
            write_isolation: Some(WriteIsolation {
                writable_roots: roots,
            }),
            ..PolicyExecutor::new()
        }
    }

    #[cfg(windows)]
    async fn probe_write(executor: &PolicyExecutor, target: &str) -> ExecResult {
        let env = HashMap::from([("NCX_TEST_WRITE_TARGET".to_string(), target.to_string())]);
        run_pipe_child_on(executor, "write", 30, env).await
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn low_integrity_child_writes_only_inside_writable_roots() {
        let workspace = unique_dir("ws");
        std::fs::create_dir_all(workspace.join(".git").join("hooks")).unwrap();
        let executor = confined(vec![workspace.clone()]);

        let inside = workspace.join("inside.txt");
        let result = probe_write(&executor, inside.to_str().unwrap()).await;
        assert!(result.sandbox_note.is_empty(), "{}", result.render());
        assert!(result.stdout.contains("write-ok"), "{}", result.render());
        assert_eq!(std::fs::read(&inside).unwrap(), b"probe");

        let profile = PathBuf::from(std::env::var("USERPROFILE").unwrap());
        let outside = profile.join(format!("ncx-lowil-probe-{}.txt", std::process::id()));
        let result = probe_write(&executor, outside.to_str().unwrap()).await;
        let leaked = outside.exists();
        let _ = std::fs::remove_file(&outside);
        assert!(result.sandbox_note.is_empty(), "{}", result.render());
        assert!(
            result.stdout.contains("write-denied"),
            "{}",
            result.render()
        );
        assert!(!leaked, "Low IL child wrote outside the workspace");

        // Protected workspace entries keep a Medium label.
        let hook = workspace.join(".git").join("hooks").join("pre-commit");
        let result = probe_write(&executor, hook.to_str().unwrap()).await;
        assert!(
            result.stdout.contains("write-denied"),
            "{}",
            result.render()
        );
        assert!(!hook.exists());

        // TEMP/TMP point at a writable per-run directory that is removed after.
        let result = probe_write(&executor, "<temp>").await;
        assert!(result.stdout.contains("write-ok"), "{}", result.render());
        assert!(result.stdout.contains("ncx-sandbox"), "{}", result.render());
        let temp_file = result
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("write-ok "))
            .map(PathBuf::from)
            .unwrap();
        assert!(!temp_file.parent().unwrap().exists());

        // Labeling is idempotent: a second executor reuses the label.
        let again = probe_write(&confined(vec![workspace.clone()]), inside.to_str().unwrap()).await;
        assert!(again.stdout.contains("write-ok"), "{}", again.render());
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn read_only_isolation_denies_workspace_writes() {
        let workspace = unique_dir("ro");
        let target = workspace.join("blocked.txt");
        let result = probe_write(&confined(Vec::new()), target.to_str().unwrap()).await;
        assert!(result.sandbox_note.is_empty(), "{}", result.render());
        assert!(
            result.stdout.contains("write-denied"),
            "{}",
            result.render()
        );
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn unsafe_writable_root_degrades_with_a_visible_note() {
        let profile = PathBuf::from(std::env::var("USERPROFILE").unwrap());
        let result = confined(vec![profile])
            .run("echo still_ran", &std::env::temp_dir(), 30)
            .await;
        assert!(result.ok(), "{}", result.render());
        assert!(result.stdout.contains("still_ran"));
        assert!(
            result.sandbox_note.contains("user profile"),
            "{:?}",
            result.sandbox_note
        );
        assert!(result
            .render()
            .contains("[sandbox] OS write isolation unavailable"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn confined_shell_preserves_output_exit_code_and_timeout() {
        let workspace = unique_dir("shell");
        let executor = confined(vec![workspace.clone()]);
        let result = executor.run("echo 中文 & exit 7", &workspace, 30).await;
        assert_eq!(result.exit_code, 7, "{}", result.render());
        assert!(result.stdout.contains("中文"), "{:?}", result.stdout);
        let result = run_pipe_child_on(&executor, "large", 30, HashMap::new()).await;
        assert!(result.ok(), "{}", result.render());
        assert!(result.stdout.contains(&"o".repeat(1_000_000)));
        assert!(result.stderr.contains(&"e".repeat(1_000_000)));
        let result = executor
            .run("ping -n 30 127.0.0.1 >nul", &workspace, 1)
            .await;
        assert!(result.timed_out);
        assert_eq!(result.exit_code, 124);
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn job_memory_limit_caps_child_commit() {
        let env = || HashMap::from([("NCX_TEST_ALLOC_MB".to_string(), "1024".to_string())]);
        let capped = PolicyExecutor {
            job_memory_limit: 256 << 20,
            ..PolicyExecutor::new()
        };
        let result = run_pipe_child_on(&capped, "alloc", 30, env()).await;
        assert!(
            result.stdout.contains("alloc-denied"),
            "{}",
            result.render()
        );
        let uncapped = PolicyExecutor {
            job_memory_limit: 0,
            ..PolicyExecutor::new()
        };
        let result = run_pipe_child_on(&uncapped, "alloc", 30, env()).await;
        assert!(result.stdout.contains("alloc-ok"), "{}", result.render());
    }

    #[test]
    fn for_policy_maps_modes_to_write_isolation() {
        use ncx_sandbox::READ_ONLY;
        let ws = std::env::temp_dir();
        assert_eq!(
            PolicyExecutor::for_policy(&SandboxPolicy::new(DANGER_FULL_ACCESS, &ws))
                .write_isolation,
            None
        );
        assert_eq!(
            PolicyExecutor::for_policy(&SandboxPolicy::new(READ_ONLY, &ws)).write_isolation,
            Some(WriteIsolation::default())
        );
        let extra = ws.join("extra-root");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_writable_roots([&extra]);
        let roots = PolicyExecutor::for_policy(&policy)
            .write_isolation
            .unwrap()
            .writable_roots;
        assert_eq!(
            roots,
            vec![policy.workspace.clone(), policy.writable_roots[0].clone()]
        );
    }
}
