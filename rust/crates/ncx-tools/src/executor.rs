//! Sandboxed command execution — Rust port of `nanocodex/sandbox/executor.py`.
//!
//! `PolicyExecutor` runs a command in a subprocess with a timeout. On Windows
//! the process tree is contained in a Win32 Job Object (kill-on-job-close +
//! active-process cap) for real OS-level PROCESS/RESOURCE containment — the
//! whole descendant tree dies together on timeout/exit. This is NOT
//! filesystem/network isolation; that stays gated at the policy+approval layer
//! (see the Python module docstring). Job-API failure degrades to an
//! un-contained run rather than failing the command.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::text_encoding::decode_text_lossy;

const MAX_OUTPUT: usize = 16_000;

/// Outcome of a single sandboxed command.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub sandbox_denied: bool,
    pub denial_reason: String,
}

impl ExecResult {
    pub fn ok(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && !self.sandbox_denied
    }

    /// Render the complete, untruncated command output (`stdout`, `stderr`,
    /// timeout notice, and `Exit code: N`).
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

/// Run commands under policy-level enforcement plus (on Windows) Job-Object
/// process containment.
#[derive(Debug, Clone)]
pub struct PolicyExecutor {
    /// Generous fork-bomb backstop on Windows; 0 disables the active-process cap.
    pub active_process_limit: u32,
}

impl Default for PolicyExecutor {
    fn default() -> Self {
        // 512 mirrors WindowsJobExecutor.ACTIVE_PROCESS_LIMIT.
        PolicyExecutor {
            active_process_limit: 512,
        }
    }
}

impl PolicyExecutor {
    pub fn new() -> Self {
        Self::default()
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
        let mut cmd = command_with_env(command, cwd, extra_env);
        cmd.current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return ExecResult {
                    exit_code: 1,
                    stderr: format!("spawn failed: {e}"),
                    ..Default::default()
                }
            }
        };

        #[cfg(windows)]
        let _job = child
            .id()
            .and_then(|pid| win_job::Job::contain(pid, self.active_process_limit));

        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
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
            Ok(status) => {
                let code = status.ok().and_then(|s| s.code()).unwrap_or(1);
                ExecResult {
                    exit_code: code,
                    stdout: decode_text_lossy(&out),
                    stderr: decode_text_lossy(&err),
                    ..Default::default()
                }
            }
            Err(_) => {
                // Timed out: kill the tree.
                #[cfg(windows)]
                if let Some(j) = _job.as_ref() {
                    j.terminate();
                }
                let _ = child.start_kill();
                let _ = child.wait().await;
                ExecResult {
                    exit_code: 124,
                    timed_out: true,
                    stdout: decode_text_lossy(&out),
                    stderr: decode_text_lossy(&err),
                    ..Default::default()
                }
            }
        }
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
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    pub struct Job {
        job: HANDLE,
        proc: HANDLE,
    }

    // The handles are owned by this struct and only touched on the executor task.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// Create a job with kill-on-close (+ optional active-process cap) and
        /// assign `pid` to it. Returns None on any API failure (degrade to
        /// un-contained), mirroring the Python OSError fallback.
        pub fn contain(pid: u32, active_process_limit: u32) -> Option<Job> {
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if active_process_limit > 0 {
                    flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                    info.BasicLimitInformation.ActiveProcessLimit = active_process_limit;
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
                let proc = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if proc.is_null() {
                    CloseHandle(job);
                    return None;
                }
                if AssignProcessToJobObject(job, proc) == 0 {
                    CloseHandle(proc);
                    CloseHandle(job);
                    return None;
                }
                Some(Job { job, proc })
            }
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
        mut env: HashMap<String, String>,
    ) -> ExecResult {
        env.insert("NCX_TEST_PIPE_MODE".into(), mode.into());
        let exe = std::env::current_exe().unwrap();
        let prefix = if cfg!(windows) { ".\\" } else { "./" };
        let command = format!(
            "{prefix}{} --exact executor::tests::pipe_output_child --nocapture",
            exe.file_name().unwrap().to_str().unwrap()
        );
        PolicyExecutor::new()
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
}
