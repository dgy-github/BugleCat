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
    /// Also enforce this policy at the OS level (`os_sandbox = "enforce"`):
    /// shell commands run in an isolated process that can only write the
    /// writable roots and only reach the network when it is enabled.
    pub os_enforced: bool,
    /// Optional scoped write boundary. When `Some`, writes are restricted
    /// strictly to these directories rather than the entire workspace.
    pub scoped_writable_roots: Option<Vec<PathBuf>>,
    /// Optional scoped read boundary. When `Some`, reads are restricted to
    /// these directories (plus `effective_writable_roots`), while sensitive
    /// files remain blocked.
    pub scoped_read_roots: Option<Vec<PathBuf>>,
    /// Explicit paths/directories locked against runtime writes (e.g. scope
    /// manifests and installed scope tool directories).
    pub protected_paths: Vec<PathBuf>,
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
            os_enforced: false,
            scoped_writable_roots: None,
            scoped_read_roots: None,
            protected_paths: Vec::new(),
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

    pub fn with_os_enforcement(mut self, on: bool) -> Self {
        self.os_enforced = on;
        self
    }

    pub fn with_protected_paths<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        for p in paths {
            let resolved = resolve_for_policy(&self.workspace, p.as_ref());
            if !self.protected_paths.contains(&resolved) {
                self.protected_paths.push(resolved);
            }
        }
        self
    }

    /// True when writes are bound to an explicit scope allowlist and must not
    /// escalate outside it via interactive approval.
    pub fn is_write_scope_restricted(&self) -> bool {
        self.scoped_writable_roots.is_some()
    }

    /// Monotonically narrow this policy for a Scoped Agent (`ncx-scope/v1`).
    ///
    /// Never widens any axis of `self`:
    /// * Global `read-only` stays `read-only` even if `writable_roots` is non-empty.
    /// * Global `danger-full-access` drops down to `workspace-write` (or `read-only`).
    /// * Every candidate writable root must already be writable under `self`.
    /// * Network is `self.network_access && network`.
    /// * OS enforcement is `self.os_enforced || enforce_os_sandbox`.
    pub fn narrow(
        &self,
        read_roots: Option<&[PathBuf]>,
        writable_roots: &[PathBuf],
        network: bool,
        enforce_os_sandbox: bool,
        extra_protected_paths: &[PathBuf],
    ) -> Self {
        let mut protected = self.protected_paths.clone();
        for p in extra_protected_paths {
            let resolved = resolve_for_policy(&self.workspace, p);
            if !protected.contains(&resolved) {
                protected.push(resolved);
            }
        }

        let allowed_writes: Vec<PathBuf> = if self.writes_allowed() {
            writable_roots
                .iter()
                .map(|root| resolve_for_policy(&self.workspace, root))
                .filter(|resolved| self.can_write(resolved))
                .filter(|resolved| !protected.iter().any(|prot| resolved.starts_with(prot)))
                .collect()
        } else {
            Vec::new()
        };

        let narrowed_mode = if allowed_writes.is_empty() {
            READ_ONLY.to_string()
        } else {
            WORKSPACE_WRITE.to_string()
        };

        let raw_reads: Vec<PathBuf> = match read_roots {
            Some(roots) => roots
                .iter()
                .map(|root| resolve_for_policy(&self.workspace, root))
                .collect(),
            None => vec![resolve_for_policy(&self.workspace, &self.workspace)],
        };
        let allowed_reads: Vec<PathBuf> = raw_reads
            .into_iter()
            .filter(|resolved| match &self.scoped_read_roots {
                Some(existing) => existing
                    .iter()
                    .map(|r| resolve_for_policy(&self.workspace, r))
                    .any(|r| resolved.starts_with(&r)),
                None => true,
            })
            .collect();

        SandboxPolicy {
            mode: narrowed_mode,
            workspace: self.workspace.clone(),
            writable_roots: self.writable_roots.clone(),
            network_access: self.network_access && network,
            allow_temp_write: false,
            os_enforced: self.os_enforced || enforce_os_sandbox,
            scoped_writable_roots: Some(allowed_writes),
            scoped_read_roots: Some(allowed_reads),
            protected_paths: protected,
        }
    }

    /// True for the modes that permit any writes at all.
    pub fn writes_allowed(&self) -> bool {
        if let Some(scoped) = &self.scoped_writable_roots {
            if scoped.is_empty() {
                return false;
            }
        }
        self.mode == WORKSPACE_WRITE || self.mode == DANGER_FULL_ACCESS
    }

    /// Resolved directory trees writable without escalation.
    pub fn effective_writable_roots(&self) -> Vec<PathBuf> {
        if !self.writes_allowed() {
            return Vec::new();
        }
        if let Some(scoped) = &self.scoped_writable_roots {
            return scoped
                .iter()
                .map(|root| resolve_for_policy(&self.workspace, root))
                .collect();
        }
        let mut roots = vec![resolve_for_policy(&self.workspace, &self.workspace)];
        roots.extend(
            self.writable_roots
                .iter()
                .map(|root| resolve_for_policy(&self.workspace, root)),
        );
        if self.allow_temp_write {
            let temp = std::env::temp_dir();
            roots.push(resolve_for_policy(&temp, &temp));
        }
        roots
    }

    /// Resolved directory trees readable when a scope read boundary is active.
    /// `None` means standard unscoped reads (all non-sensitive paths).
    pub fn effective_readable_roots(&self) -> Option<Vec<PathBuf>> {
        let scoped = self.scoped_read_roots.as_ref()?;
        let mut roots: Vec<PathBuf> = scoped
            .iter()
            .map(|root| resolve_for_policy(&self.workspace, root))
            .collect();
        for writable in self.effective_writable_roots() {
            if !roots.contains(&writable) {
                roots.push(writable);
            }
        }
        Some(roots)
    }

    fn is_runtime_protected(&self, lexical: &Path, target: &Path) -> bool {
        self.protected_paths.iter().any(|prot| {
            let resolved_prot = resolve_for_policy(&self.workspace, prot);
            target.starts_with(&resolved_prot) || lexical.starts_with(&resolved_prot)
        })
    }

    /// Whether reading `path` is permitted under this policy.
    ///
    /// Outside `danger-full-access`, sensitive credential and private-key files
    /// (`.env`, SSH private keys, `.git-credentials`, `.nanocodex/config.toml`,
    /// cloud credentials, etc.) are blocked from automatic reads.
    pub fn can_read(&self, path: impl AsRef<Path>) -> bool {
        if self.mode == DANGER_FULL_ACCESS && self.scoped_read_roots.is_none() {
            return true;
        }
        let raw = path.as_ref();
        if is_sensitive_read_path(raw) {
            return false;
        }
        let target = resolve_for_policy(&self.workspace, raw);
        if is_sensitive_read_path(&target) {
            return false;
        }
        if let Some(roots) = self.effective_readable_roots() {
            return roots.iter().any(|root| target.starts_with(root));
        }
        true
    }

    /// Whether a write to `path` is allowed under this policy without escalation.
    ///
    /// Resolves existing ancestor symlinks/junctions to prevent symlink escape
    /// out of the workspace, and treats execution-hijack or secret targets
    /// (`.git/hooks/**`, `.git/config`, `.env*`, `.nanocodex/config.toml`) as
    /// requiring explicit escalation even inside `writable_roots`.
    pub fn can_write(&self, path: impl AsRef<Path>) -> bool {
        let raw = path.as_ref();
        let lexical = normalize(&make_absolute(&self.workspace, raw));
        let target = resolve_for_policy(&self.workspace, raw);
        if self.is_runtime_protected(&lexical, &target) {
            return false;
        }
        if self.mode == DANGER_FULL_ACCESS && self.scoped_writable_roots.is_none() {
            return true;
        }
        if !self.writes_allowed() {
            return false;
        }
        if is_protected_write_path(raw)
            || is_protected_write_path(&lexical)
            || is_protected_write_path(&target)
        {
            return false;
        }
        // `starts_with` is component-wise, so it covers both `target == root`
        // and `root` being an ancestor of `target` (and won't match a mere
        // string prefix like `/a/bc` under `/a/b`).
        self.effective_writable_roots()
            .iter()
            .any(|root| target.starts_with(root))
    }

    pub fn describe(&self) -> String {
        let net = match (
            self.network_access,
            self.os_enforced && self.mode != DANGER_FULL_ACCESS,
        ) {
            (true, false) => "network on",
            (false, false) => "network off",
            (true, true) => "network on, OS-enforced",
            (false, true) => "network off, OS-enforced",
        };
        match self.mode.as_str() {
            DANGER_FULL_ACCESS => format!("{} (no restrictions, {net})", self.mode),
            READ_ONLY => format!("{} (no writes, {net})", self.mode),
            _ => {
                let roots: Vec<String> = self
                    .effective_writable_roots()
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
pub fn normalize(p: &Path) -> PathBuf {
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

/// Strip Windows extended-length path prefixes (`\\?\` and `\\?\UNC\`) so
/// canonicalized paths compare cleanly with lexically normalized paths.
pub fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    text.strip_prefix(r"\\?\")
        .map_or_else(|| path.clone(), PathBuf::from)
}

/// Normalize `p` against `base` and resolve the deepest existing ancestor on
/// disk so symlinks/junctions inside the workspace cannot escape the boundary.
pub fn resolve_for_policy(base: &Path, p: &Path) -> PathBuf {
    let lexical = normalize(&make_absolute(base, p));
    let mut current: &Path = &lexical;
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical) = current.canonicalize() {
            let mut resolved = strip_verbatim_prefix(canonical);
            for part in suffix.into_iter().rev() {
                resolved.push(part);
            }
            return normalize(&resolved);
        }
        let Some(name) = current.file_name() else {
            break;
        };
        suffix.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    lexical
}

/// True when `path` refers to a credential, private key, or secret file that
/// should not be read without `danger-full-access`.
pub fn is_sensitive_read_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower_name = file_name.to_ascii_lowercase();
    if is_secret_dotenv_name(&lower_name) {
        return true;
    }
    if matches!(
        lower_name.as_str(),
        "id_rsa"
            | "id_ed25519"
            | "id_ecdsa"
            | "id_dsa"
            | ".git-credentials"
            | ".netrc"
            | ".npmrc"
            | ".pypirc"
            | "authorized_keys"
    ) {
        return true;
    }
    if let Some(ext) = Path::new(&lower_name).extension().and_then(|e| e.to_str()) {
        if matches!(ext, "pem" | "key" | "pfx" | "p12" | "kdbx") {
            return true;
        }
    }
    let comps: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(os) => Some(os.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    for window in comps.windows(2) {
        if matches!(
            (window[0].as_str(), window[1].as_str()),
            (".nanocodex", "config.toml")
                | (".aws", "credentials")
                | (".ssh", "config")
                | (".kube", "config")
                | (".git", "credentials")
        ) {
            return true;
        }
    }
    false
}

/// True when `path` targets an execution-hijack hook/config or secret file
/// inside the workspace that must require explicit escalation before writing.
pub fn is_protected_write_path(path: &Path) -> bool {
    if is_sensitive_read_path(path) {
        return true;
    }
    let comps: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(os) => Some(os.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    for window in comps.windows(2) {
        if window[0] == ".git" && matches!(window[1].as_str(), "hooks" | "config") {
            return true;
        }
        if window[0] == ".nanocodex" && matches!(window[1].as_str(), "agents" | "providers.json") {
            return true;
        }
    }
    false
}

fn is_secret_dotenv_name(lower_name: &str) -> bool {
    if lower_name == ".env" {
        return true;
    }
    if let Some(suffix) = lower_name.strip_prefix(".env.") {
        return !matches!(
            suffix,
            "example" | "sample" | "template" | "dist" | "schema" | "defaults"
        );
    }
    false
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
        assert!(p.can_read(".env"));
        assert!(p.can_write(".git/hooks/pre-commit"));
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

    #[test]
    fn sensitive_credential_files_blocked_from_read_and_write_outside_full_access() {
        let ws = base().join("ws");
        let p = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        assert!(!p.can_read(ws.join(".env")));
        assert!(!p.can_read(ws.join(".env.local")));
        assert!(p.can_read(ws.join(".env.example")));
        assert!(!p.can_read(ws.join("secrets/server.pem")));
        assert!(!p.can_read(ws.join(".nanocodex/config.toml")));
        assert!(!p.can_write(ws.join(".env")));
        assert!(!p.can_write(ws.join(".git/hooks/pre-commit")));
        assert!(!p.can_write(ws.join(".git/config")));
        assert!(!p.can_write(ws.join(".nanocodex/agents/refund.toml")));
    }

    #[test]
    fn scope_narrowing_never_widens_global_policy_and_protects_scope_files() {
        let ws = base().join("ws");
        let global_ro = SandboxPolicy::new(READ_ONLY, &ws);
        let narrowed_ro = global_ro.narrow(
            Some(&[PathBuf::from("data/orders")]),
            &[PathBuf::from("out/refund")],
            true,
            true,
            &[PathBuf::from("agents")],
        );
        // Global read-only cannot be widened to write or network.
        assert_eq!(narrowed_ro.mode, READ_ONLY);
        assert!(!narrowed_ro.writes_allowed());
        assert!(!narrowed_ro.can_write(ws.join("out/refund/a.json")));
        assert!(!narrowed_ro.network_access);
        assert!(narrowed_ro.os_enforced);
        assert!(narrowed_ro.can_read(ws.join("data/orders/1.json")));
        assert!(!narrowed_ro.can_read(ws.join("src/secret_logic.rs")));

        // Global workspace-write narrowed to out/refund blocks sibling writes and agents/.
        let global_rw = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_network_access(true);
        let narrowed_rw = global_rw.narrow(
            Some(&[
                PathBuf::from("data/orders"),
                PathBuf::from("docs/refund-policy"),
            ]),
            &[PathBuf::from("out/refund"), PathBuf::from("../escape")],
            false,
            false,
            &[PathBuf::from("agents")],
        );
        assert_eq!(narrowed_rw.mode, WORKSPACE_WRITE);
        assert!(!narrowed_rw.network_access);
        assert!(narrowed_rw.can_write(ws.join("out/refund/decision.json")));
        assert!(narrowed_rw.can_read(ws.join("out/refund/decision.json")));
        assert!(!narrowed_rw.can_write(ws.join("data/orders/1.json")));
        assert!(!narrowed_rw.can_write(ws.join("agents/refund.toml")));
        assert!(!narrowed_rw.can_write(ws.join("../escape/x.txt")));
    }
}
