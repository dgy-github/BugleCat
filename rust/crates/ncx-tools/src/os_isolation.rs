//! OS-level isolation for shell commands (`os_sandbox = "enforce"`).
//!
//! [`OsIsolation`] turns a [`SandboxPolicy`] into the directories a command
//! may write and read and whether it may use the network; [`spawn_shell`]
//! starts the command either normally or — when isolation is requested — in
//! a Windows AppContainer that the OS holds to exactly those rules. Other
//! platforms fail closed when isolation is requested.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use ncx_sandbox::{SandboxPolicy, DANGER_FULL_ACCESS};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::executor::{command_with_env, spawn_contained, Containment};

/// Extra read-only roots for isolated commands, separated like `PATH`.
pub const READ_ROOTS_ENV: &str = "NCX_OS_SANDBOX_READ_ROOTS";

/// What an isolated command may touch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsIsolation {
    /// Directory trees the command may create, modify, and delete files in.
    pub writable_roots: Vec<PathBuf>,
    /// Directory trees it may only read and execute from (toolchains).
    pub readable_roots: Vec<PathBuf>,
    /// Whether outbound network access is allowed.
    pub network: bool,
}

impl OsIsolation {
    /// Isolation that enforces `policy`, or `None` when the policy is not
    /// OS-enforced or grants full access anyway.
    pub fn from_policy(policy: &SandboxPolicy) -> Option<Self> {
        if !policy.os_enforced || policy.mode == DANGER_FULL_ACCESS {
            return None;
        }
        let mut writable_roots = Vec::new();
        let mut readable_roots = Vec::new();
        if policy.writes_allowed() {
            writable_roots.push(policy.workspace.clone());
            writable_roots.extend(policy.writable_roots.iter().cloned());
            if policy.allow_temp_write {
                writable_roots.push(std::env::temp_dir());
            }
        } else {
            readable_roots.push(policy.workspace.clone());
        }
        let home = home_dir();
        for root in toolchain_roots(std::env::var_os("PATH"), home.as_deref()) {
            if !readable_roots.contains(&root) && !writable_roots.contains(&root) {
                readable_roots.push(root);
            }
        }
        if let Some(extra) = std::env::var_os(READ_ROOTS_ENV) {
            readable_roots.extend(std::env::split_paths(&extra).filter(|p| p.is_dir()));
        }
        Some(OsIsolation {
            writable_roots,
            readable_roots,
            network: policy.network_access,
        })
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Directories an isolated command needs to read to run the user's tools,
/// derived from `PATH`: each entry, or its parent when the entry is a
/// `bin` / `Scripts` / `cmd` folder (so `D:\py\Scripts` grants `D:\py` and its
/// standard library). Locations every container can already read
/// (`%SystemRoot%`, `%ProgramFiles%`) are skipped, and a root is never the
/// user's home, a drive root, or an ancestor of either — those would expose
/// credentials such as `~/.ssh`. A `.cargo` root also brings its sibling
/// `.rustup`, where the real toolchains live.
pub fn toolchain_roots(path_var: Option<std::ffi::OsString>, home: Option<&Path>) -> Vec<PathBuf> {
    let Some(path_var) = path_var else {
        return Vec::new();
    };
    let preinstalled: Vec<PathBuf> = ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|value| normalize(&PathBuf::from(value)))
        .collect();
    let home = home.map(normalize);
    let too_broad = |candidate: &Path| {
        candidate.parent().is_none()
            || home
                .as_deref()
                .is_some_and(|home| home.starts_with(candidate))
    };
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push = |root: PathBuf| {
        if !roots.contains(&root) {
            roots.push(root);
        }
    };
    for entry in std::env::split_paths(&path_var) {
        if entry.as_os_str().is_empty() || !entry.is_dir() {
            continue;
        }
        let entry = normalize(&entry);
        if preinstalled.iter().any(|root| entry.starts_with(root)) {
            continue;
        }
        let leaf = entry
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let mut candidate = entry.clone();
        if matches!(leaf.as_str(), "bin" | "scripts" | "cmd") {
            if let Some(parent) = entry.parent().filter(|parent| !too_broad(parent)) {
                candidate = parent.to_path_buf();
            }
        }
        if too_broad(&candidate) {
            continue;
        }
        if candidate
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(".cargo"))
        {
            if let Some(rustup) = candidate.parent().map(|parent| parent.join(".rustup")) {
                if rustup.is_dir() {
                    push(rustup);
                }
            }
        }
        push(candidate);
    }
    roots
}

/// Lexically normalize (drop `.` components and trailing separators) so
/// `starts_with` comparisons between roots line up.
fn normalize(path: &Path) -> PathBuf {
    ncx_sandbox::normalize(path)
}

pub(crate) type BoxRead = Box<dyn AsyncRead + Unpin + Send>;
pub(crate) type BoxWrite = Box<dyn AsyncWrite + Unpin + Send>;

/// A spawned shell command, native or isolated.
pub(crate) enum ProcessChild {
    Native(tokio::process::Child),
    #[cfg(windows)]
    Isolated(crate::appcontainer::IsolatedChild),
}

