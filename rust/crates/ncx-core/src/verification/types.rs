//! Rust v2 adds exact byte spans and host provenance to the Python v1 fixture contract.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const POLICY_VERSION: &str = "host-rules-v3.3";
pub const TEMPLATE_VERSION: &str = "jev-single-claim-v2";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Shadow,
    Enforce,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub mode: Mode,
    pub model_enabled: bool,
    /// Only deterministic categories explicitly approved by the host. Jev scores
    /// are never enforcement inputs until calibration establishes a policy.
    pub enforce_categories: Vec<ClaimKind>,
    pub experimental_jev: Option<ExperimentalJevPolicy>,
}
#[derive(Debug, Clone)]
pub struct ExperimentalJevPolicy {
    pub threshold: f64,
    pub categories: Vec<ClaimKind>,
    pub version: String,
}
impl Config {
    pub fn from_env() -> Self {
        let mode = match std::env::var("NCX_JEV_MODE")
            .ok()
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("shadow") => Mode::Shadow,
            Some("enforce") => Mode::Enforce,
            _ => Mode::Off,
        };
        let model_enabled = std::env::var("NCX_JEV_MODEL_ENABLED")
            .map(|v| !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "off"))
            .unwrap_or(true);
        let enforce_categories = std::env::var("NCX_ENFORCE_CATEGORIES")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|part| match part.trim().to_lowercase().as_str() {
                        "execution_success" => Some(ClaimKind::ExecutionSuccess),
                        "tests_passed" => Some(ClaimKind::TestsPassed),
                        "semantic" => Some(ClaimKind::Semantic),
                        "limitation" => Some(ClaimKind::Limitation),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            mode,
            model_enabled,
            enforce_categories,
            experimental_jev: None,
        }
    }

    pub fn off() -> Self {
        Self {
            mode: Mode::Off,
            model_enabled: false,
            enforce_categories: vec![],
            experimental_jev: None,
        }
    }

    pub fn shadow() -> Self {
        Self {
            mode: Mode::Shadow,
            model_enabled: true,
            enforce_categories: vec![],
            experimental_jev: None,
        }
    }

    pub fn enforce(categories: Vec<ClaimKind>) -> Self {
        Self {
            mode: Mode::Enforce,
            model_enabled: true,
            enforce_categories: categories,
            experimental_jev: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.mode != Mode::Off
    }

    pub fn is_enforcing(&self) -> bool {
        self.mode == Mode::Enforce
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    NeedEvidence,
    Contradiction,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    ExecutionSuccess,
    TestsPassed,
    Semantic,
    Limitation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub kind: ClaimKind,
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub polarity: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub requested_scope: Option<String>,
    #[serde(default)]
    pub tool_hint: Option<String>,
    #[serde(default)]
    pub binding_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub hashes: BTreeMap<String, String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<u64>,
    pub workspace: String,
    pub source_ref: String,
    pub kind: String,
    pub tool_call_id: Option<String>,
    pub tool: Option<String>,
    pub command: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub timestamp: u64,
    pub content_hash: String,
    pub excerpt: String,
    pub truncated: bool,
    pub scope: Vec<String>,
    pub before: Option<FileSnapshot>,
    pub after: Option<FileSnapshot>,
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub call_sequence: Option<i64>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_paths: Vec<String>,
    #[serde(default)]
    pub authoritative: bool,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub checker: Option<String>,
    #[serde(default)]
    pub checked_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub text: String,
    pub facts: String,
    pub requirements: String,
    #[serde(default)]
    pub requirement_sources: String,
    pub unknown_fact_ids: Vec<String>,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementSource {
    pub source_ref: String,
    pub turn_id: Option<u64>,
    pub sequence: i64,
    pub workspace: String,
    pub content_hash: String,
    pub instruction: crate::instruction_state::Instruction,
    pub attachment_metadata: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationInput {
    pub schema_version: String,
    pub session_id: String,
    pub turn_id: Option<u64>,
    pub workspace: String,
    pub requirements: Vec<String>,
    pub candidate: String,
    pub candidate_hash: String,
    pub claims: Vec<Claim>,
    pub evidence: Vec<Evidence>,
    pub current_files: FileSnapshot,
    pub unresolved_call_ids: Vec<String>,
    pub coverage: String,
    pub limitations: Vec<String>,
    #[serde(default)]
    pub coverage_detail: Coverage,
    #[serde(default)]
    pub requirement_sources: Vec<RequirementSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimDecision {
    pub claim_id: String,
    pub verdict: Verdict,
    pub reason_code: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub schema_version: String,
    pub verdict: Verdict,
    pub reason_code: String,
    pub decisions: Vec<ClaimDecision>,
    pub call_status: String,
    pub candidate_hash: String,
    pub input_hash: String,
    pub policy_version: String,
    pub template_version: String,
    pub elapsed_ms: u128,
    /// Raw model probabilities are observations, never a correctness percentage.
    #[serde(default)]
    pub observations: Vec<serde_json::Value>,
    #[serde(default)]
    pub persistence_status: String,
    #[serde(default)]
    pub attempted_calls: usize,
    #[serde(default)]
    pub claim_verdict: Option<Verdict>,
    #[serde(default)]
    pub coverage: Coverage,
}

impl VerificationResult {
    pub fn is_certified_pass(&self) -> bool {
        self.verdict == Verdict::Pass
    }

    pub fn has_contradictions(&self) -> bool {
        self.decisions
            .iter()
            .any(|d| d.verdict == Verdict::Contradiction)
    }

    pub fn has_missing_evidence(&self) -> bool {
        self.decisions
            .iter()
            .any(|d| d.verdict == Verdict::NeedEvidence)
    }

    pub fn should_correct(&self, config: &Config, input: &VerificationInput) -> bool {
        config.mode == Mode::Enforce
            && self.decisions.iter().any(|d| {
                matches!(d.verdict, Verdict::NeedEvidence | Verdict::Contradiction)
                    && input
                        .claims
                        .iter()
                        .any(|c| c.id == d.claim_id && config.enforce_categories.contains(&c.kind))
            })
    }
    pub fn feedback(&self) -> String {
        let issues: Vec<_> = self
            .decisions
            .iter()
            .filter(|d| matches!(d.verdict, Verdict::NeedEvidence | Verdict::Contradiction))
            .map(|d| {
                format!(
                    "{}: {} (sources: {})",
                    d.claim_id,
                    d.reason_code,
                    d.evidence_ids.join(",")
                )
            })
            .collect();
        format!("Host verification rejected draft: {}. Revise the claim or collect the missing read-only evidence using existing tools and permissions. Do not automatically replay writes. Unknown is not verified.", issues.join("; "))
    }
}
