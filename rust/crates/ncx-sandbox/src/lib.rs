//! ncx-sandbox — Codex-style sandbox policy + approval state machine.
//!
//! Rust port of `nanocodex/sandbox/` (`policy.py` + `approval.py`). Two
//! orthogonal axes gate every action, mirroring Codex:
//!
//! * [`SandboxPolicy`] — what is physically allowed (reads / writes / network).
//! * [`Approver`] — what to do when an action exceeds the sandbox
//!   (auto-approve / ask / auto-deny), per the four approval policies.
//!
//! Both layers are pure decisions; enforcement lives in the executor crate.

pub mod approval;
pub mod policy;

pub use approval::{
    command_needs_escalation, command_uses_network, is_trusted, looks_read_only,
    matches_dangerous_pattern, split_chain, step_decision, ApprovalRequest, Approver, Decision,
    NEVER, ON_FAILURE, ON_REQUEST, UNTRUSTED,
};
pub use policy::{
    is_protected_write_path, is_sensitive_read_path, normalize, resolve_for_policy,
    strip_verbatim_prefix, SandboxPolicy, DANGER_FULL_ACCESS, READ_ONLY, WORKSPACE_WRITE,
};

#[derive(Debug, Clone)]
pub struct PolicyService {
    pub sandbox: SandboxPolicy,
    pub approval_policy: String,
    pub plan_mode: bool,
}

/// Compatibility name retained for callers that only treated the policy as a snapshot.
pub type PolicySnapshot = PolicyService;
