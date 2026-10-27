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
    /// Trees scanned for protected paths (`.git/hooks`, `.git/config`,
    /// secret files) whose container access is narrowed below the grants.
    pub protected_roots: Vec<PathBuf>,
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
        let mut protected_roots = vec![policy.workspace.clone()];
        protected_roots.extend(policy.writable_roots.iter().cloned());
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
            protected_roots,
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

/// A note for the output of a failed isolated command when the container
/// can't query the volume holding `cwd` or a writable root: cmd's `del` and
/// `dir` then fail with "access denied". `None` once every volume is set up
/// (see `ncx --os-sandbox-setup`), and always off Windows.
pub fn setup_hint(isolation: &OsIsolation, cwd: &Path) -> Option<String> {
    let missing = unready_volume_roots(isolation, cwd);
    if missing.is_empty() {
        return None;
    }
    let roots: Vec<String> = missing.iter().map(|r| r.display().to_string()).collect();
    Some(format!(
        "[sandbox] cmd's `del` and `dir` fail with \"access denied\" here because the OS \
         sandbox has no access to the drive root {}. Run `ncx --os-sandbox-setup` once (one \
         administrator prompt) to fix it; until then `rd /s /q` still removes directories.",
        roots.join(", ")
    ))
}

/// Volume roots under `cwd` and the writable roots that the container can't
/// query yet. Roots found ready are remembered for the life of the process.
#[cfg(windows)]
pub fn unready_volume_roots(isolation: &OsIsolation, cwd: &Path) -> Vec<PathBuf> {
    use crate::appcontainer::{shared, volume_root, volume_root_ready};
    use std::sync::Mutex;
    static READY: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    let Ok(container) = shared() else {
        return Vec::new();
    };
    let mut missing: Vec<PathBuf> = Vec::new();
    let paths = std::iter::once(cwd).chain(isolation.writable_roots.iter().map(PathBuf::as_path));
    for path in paths {
        let Ok(root) = volume_root(path) else {
            continue;
        };
        if missing.contains(&root) || READY.lock().unwrap().contains(&root) {
            continue;
        }
        match volume_root_ready(container, &root) {
            Ok(true) => READY.lock().unwrap().push(root),
            Ok(false) => missing.push(root),
            Err(_) => {}
        }
    }
    missing
}

#[cfg(not(windows))]
pub fn unready_volume_roots(_isolation: &OsIsolation, _cwd: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// How a protected path is shielded from isolated commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shield {
    /// Secret file: no reading, writing, deleting, or renaming.
    Secret,
    /// Hook config (`.git/config`, a `.git` pointer file): read-only.
    Locked,
    /// Hook directory (`.git/hooks`): read-only, including everything below.
    LockedTree,
    /// The `.git` directory: usable, but it can't be deleted or renamed (which
    /// would let a command swap in a repository with its own hooks).
    Pinned,
}

/// A path under a protected root and how it is shielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedPath {
    pub path: PathBuf,
    pub shield: Shield,
}

/// Directories never descended into while scanning: large, generated, and
/// not where project secrets or repositories live.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
];

/// Directory entries visited per scan before giving up on the rest.
const SCAN_BUDGET: usize = 200_000;

/// Find the paths under `roots` that isolated commands must not touch: secret
/// files (per [`ncx_sandbox::is_sensitive_read_path`]), and in every git
/// repository the `.git` directory, its `config` and `hooks`, or a `.git`
/// pointer file. `.git/hooks` is listed even when missing, so it can be
/// created and locked before a command could plant hooks there. Symlinks and
/// junctions are not followed.
pub fn protected_paths(roots: &[PathBuf]) -> Vec<ProtectedPath> {
    let mut found = Vec::new();
    let mut budget = SCAN_BUDGET;
    for root in roots {
        scan_dir(root, &mut found, &mut budget);
    }
    found
}

fn scan_dir(dir: &Path, found: &mut Vec<ProtectedPath>, budget: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name == ".git" {
            if file_type.is_dir() {
                found.push(ProtectedPath {
                    path: path.clone(),
                    shield: Shield::Pinned,
                });
                found.push(ProtectedPath {
                    path: path.join("hooks"),
                    shield: Shield::LockedTree,
                });
                for (file, shield) in [("config", Shield::Locked), ("credentials", Shield::Secret)]
                {
                    if path.join(file).is_file() {
                        found.push(ProtectedPath {
                            path: path.join(file),
                            shield,
                        });
                    }
                }
            } else {
                found.push(ProtectedPath {
                    path,
                    shield: Shield::Locked,
                });
            }
        } else if file_type.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                scan_dir(&path, found, budget);
            }
        } else if ncx_sandbox::is_sensitive_read_path(&path) {
            found.push(ProtectedPath {
                path,
                shield: Shield::Secret,
            });
        }
    }
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
    shield_protected(container, &isolation.protected_roots)?;
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