impl ProcessChild {
    pub(crate) fn take_stdin(&mut self) -> Option<BoxWrite> {
        match self {
            ProcessChild::Native(child) => child.stdin.take().map(|s| Box::new(s) as BoxWrite),
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child
                .stdin
                .take()
                .map(|f| Box::new(tokio::fs::File::from_std(f)) as BoxWrite),
        }
    }

    pub(crate) fn take_stdout(&mut self) -> Option<BoxRead> {
        match self {
            ProcessChild::Native(child) => child.stdout.take().map(|s| Box::new(s) as BoxRead),
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child
                .stdout
                .take()
                .map(|f| Box::new(tokio::fs::File::from_std(f)) as BoxRead),
        }
    }

    pub(crate) fn take_stderr(&mut self) -> Option<BoxRead> {
        match self {
            ProcessChild::Native(child) => child.stderr.take().map(|s| Box::new(s) as BoxRead),
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child
                .stderr
                .take()
                .map(|f| Box::new(tokio::fs::File::from_std(f)) as BoxRead),
        }
    }

    /// Wait for exit; a process killed without an exit code reports 1.
    pub(crate) async fn wait(&mut self) -> io::Result<i32> {
        match self {
            ProcessChild::Native(child) => child.wait().await.map(|s| s.code().unwrap_or(1)),
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child.wait().await,
        }
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<i32>> {
        match self {
            ProcessChild::Native(child) => child
                .try_wait()
                .map(|status| status.map(|s| s.code().unwrap_or(1))),
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child.try_wait(),
        }
    }

    pub(crate) fn start_kill(&mut self) {
        match self {
            ProcessChild::Native(child) => {
                let _ = child.start_kill();
            }
            #[cfg(windows)]
            ProcessChild::Isolated(child) => child.kill(),
        }
    }
}

/// Start `command` through the platform shell in `cwd`, with piped
/// stdout/stderr (and stdin when `pipe_stdin`), contained so its whole tree
/// can be killed — and, when `isolation` is given, inside the OS sandbox.
pub(crate) fn spawn_shell(
    command: &str,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
    pipe_stdin: bool,
    active_process_limit: u32,
    isolation: Option<&OsIsolation>,
) -> io::Result<(ProcessChild, Containment)> {
    if let Some(isolation) = isolation {
        return spawn_isolated(command, cwd, extra_env, active_process_limit, isolation);
    }
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
    let (child, containment) = spawn_contained(&mut cmd, active_process_limit)?;
    Ok((ProcessChild::Native(child), containment))
}

#[cfg(windows)]
fn spawn_isolated(
    command: &str,
    cwd: &Path,
    extra_env: &HashMap<String, String>,
    active_process_limit: u32,
    isolation: &OsIsolation,
) -> io::Result<(ProcessChild, Containment)> {
    use crate::appcontainer::{self, Access};

    let container = appcontainer::shared()?;
    for root in &isolation.writable_roots {
        ensure_grant(container, root, Access::ReadWrite)?;
    }
    for root in &isolation.readable_roots {
        ensure_grant(container, root, Access::Read)?;
    }
    let mut env: std::collections::BTreeMap<String, String> =
        crate::executor::build_env().into_iter().collect();
    env.extend(extra_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    // The host temp dir is not readable from inside; use the container's own.
    let temp = container.temp_dir().display().to_string();
    env.insert("TEMP".into(), temp.clone());
    env.insert("TMP".into(), temp);
    let comspec = env
        .get("COMSPEC")
        .cloned()
        .unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".to_string());
    let command_line = format!("cmd.exe /D /S /C \"chcp 65001>nul & {command}\"");
    let mut child = appcontainer::spawn(
        container,
        Path::new(&comspec),
        &command_line,
        cwd,
        &env,
        isolation.network,
    )?;
    let containment = Containment::for_job(crate::executor::win_job::Job::contain(
        child.id(),
        active_process_limit,
    ));
    if let Err(error) = child.resume() {
        child.kill();
        return Err(error);
    }
    Ok((ProcessChild::Isolated(child), containment))
}

/// Grant `access` on `root` once per process; the ACL change itself is
/// persistent, and an existing grant is detected without re-propagating it.
#[cfg(windows)]
fn ensure_grant(
    container: &crate::appcontainer::AppContainer,
    root: &Path,
    access: crate::appcontainer::Access,
) -> io::Result<()> {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static DONE: Mutex<Option<HashSet<(PathBuf, bool)>>> = Mutex::new(None);

    if !root.is_dir() {
        return Ok(());
    }
    let key = (
        root.to_path_buf(),
        access == crate::appcontainer::Access::ReadWrite,
    );
    if DONE
        .lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .contains(&key)
    {
        return Ok(());
    }
    if !crate::appcontainer::has_grant(container, root, access)? {
        crate::appcontainer::grant(container, root, access).map_err(|error| {
            io::Error::other(format!(
                "granting sandbox access to {}: {error}",
                root.display()
            ))
        })?;
    }
    DONE.lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(key);
    Ok(())
}

#[cfg(not(windows))]
fn spawn_isolated(
    _command: &str,
    _cwd: &Path,
    _extra_env: &HashMap<String, String>,
    _active_process_limit: u32,
    _isolation: &OsIsolation,
) -> io::Result<(ProcessChild, Containment)> {
    Err(io::Error::other(
        "os_sandbox = \"enforce\" is only supported on Windows so far; set os_sandbox = \"off\" \
         to run commands under the policy-level sandbox only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{READ_ONLY, WORKSPACE_WRITE};

    #[test]
    fn isolation_follows_the_policy() {
        let ws = std::env::temp_dir().join("ncx_iso_policy_ws");
        let off = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        assert_eq!(OsIsolation::from_policy(&off), None);
        let full = SandboxPolicy::new(DANGER_FULL_ACCESS, &ws).with_os_enforcement(true);
        assert_eq!(OsIsolation::from_policy(&full), None);

        let write = SandboxPolicy::new(WORKSPACE_WRITE, &ws)
            .with_writable_roots([ws.join("extra")])
            .with_os_enforcement(true);
        let iso = OsIsolation::from_policy(&write).unwrap();
        assert_eq!(iso.writable_roots[0], write.workspace);
        assert!(iso.writable_roots.contains(&write.writable_roots[0]));
        assert!(!iso.network);

        let read = SandboxPolicy::new(READ_ONLY, &ws)
            .with_network_access(true)
            .with_os_enforcement(true);
        let iso = OsIsolation::from_policy(&read).unwrap();
        assert!(iso.writable_roots.is_empty());
        assert!(iso.readable_roots.contains(&read.workspace));
        assert!(iso.network);
    }

    #[cfg(windows)]
    fn isolated_workspace(label: &str) -> (PathBuf, PathBuf, OsIsolation) {
        let base = std::env::temp_dir().join(format!(
            "ncx_iso_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = base.join("ws");
        let outside = base.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let isolation = OsIsolation {
            writable_roots: vec![workspace.clone()],
            readable_roots: Vec::new(),
            network: false,
        };
        (workspace, outside, isolation)
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn isolated_executor_writes_only_inside_the_workspace() {
        let (workspace, outside, isolation) = isolated_workspace("exec");
        let escaped = outside.join("escaped.txt");
        let result = crate::PolicyExecutor::new()
            .with_isolation(Some(isolation))
            .run(
                &format!(
                    "echo 中文 inside> inside.txt && type inside.txt && echo out> \"{}\"",
                    escaped.display()
                ),
                &workspace,
                30,
            )
            .await;
        assert!(workspace.join("inside.txt").exists(), "{}", result.render());
        assert!(result.stdout.contains("中文 inside"), "{}", result.render());
        assert!(
            !escaped.exists(),
            "write outside the workspace must be blocked"
        );
        assert_ne!(result.exit_code, 0, "{}", result.render());
        let _ = std::fs::remove_dir_all(workspace.parent().unwrap());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn isolated_background_process_streams_output() {
        let (workspace, _outside, isolation) = isolated_workspace("managed");
        let mut process = crate::PolicyExecutor::new()
            .with_isolation(Some(isolation))
            .spawn_managed("echo managed_isolated_ok", &workspace)
            .unwrap();
        let mut seen = String::new();
        for _ in 0..100 {
            let snapshot = process.poll(0).await.unwrap();
            seen = snapshot.chunks.iter().map(|c| c.text.clone()).collect();
            if !snapshot.running {
                assert_eq!(snapshot.exit_code, Some(0));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(seen.contains("managed_isolated_ok"), "{seen}");
        let _ = std::fs::remove_dir_all(workspace.parent().unwrap());
    }

    #[test]
    fn toolchain_roots_never_expose_home_or_drive_roots() {
        let base = std::env::temp_dir().join(format!("ncx_iso_roots_{}", std::process::id()));
        let home = base.join("home");
        let py = base.join("py");
        for dir in [
            home.join("bin"),
            home.join(".cargo").join("bin"),
            home.join(".rustup"),
            py.join("Scripts"),
            base.join("tools").join("node"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let path = std::env::join_paths([
            home.join("bin"),
            home.join(".cargo").join("bin"),
            py.join("Scripts"),
            py.clone(),
            base.join("tools").join("node"),
            home.clone(),
            base.join("missing"),
        ])
        .unwrap();
        let roots = toolchain_roots(Some(path), Some(&home));
        let home = normalize(&home);
        // `home\bin` keeps the entry itself: its parent is the home dir.
        assert!(roots.contains(&home.join("bin")), "{roots:?}");
        assert!(roots.contains(&home.join(".cargo")), "{roots:?}");
        assert!(roots.contains(&home.join(".rustup")), "{roots:?}");
        assert!(roots.contains(&normalize(&py)), "{roots:?}");
        assert!(
            roots.contains(&normalize(&base.join("tools").join("node"))),
            "{roots:?}"
        );
        assert!(
            !roots.contains(&home),
            "home itself must never be granted: {roots:?}"
        );
        assert!(roots.iter().all(|r| r.parent().is_some()), "{roots:?}");
        assert_eq!(roots.iter().filter(|r| **r == normalize(&py)).count(), 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
