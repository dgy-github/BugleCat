//! Codex-style sandbox policy — Rust port of `nanocodex/sandbox/policy.py`.
//!
//! Mirrors Codex's three sandbox modes:
//!
//! * `read-only`          — read anywhere; no writes; no network
//! * `workspace-write`    — read anywhere; write to workspace + writable roots
//!   (+ system temp only when opted in); no network unless explicitly enabled
//! * `danger-full-access` — no restrictions
//!
//! This makes *policy decisions* (is this path writable? is network allowed?).
//! Enforcement lives in the executor crate. Path comparison is **lexical**
//! (components normalized, no filesystem access / symlink resolution) so a
//! decision never depends on a path existing — matching how the Python side
//! used `Path.resolve(strict=False)` for the write-boundary check.

use std::path::{Component, Path, PathBuf};

pub const READ_ONLY: &str = "read-only";
pub const WORKSPACE_WRITE: &str = "workspace-write";
pub const DANGER_FULL_ACCESS: &str = "danger-full-access";

/// Resolved filesystem/network permissions for a sandbox mode.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub mode: String,
    pub workspace: PathBuf,
    pub writable_roots: Vec<PathBuf>,
    pub network_access: bool,
    /// Tightened default: the system temp dir is NOT writable unless opted in.
    pub allow_temp_write: bool,
}

impl SandboxPolicy {
    /// Build a policy for `mode` rooted at `workspace` (normalized absolute).
    pub fn new(mode: impl Into<String>, workspace: impl AsRef<Path>) -> Self {
        SandboxPolicy {
            mode: mode.into(),
            workspace: normalize(workspace.as_ref()),
            writable_roots: Vec::new(),
            network_access: false,
            allow_temp_write: false,
        }
    }

    pub fn with_writable_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.writable_roots = roots.into_iter().map(|p| normalize(p.as_ref())).collect();
        self
    }

    pub fn with_allow_temp_write(mut self, allow: bool) -> Self {
        self.allow_temp_write = allow;
        self
    }

    pub fn with_network_access(mut self, on: bool) -> Self {
        self.network_access = on;
        self
    }

    /// True for the modes that permit any writes at all.
    pub fn writes_allowed(&self) -> bool {
        self.mode == WORKSPACE_WRITE || self.mode == DANGER_FULL_ACCESS
    }

    fn writable_dirs(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.workspace.clone()];
        roots.extend(self.writable_roots.iter().cloned());
        if self.allow_temp_write {
            roots.push(normalize(&std::env::temp_dir()));
        }
        roots
    }

    /// All three modes permit reads (secret-file protection is a tool-layer
    /// concern, like the Python side).
    pub fn can_read(&self, _path: impl AsRef<Path>) -> bool {
        true
    }

    /// Whether a write to `path` is allowed under this policy.
    pub fn can_write(&self, path: impl AsRef<Path>) -> bool {
        if self.mode == DANGER_FULL_ACCESS {
            return true;
        }
        if self.mode == READ_ONLY {
            return false;
        }
        let target = normalize(&make_absolute(&self.workspace, path.as_ref()));
        // `starts_with` is component-wise, so it covers both `target == root`
        // and `root` being an ancestor of `target` (and won't match a mere
        // string prefix like `/a/bc` under `/a/b`).
        self.writable_dirs()
            .iter()
            .any(|root| target.starts_with(root))
    }

    pub fn describe(&self) -> String {
        let net = if self.network_access {
            "network on"
        } else {
            "network off"
        };
        match self.mode.as_str() {
            DANGER_FULL_ACCESS => format!("{} (no restrictions, {net})", self.mode),
            READ_ONLY => format!("{} (no writes, {net})", self.mode),
            _ => {
                let roots: Vec<String> = self
                    .writable_dirs()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                format!("{} ({net}; writable: {})", self.mode, roots.join(", "))
            }
        }
    }
}

/// Join `p` onto `base` when relative; leave it as-is when absolute.
fn make_absolute(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Lexically normalize a path: drop `.`, resolve `..` against prior components.
/// No filesystem access, so it works on paths that don't exist.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Mirrors tests/test_sandbox.py. Paths are built under the system temp dir
    // so they're absolute and cross-platform; no directories are created
    // (the policy check is lexical).
    fn base() -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ncx_policy_test-{}-{timestamp}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn read_only_forbids_writes() {
        let workspace = base();
        let p = SandboxPolicy::new(READ_ONLY, &workspace);
        assert!(p.can_read(workspace.join("f.py")));
        assert!(!p.can_write(workspace.join("f.py")));
        assert!(!p.writes_allowed());
    }

    #[test]
    fn workspace_write_allows_inside_only() {
        let root = base();
        let ws = root.join("ws");
        let outside = root.join("outside");
        let p = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_allow_temp_write(false);
        assert!(p.can_write(ws.join("a.py")));
        assert!(p.can_write(ws.join("sub").join("b.py")));
        assert!(!p.can_write(outside.join("c.py")));
    }

    #[test]
    fn workspace_write_denies_system_temp_by_default() {
        // Workspace lives under temp, but the temp ROOT is only writable when
        // opted in — a sibling temp file stays denied by default.
        let root = base();
        let ws = root.join("ncx_ws");
        let tmp_file = root.join("ncx_probe.txt");

        let default = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        assert!(!default.can_write(&tmp_file));

        let opted_in = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_allow_temp_write(true);
        assert!(opted_in.can_write(&tmp_file));
    }

    #[test]
    fn workspace_write_honors_extra_writable_roots() {
        let root = base();
        let ws = root.join("ws");
        let extra = root.join("extra");
        let p = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_writable_roots([&extra]);
        assert!(p.can_write(extra.join("x.py")));
    }

    #[test]
    fn danger_full_access_allows_everything() {
        let p = SandboxPolicy::new(DANGER_FULL_ACCESS, base());
        assert!(p.can_write("/etc/passwd"));
        // The policy itself does NOT force network on — callers (load_config) do.
        assert!(!p.network_access);
    }

    #[test]
    fn relative_path_resolves_against_workspace() {
        let ws = base().join("ws");
        let p = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        assert!(p.can_write("a.py")); // relative -> ws/a.py, inside
        assert!(!p.can_write(Path::new("..").join("escape.py"))); // climbs out
    }
}