/// Protected roots are rescanned at most this often; restrictions applied by
/// an earlier scan stay on the files in between.
#[cfg(windows)]
const RESCAN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Narrow the container's access to every [`protected_paths`] entry under
/// `roots`, then take [`REMOVE_RIGHTS`](crate::appcontainer::REMOVE_RIGHTS)
/// off each entry's parent: a parent's `FILE_DELETE_CHILD` would otherwise
/// still let a command delete or rename the entry. Rights are only ever
/// intersected with what the container already has, never widened.
#[cfg(windows)]
fn shield_protected(
    container: &crate::appcontainer::AppContainer,
    roots: &[PathBuf],
) -> io::Result<()> {
    use crate::appcontainer::{ContainerRights, READ_RIGHTS, REMOVE_RIGHTS};
    use std::sync::Mutex;
    use std::time::Instant;
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    static SCANNED: Mutex<Option<HashMap<Vec<PathBuf>, Instant>>> = Mutex::new(None);

    if roots.is_empty() {
        return Ok(());
    }
    {
        let mut scanned = SCANNED.lock().unwrap();
        let scanned = scanned.get_or_insert_with(HashMap::new);
        if scanned
            .get(roots)
            .is_some_and(|at| at.elapsed() < RESCAN_INTERVAL)
        {
            return Ok(());
        }
    }
    let mut parents: Vec<PathBuf> = Vec::new();
    for target in protected_paths(roots) {
        if target.shield == Shield::LockedTree && !target.path.exists() {
            std::fs::create_dir(&target.path).map_err(|error| {
                io::Error::other(format!(
                    "creating {} to protect it from sandboxed commands: {error}",
                    target.path.display()
                ))
            })?;
        }
        let allowed = match target.shield {
            Shield::Secret => ContainerRights::default(),
            Shield::Locked => ContainerRights {
                own: READ_RIGHTS,
                inherited: 0,
            },
            Shield::LockedTree => ContainerRights {
                own: READ_RIGHTS,
                inherited: READ_RIGHTS,
            },
            Shield::Pinned => ContainerRights {
                own: FILE_ALL_ACCESS & !REMOVE_RIGHTS,
                inherited: FILE_ALL_ACCESS,
            },
        };
        ensure_restricted(container, &target.path, allowed)?;
        if let Some(parent) = target.path.parent() {
            if !parents.iter().any(|p| p == parent) {
                parents.push(parent.to_path_buf());
            }
        }
    }
    for parent in &parents {
        let allowed = ContainerRights {
            own: !REMOVE_RIGHTS,
            inherited: u32::MAX,
        };
        ensure_restricted(container, parent, allowed)?;
    }
    SCANNED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(roots.to_vec(), Instant::now());
    Ok(())
}

