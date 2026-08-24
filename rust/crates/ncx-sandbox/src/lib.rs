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
    step_decision, ApprovalRequest, Approver, Decision, NEVER, ON_FAILURE, ON_REQUEST, UNTRUSTED,
};
pub use policy::{SandboxPolicy, DANGER_FULL_ACCESS, READ_ONLY, WORKSPACE_WRITE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySnapshot {
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub plan_mode: bool,
    pub network_access: bool,
}