/// Cut the container's rights on `path` down to at most `allowed`; a no-op
/// when they are already within it.
#[cfg(windows)]
fn ensure_restricted(
    container: &crate::appcontainer::AppContainer,
    path: &Path,
    allowed: crate::appcontainer::ContainerRights,
) -> io::Result<()> {
    use crate::appcontainer::{container_rights, restrict, ContainerRights};

    let protect = |error: io::Error| {
        io::Error::other(format!(
            "protecting {} from sandboxed commands: {error}",
            path.display()
        ))
    };
    let (current, _) = container_rights(container, path).map_err(protect)?;
    let target = ContainerRights {
        own: current.own & allowed.own,
        inherited: current.inherited & allowed.inherited,
    };
    if target == current {
        return Ok(());
    }
    restrict(container, path, target).map_err(protect)
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
        assert_eq!(
            iso.protected_roots,
            vec![write.workspace.clone(), write.writable_roots[0].clone()]
        );

        let read = SandboxPolicy::new(READ_ONLY, &ws)
            .with_network_access(true)
            .with_os_enforcement(true);
        let iso = OsIsolation::from_policy(&read).unwrap();
        assert!(iso.writable_roots.is_empty());
        assert!(iso.readable_roots.contains(&read.workspace));
        assert!(iso.network);
        assert_eq!(iso.protected_roots, vec![read.workspace.clone()]);
    }

    fn scan_fixture(label: &str) -> PathBuf {
        let ws = std::env::temp_dir().join(format!(
            "ncx_iso_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for dir in [
            ws.join(".git").join("hooks"),
            ws.join("src"),
            ws.join("sub"),
            ws.join("node_modules").join("pkg"),
            ws.join("vendor").join("lib"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for (file, text) in [
            (ws.join(".git").join("config"), "[core]\n"),
            (
                ws.join(".git").join("hooks").join("pre-commit.sample"),
                "#!/bin/sh\n",
            ),
            (ws.join(".env"), "SECRET=TOPSECRET\n"),
            (ws.join(".env.example"), "EXAMPLE=1\n"),
            (ws.join("sub").join(".env"), "SECRET=TOPSECRET\n"),
            (ws.join("src").join("a.txt"), "a\n"),
            (ws.join("node_modules").join("pkg").join(".env"), "x\n"),
            (
                ws.join("vendor").join("lib").join(".git"),
                "gitdir: ../../.git\n",
            ),
        ] {
            std::fs::write(file, text).unwrap();
        }
        ws
    }

    #[test]
    fn scanner_finds_repositories_and_secrets() {
        let ws = scan_fixture("scan");
        let found = protected_paths(std::slice::from_ref(&ws));
        let shield_of = |path: PathBuf| found.iter().find(|p| p.path == path).map(|p| p.shield);
        assert_eq!(shield_of(ws.join(".git")), Some(Shield::Pinned));
        assert_eq!(
            shield_of(ws.join(".git").join("hooks")),
            Some(Shield::LockedTree)
        );
        assert_eq!(
            shield_of(ws.join(".git").join("config")),
            Some(Shield::Locked)
        );
        assert_eq!(shield_of(ws.join(".env")), Some(Shield::Secret));
        assert_eq!(shield_of(ws.join("sub").join(".env")), Some(Shield::Secret));
        assert_eq!(
            shield_of(ws.join("vendor").join("lib").join(".git")),
            Some(Shield::Locked)
        );
        assert_eq!(shield_of(ws.join(".env.example")), None);
        assert_eq!(shield_of(ws.join("src").join("a.txt")), None);
        assert_eq!(
            shield_of(ws.join("node_modules").join("pkg").join(".env")),
            None,
            "generated dirs are not scanned"
        );
        assert_eq!(
            shield_of(ws.join(".git").join("hooks").join("pre-commit.sample")),
            None,
            "nothing inside .git beyond the named entries"
        );

        // A repository without a hooks dir still reports it, so it gets locked.
        std::fs::remove_dir_all(ws.join(".git").join("hooks")).unwrap();
        let found = protected_paths(std::slice::from_ref(&ws));
        assert!(found
            .iter()
            .any(|p| p.path == ws.join(".git").join("hooks") && p.shield == Shield::LockedTree));
        let _ = std::fs::remove_dir_all(ws);
    }

    #[cfg(windows)]
    fn shielded_isolation(ws: &Path) -> OsIsolation {
        OsIsolation {
            writable_roots: vec![ws.to_path_buf()],
            readable_roots: Vec::new(),
            network: false,
            protected_roots: vec![ws.to_path_buf()],
        }
    }

    #[cfg(windows)]
    async fn run_isolated(
        isolation: &OsIsolation,
        workspace: &Path,
        command: &str,
    ) -> crate::ExecResult {
        crate::PolicyExecutor::new()
            .with_isolation(Some(isolation.clone()))
            .run(command, workspace, 30)
            .await
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn isolated_commands_cannot_touch_protected_paths() {
        let ws = scan_fixture("shield");
        let isolation = shielded_isolation(&ws);
        let git = ws.join(".git");
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();

        // Ordinary work still runs (and shields the workspace on first spawn):
        // files anywhere else, including new ones inside `.git` and `sub`.
        let result = run_isolated(
            &isolation,
            &ws,
            r"echo ok> root.txt && echo ok>> src\a.txt && echo ok> sub\b.txt && echo x> .git\index.lock && type .git\config && type .env.example",
        )
        .await;
        assert_eq!(result.exit_code, 0, "{}", result.render());
        assert!(result.stdout.contains("[core]"), "{}", result.render());
        assert!(result.stdout.contains("EXAMPLE=1"), "{}", result.render());
        assert_eq!(read(&ws.join("root.txt")).trim(), "ok");
        assert!(read(&ws.join("src").join("a.txt")).starts_with("a\nok"));
        assert_eq!(read(&ws.join("sub").join("b.txt")).trim(), "ok");
        assert_eq!(read(&git.join("index.lock")).trim(), "x");
        let container = crate::appcontainer::shared().unwrap();
        assert!(
            crate::appcontainer::has_grant(container, &ws, crate::appcontainer::Access::ReadWrite)
                .unwrap(),
            "the workspace grant must survive narrowing the root"
        );
        // The user's own access is untouched.
        std::fs::write(ws.join(".env"), "SECRET=TOPSECRET\nMORE=1\n").unwrap();
        std::fs::write(git.join("hooks").join("pre-push"), "#!/bin/sh\n").unwrap();
        assert!(read(&git.join("config")).contains("[core]"));

        // Secrets can't be read, copied, or removed.
        for command in [r"type .env", r"type sub\.env", r"copy .env leaked.txt"] {
            let result = run_isolated(&isolation, &ws, command).await;
            assert!(
                !result.render().contains("TOPSECRET"),
                "{command}: {}",
                result.render()
            );
        }
        assert!(!read(&ws.join("leaked.txt")).contains("TOPSECRET"));
        run_isolated(&isolation, &ws, r"del /f /q .env & ren .env moved.env").await;
        assert!(read(&ws.join(".env")).contains("TOPSECRET"));
        assert!(!ws.join("moved.env").exists());

        // Hook config and hooks can't be changed, removed, re-ACLed, or swapped out.
        for command in [
            r"echo [core]hooksPath=evil>> .git\config",
            r"del /f /q .git\config",
            r"ren .git\config config.bak",
            r"icacls .git\config /reset",
            r"echo evil> .git\hooks\pre-commit",
            r"echo evil> .git\hooks\pre-commit.sample",
            r"ren .git\hooks hooks.bak",
            r"rd /s /q .git\hooks",
            r"ren .git git.bak",
            r"echo gitdir: evil> vendor\lib\.git",
        ] {
            run_isolated(&isolation, &ws, command).await;
        }
        assert_eq!(read(&git.join("config")), "[core]\n");
        assert!(!git.join("config.bak").exists());
        assert!(!git.join("hooks").join("pre-commit").exists());
        assert_eq!(
            read(&git.join("hooks").join("pre-commit.sample")),
            "#!/bin/sh\n"
        );
        assert!(!git.join("hooks.bak").exists());
        assert!(!ws.join("git.bak").exists());
        assert_eq!(
            read(&ws.join("vendor").join("lib").join(".git")),
            "gitdir: ../../.git\n"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shielding_never_widens_read_only_access() {
        use crate::appcontainer::{container_rights, REMOVE_RIGHTS};
        use windows_sys::Win32::Storage::FileSystem::{FILE_APPEND_DATA, FILE_WRITE_DATA};

        let ws = scan_fixture("readonly");
        let isolation = OsIsolation {
            writable_roots: Vec::new(),
            readable_roots: vec![ws.clone()],
            network: false,
            protected_roots: vec![ws.clone()],
        };
        let result = run_isolated(&isolation, &ws, r"type .env & echo x> .git\x.txt").await;
        assert!(
            !result.render().contains("TOPSECRET"),
            "{}",
            result.render()
        );
        assert!(!ws.join(".git").join("x.txt").exists());
        let container = crate::appcontainer::shared().unwrap();
        let (rights, _) = container_rights(container, &ws.join(".git")).unwrap();
        let write = FILE_WRITE_DATA | FILE_APPEND_DATA | REMOVE_RIGHTS;
        assert_eq!(rights.own & write, 0, "{rights:?}");
        assert_eq!(rights.inherited & write, 0, "{rights:?}");
        let (rights, _) = container_rights(container, &ws.join(".env")).unwrap();
        assert_eq!(rights, Default::default());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_commands_point_to_setup_while_the_drive_is_unready() {
        let (workspace, _outside, isolation) = isolated_workspace("hint");
        let unready = !unready_volume_roots(&isolation, &workspace).is_empty();
        let ok = run_isolated(&isolation, &workspace, "echo fine").await;
        assert_eq!(ok.exit_code, 0, "{}", ok.render());
        assert!(!ok.stderr.contains("--os-sandbox-setup"), "{}", ok.render());

        let result = run_isolated(&isolation, &workspace, "echo a> a.txt && del a.txt").await;
        if unready {
            assert_ne!(result.exit_code, 0, "{}", result.render());
            assert!(
                result.stderr.contains("ncx --os-sandbox-setup"),
                "{}",
                result.render()
            );
        } else {
            assert_eq!(result.exit_code, 0, "{}", result.render());
            assert!(!workspace.join("a.txt").exists());
        }
        let _ = std::fs::remove_dir_all(workspace.parent().unwrap());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn missing_hooks_dir_is_created_locked() {
        let ws = scan_fixture("nohooks");
        std::fs::remove_dir_all(ws.join(".git").join("hooks")).unwrap();
        let isolation = shielded_isolation(&ws);
        let result = run_isolated(&isolation, &ws, r"echo evil> .git\hooks\post-checkout").await;
        assert_ne!(result.exit_code, 0, "{}", result.render());
        assert!(ws.join(".git").join("hooks").is_dir());
        assert!(!ws.join(".git").join("hooks").join("post-checkout").exists());
        let _ = std::fs::remove_dir_all(&ws);
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
            protected_roots: vec![workspace.clone()],
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
