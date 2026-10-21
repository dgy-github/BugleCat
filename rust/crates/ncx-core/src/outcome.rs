//! Autonomous Completion Rate (ACR) outcome recording, followup signals,
//! Wilson confidence intervals, 60-point singularity evaluation, and
//! online routing feedback statistics.
//!
//! Privacy guarantee: `TaskOutcome` never stores raw prompt text; only a
//! deterministic hash (`query_hash`) and structured metadata are persisted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_state::AuthoritySource;
use crate::context_store::{ContextEvent, ContextStore, EventStatus, EventType};
use crate::memory::MemoryStore;

/// Token usage summary persisted with a `TaskOutcome`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

pub const MIN_SLICE_SAMPLE: usize = 20;
pub const SINGULARITY_MIN_SAMPLE: usize = 30;
pub const SINGULARITY_TARGET_LOWER_BOUND: f64 = 0.60;
pub const SINGULARITY_MAX_REWORK_RATE: f64 = 0.15;
/// Shadow baseline length before any feedback switch may go `on` (plan N6).
pub const BASELINE_MIN_DAYS: u64 = 14;
/// Categories that need `MIN_BASELINE_CATEGORY_SAMPLE` determined tasks.
pub const BASELINE_MIN_CATEGORIES: usize = 2;
pub const MIN_BASELINE_CATEGORY_SAMPLE: usize = 30;

/// Role of a turn in the task hierarchy. Only `Primary` turns are counted in
/// the user-facing ACR denominator; worker/node turns are linked via `parent_task_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskRole {
    #[default]
    Primary,
    OrchestratorWorker,
    OrchestratorNode,
    GoalRound,
}

impl TaskRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::OrchestratorWorker => "orchestrator_worker",
            Self::OrchestratorNode => "orchestrator_node",
            Self::GoalRound => "goal_round",
        }
    }
}

/// Deterministic task category. `Chat` turns (no tool calls and no code/command
/// deliverables) are tracked separately and excluded from the engineering ACR denominator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Bugfix,
    Feature,
    Refactor,
    Test,
    Build,
    Docs,
    Explain,
    Chat,
    Other,
}

impl TaskCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bugfix => "bugfix",
            Self::Feature => "feature",
            Self::Refactor => "refactor",
            Self::Test => "test",
            Self::Build => "build",
            Self::Docs => "docs",
            Self::Explain => "explain",
            Self::Chat => "chat",
            Self::Other => "other",
        }
    }

    /// Deterministic rule-based classification from the user prompt and tools used.
    /// Zero LLM calls.
    pub fn classify(query: &str, tools_used: &[String], has_deliverable: bool) -> Self {
        let q = query.trim().to_lowercase();
        if tools_used.is_empty() && !has_deliverable {
            return Self::Chat;
        }
        let has_feature_cue = contains_term_any(
            &q,
            &[
                "add",
                "implement",
                "implementation",
                "create",
                "support",
                "feature",
                "build a",
                "新增",
                "实现",
                "开发",
                "加上",
                "支持",
                "添加",
                "実装",
                "追加",
            ],
        );
        if contains_term_any(
            &q,
            &[
                "bug", "fix", "panic", "crash", "broken", "修复", "报错", "崩溃", "故障", "バグ",
                "修正",
            ],
        ) || (!has_feature_cue
            && contains_term_any(&q, &["error", "issue", "failure", "异常", "解决"]))
        {
            return Self::Bugfix;
        }
        if contains_term_any(
            &q,
            &[
                "refactor",
                "cleanup",
                "clean up",
                "rename",
                "restructure",
                "重构",
                "清理",
                "整理",
                "リファクタ",
            ],
        ) {
            return Self::Refactor;
        }
        if contains_term_any(
            &q,
            &[
                "test",
                "pytest",
                "cargo test",
                "unit test",
                "测试",
                "单测",
                "テスト",
            ],
        ) {
            return Self::Test;
        }
        if contains_term_any(
            &q,
            &[
                "build",
                "compile",
                "compiling",
                "release",
                "package",
                "bundle",
                "ci",
                "编译",
                "构建",
                "打包",
                "发布",
                "ビルド",
            ],
        ) {
            return Self::Build;
        }
        if !has_feature_cue
            && contains_term_any(
                &q,
                &[
                    "readme",
                    "doc",
                    "docs",
                    "comment",
                    "markdown",
                    "文档",
                    "注释",
                    "说明",
                    "ドキュメント",
                ],
            )
        {
            return Self::Docs;
        }
        if contains_term_any(
            &q,
            &[
                "explain",
                "why",
                "how does",
                "what is",
                "walkthrough",
                "解释",
                "为什么",
                "怎么工作",
                "分析",
                "看下",
                "说明一下",
            ],
        ) && !tools_used.iter().any(|t| {
            matches!(
                t.as_str(),
                "write_file" | "edit_file" | "apply_patch" | "multi_edit"
            )
        }) {
            return Self::Explain;
        }
        if has_feature_cue {
            return Self::Feature;
        }
        if tools_used.iter().any(|t| {
            matches!(
                t.as_str(),
                "write_file" | "edit_file" | "apply_patch" | "multi_edit"
            )
        }) {
            return Self::Feature;
        }
        Self::Other
    }
}

/// Orchestrator / execution routing key attached to a task outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteKey {
    /// `"Simple"` | `"Medium"` | `"Complex"`
    pub complexity: String,
    /// `"single"` | `"flash+pro"` | `"dag"`
    pub tier_config: String,
    pub verify_passed: bool,
    pub verify_rounds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_recommended_route: Option<String>,
    /// Complexity chosen by the classifier before any routing-feedback upgrade.
    /// Routing statistics bucket on this so an `on`-mode upgrade cannot starve
    /// the original bucket of fresh samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_complexity: Option<String>,
}

/// Complete record of a single turn's execution outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub task_id: String,
    pub parent_task_id: Option<String>,
    pub session_id: String,
    pub turn_id: u64,
    pub role: TaskRole,
    pub authority: AuthoritySource,
    pub category: TaskCategory,
    /// Deterministic hash of the normalized prompt (never stores raw user text).
    pub query_hash: String,
    pub model: String,
    pub fast_model: Option<String>,
    pub permission_mode: String,
    pub route: Option<RouteKey>,
    pub terminal_state: String,
    pub stop_reason: String,
    /// `"passed"` | `"failed"` | `"skipped"` | `"unverified"`
    pub verification: Option<String>,
    pub iterations: usize,
    pub tool_calls: usize,
    pub tools_used: Vec<String>,
    pub approvals_requested: u32,
    pub approvals_denied: u32,
    pub user_questions: u32,
    pub recalled_note_ids: Vec<String>,
    pub files_touched: Vec<String>,
    /// Post-turn content hashes of `files_touched` for I5 manual-edit detection.
    #[serde(default)]
    pub file_hashes: BTreeMap<String, String>,
    pub workspace_revision: Option<String>,
    pub usage: Usage,
    pub duration_ms: u64,
    pub timestamp_ms: u64,
    /// Whether feedback-weighted recall would select different notes than plain
    /// recall on this turn (`None` when memory feedback is off or nothing was recalled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_rank_changed: Option<bool>,
}

/// Post-turn signal observed on the next human turn, checkpoint restore, or `/outcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowupSignal {
    /// Next turn proceeded to a new topic without rollback/manual-edit/correction.
    Accepted,
    /// User issued a correction turn (`inferred = true` for heuristic keyword match).
    Correction {
        inferred: bool,
        note: Option<String>,
    },
    /// User restored a checkpoint after the turn.
    Rollback { checkpoint_id: Option<String> },
    /// User manually edited one or more `files_touched` before the next turn.
    ManualEdit { files: Vec<String> },
    /// Explicit user rating via `/outcome good|bad [note]`, GUI thumbs, or A/B accept.
    Label { good: bool, note: Option<String> },
}

/// Followup event linked to an earlier `TaskOutcome` by `task_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeFollowup {
    pub task_id: String,
    pub timestamp_ms: u64,
    pub signal: FollowupSignal,
}

/// 95% Wilson score confidence interval for a binomial proportion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WilsonCi {
    pub center: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Aggregated ACR metrics for one slice (or overall).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeSliceReport {
    pub key: String,
    /// Determined tasks only (see `pending_count`).
    pub sample_size: usize,
    /// Completed tasks with no followup yet: excluded from every rate.
    pub pending_count: usize,
    pub sufficient_sample: bool,
    pub status: String,
    pub strict_successes: usize,
    pub acr_strict: f64,
    pub acr_strict_ci: WilsonCi,
    pub tolerant_successes: usize,
    pub acr_tolerant: f64,
    pub acr_tolerant_ci: WilsonCi,
    pub rework_count: usize,
    pub rework_rate: f64,
    pub inferred_correction_count: usize,
    pub first_pass_verify_rate: f64,
    pub avg_interventions: f64,
    pub tokens_per_accepted_task: Option<u64>,
}

/// Evaluation against the 60-point singularity milestone:
/// rolling window `n >= 30`, `ACR-strict` Wilson 95% lower bound `>= 0.60`,
/// and `rework_rate <= 0.15`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingularityStatus {
    pub reached: bool,
    pub sample_size: usize,
    pub min_required_sample: usize,
    pub acr_strict: f64,
    pub acr_strict_lower_bound: f64,
    pub target_lower_bound: f64,
    pub distance_to_singularity: f64,
    pub rework_rate: f64,
    pub max_rework_rate: f64,
}

/// Singularity status of one task category (the plan's unit of "crossing").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySingularity {
    pub category: String,
    pub status: SingularityStatus,
}

/// What shadow-mode feedback would have changed if switched to `on`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShadowSummary {
    /// Primary tasks where memory feedback compared plain vs weighted recall.
    pub memory_observed: usize,
    /// ...of which weighted recall would have injected different notes.
    pub memory_rank_changed: usize,
    /// Orchestrated primary tasks.
    pub routing_observed: usize,
    /// ...of which routing feedback recommended an upgrade.
    pub routing_would_upgrade: usize,
    /// Determined tasks among the would-upgrade ones, and their strict successes
    /// on the route actually taken (the baseline an upgrade must beat).
    pub routing_would_upgrade_determined: usize,
    pub routing_would_upgrade_strict_successes: usize,
}

/// Full report returned by `ncx --outcomes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeReport {
    pub window_label: String,
    pub total_events_scanned: usize,
    pub chat_turns_excluded: usize,
    pub worker_turns_excluded: usize,
    pub overall: OutcomeSliceReport,
    /// Overall status, for reference; crossing is judged per category.
    pub singularity: SingularityStatus,
    pub category_singularity: Vec<CategorySingularity>,
    pub shadow: ShadowSummary,
    pub baseline: BaselineReadiness,
    pub slices: Vec<OutcomeSliceReport>,
}

/// Whether the shadow baseline is long and large enough to *consider*
/// switching a feedback mode to `on`. Meeting it is necessary, not
/// sufficient: the Shadow section still needs a human look.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaselineReadiness {
    /// Days between the first and last primary outcome in the window.
    pub days_observed: u64,
    /// Categories with at least `MIN_BASELINE_CATEGORY_SAMPLE` determined tasks.
    pub categories_ready: Vec<String>,
    pub memory_ready: bool,
    pub routing_ready: bool,
    /// Unmet criteria, human-readable.
    pub blockers: Vec<String>,
}

fn baseline_readiness(
    primary: &[&TaskOutcome],
    categories: &[CategorySingularity],
    shadow: &ShadowSummary,
) -> BaselineReadiness {
    let first = primary.iter().map(|o| o.timestamp_ms).min();
    let last = primary.iter().map(|o| o.timestamp_ms).max();
    let days_observed = match (first, last) {
        (Some(a), Some(b)) => b.saturating_sub(a) / 86_400_000,
        _ => 0,
    };
    let categories_ready: Vec<String> = categories
        .iter()
        .filter(|c| c.status.sample_size >= MIN_BASELINE_CATEGORY_SAMPLE)
        .map(|c| c.category.clone())
        .collect();
    let mut common = Vec::new();
    if days_observed < BASELINE_MIN_DAYS {
        common.push(format!("baseline {days_observed}/{BASELINE_MIN_DAYS} days"));
    }
    if categories_ready.len() < BASELINE_MIN_CATEGORIES {
        common.push(format!(
            "{}/{BASELINE_MIN_CATEGORIES} categories with n>={MIN_BASELINE_CATEGORY_SAMPLE} determined tasks",
            categories_ready.len()
        ));
    }
    let mut blockers = common.clone();
    let memory_sample_ok = shadow.memory_observed >= MIN_BASELINE_CATEGORY_SAMPLE;
    if !memory_sample_ok {
        blockers.push(format!(
            "memory: {}/{MIN_BASELINE_CATEGORY_SAMPLE} tasks with shadow recall comparison",
            shadow.memory_observed
        ));
    }
    let routing_sample_ok = shadow.routing_would_upgrade_determined >= MIN_SLICE_SAMPLE;
    if !routing_sample_ok {
        blockers.push(format!(
            "routing: {}/{MIN_SLICE_SAMPLE} determined tasks where an upgrade was recommended",
            shadow.routing_would_upgrade_determined
        ));
    }
    BaselineReadiness {
        days_observed,
        categories_ready,
        memory_ready: common.is_empty() && memory_sample_ok,
        routing_ready: common.is_empty() && routing_sample_ok,
        blockers,
    }
}

fn singularity_status(slice: &OutcomeSliceReport) -> SingularityStatus {
    SingularityStatus {
        reached: slice.sample_size >= SINGULARITY_MIN_SAMPLE
            && slice.acr_strict_ci.lower >= SINGULARITY_TARGET_LOWER_BOUND
            && slice.rework_rate <= SINGULARITY_MAX_REWORK_RATE,
        sample_size: slice.sample_size,
        min_required_sample: SINGULARITY_MIN_SAMPLE,
        acr_strict: slice.acr_strict,
        acr_strict_lower_bound: slice.acr_strict_ci.lower,
        target_lower_bound: SINGULARITY_TARGET_LOWER_BOUND,
        distance_to_singularity: slice.acr_strict_ci.lower - SINGULARITY_TARGET_LOWER_BOUND,
        rework_rate: slice.rework_rate,
        max_rework_rate: SINGULARITY_MAX_REWORK_RATE,
    }
}

fn shadow_summary(
    primary: &[&TaskOutcome],
    followup_map: &BTreeMap<String, Vec<OutcomeFollowup>>,
) -> ShadowSummary {
    let mut summary = ShadowSummary::default();
    for o in primary {
        if let Some(changed) = o.memory_rank_changed {
            summary.memory_observed += 1;
            if changed {
                summary.memory_rank_changed += 1;
            }
        }
        let Some(route) = &o.route else {
            continue;
        };
        summary.routing_observed += 1;
        if route.shadow_recommended_route.is_none() {
            continue;
        }
        summary.routing_would_upgrade += 1;
        let eval = evaluate_task(o, followup_map.get(&o.task_id).map(|v| v.as_slice()));
        if eval.determined {
            summary.routing_would_upgrade_determined += 1;
            if eval.strict_pass {
                summary.routing_would_upgrade_strict_successes += 1;
            }
        }
    }
    summary
}

/// Historical routing performance for online orchestrator complexity upgrades (P5).
#[derive(Debug, Clone, Default)]
pub struct RoutingStats {
    pub buckets: BTreeMap<(TaskCategory, String), RoutingBucket>,
}

#[derive(Debug, Clone, Default)]
pub struct RoutingBucket {
    pub sample_size: usize,
    pub strict_successes: usize,
    pub token_samples: Vec<u64>,
}

impl RoutingStats {
    pub fn from_outcomes(outcomes: &[TaskOutcome], followups: &[OutcomeFollowup]) -> Self {
        let followup_map = group_followups(followups);
        let mut stats = Self::default();
        for o in outcomes {
            if o.role != TaskRole::Primary || o.category == TaskCategory::Chat {
                continue;
            }
            // Direct (non-orchestrated) turns carry no route and say nothing
            // about orchestrator tiers.
            let Some(route) = &o.route else {
                continue;
            };
            // Bucket on the classifier's own choice so `on`-mode upgrades keep
            // feeding the bucket that triggered them.
            let complexity = route
                .initial_complexity
                .clone()
                .unwrap_or_else(|| route.complexity.clone());
            let eval = evaluate_task(o, followup_map.get(&o.task_id).map(|v| v.as_slice()));
            if !eval.determined {
                continue;
            }
            let bucket = stats.buckets.entry((o.category, complexity)).or_default();
            bucket.sample_size += 1;
            if eval.strict_pass {
                bucket.strict_successes += 1;
            }
            bucket.token_samples.push(o.usage.total_tokens);
        }
        stats
    }

    /// Recommend a monotonic complexity upgrade (`Simple -> Medium` or `Medium -> Complex`)
    /// when a `(category, complexity)` bucket has `n >= 20` and Wilson 95% lower bound `< 0.40`.
    /// Never downgrades.
    pub fn recommend_complexity_upgrade(
        &self,
        category: TaskCategory,
        initial_complexity: &str,
    ) -> Option<(String, String)> {
        let bucket = self
            .buckets
            .get(&(category, initial_complexity.to_string()))?;
        if bucket.sample_size < MIN_SLICE_SAMPLE {
            return None;
        }
        let ci = wilson_ci(bucket.strict_successes, bucket.sample_size);
        if ci.lower >= 0.40 {
            return None;
        }
        let upgraded = match initial_complexity {
            "Simple" => "Medium",
            "Medium" => "Complex",
            _ => return None,
        };
        let reason = format!(
            "category={} complexity={} n={} wilson_lower={:.2} < 0.40 -> {}",
            category.as_str(),
            initial_complexity,
            bucket.sample_size,
            ci.lower,
            upgraded
        );
        Some((upgraded.to_string(), reason))
    }
}

/// Compute the 95% Wilson score interval ($z = 1.959963984540054$).
pub fn wilson_ci(successes: usize, total: usize) -> WilsonCi {
    if total == 0 {
        return WilsonCi {
            center: 0.0,
            lower: 0.0,
            upper: 0.0,
        };
    }
    let n = total as f64;
    let k = successes.min(total) as f64;
    let p = k / n;
    let z = 1.959963984540054_f64;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let rad = (p * (1.0 - p) / n + z2 / (4.0 * n * n)).max(0.0).sqrt();
    let half = (z * rad) / denom;
    WilsonCi {
        center: center.clamp(0.0, 1.0),
        lower: (center - half).clamp(0.0, 1.0),
        upper: (center + half).clamp(0.0, 1.0),
    }
}

/// Deterministic 64-bit FNV-1a hex hash of normalized text.
pub fn hash_text(text: &str) -> String {
    let normalized: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in normalized.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Snapshot FNV-1a hashes of files touched during a turn so the next human turn
/// can detect I5 manual edits before the user speaks again.
pub fn snapshot_file_hashes(workspace: &Path, files: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for f in files {
        let candidate = Path::new(f);
        let full = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace.join(candidate)
        };
        if let Ok(bytes) = std::fs::read(&full) {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in &bytes {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x100000001b3);
            }
            out.insert(f.clone(), format!("{h:016x}"));
        }
    }
    out
}

/// Compare current on-disk files against the hashes recorded at the end of the previous turn.
pub fn detect_manual_edits(
    workspace: &Path,
    recorded_hashes: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut edited = Vec::new();
    for (file, old_hash) in recorded_hashes {
        let candidate = Path::new(file);
        let full = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace.join(candidate)
        };
        match std::fs::read(&full) {
            Ok(bytes) => {
                let mut h: u64 = 0xcbf29ce484222325;
                for b in &bytes {
                    h ^= u64::from(*b);
                    h = h.wrapping_mul(0x100000001b3);
                }
                let current_hash = format!("{h:016x}");
                if &current_hash != old_hash {
                    edited.push(file.clone());
                }
            }
            Err(_) => {
                // File was deleted manually after the agent touched it.
                edited.push(file.clone());
            }
        }
    }
    edited
}

/// Heuristic multilingual (ZH/EN/JA) detector for I4 correction prompts.
/// Tagged with `inferred: true` so reports can separate heuristic from explicit labels.
pub fn is_correction_prompt(prompt: &str, prev_files_touched: &[String]) -> bool {
    let lower = prompt.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let strong_cues = [
        "不对",
        "错了",
        "改错了",
        "没修好",
        "还是报错",
        "还是不对",
        "还是有问题",
        "不是这个意思",
        "重来",
        "回退",
        "撤销",
        "依然报错",
        "没生效",
        "still broken",
        "still fails",
        "still failing",
        "didn't work",
        "did not work",
        "not working",
        "you broke",
        "that's wrong",
        "thats wrong",
        "wrong file",
        "revert that",
        "undo that",
        "try again",
        "違う",
        "やり直し",
        "直っていない",
        "動かない",
    ];
    if contains_term_any(&lower, &strong_cues) {
        return true;
    }
    let weak_cues = [
        "error",
        "bug",
        "fail",
        "failure",
        "broken",
        "wrong",
        "问题",
        "报错",
        "失败",
        "エラー",
    ];
    if contains_term_any(&lower, &weak_cues) {
        for f in prev_files_touched {
            let basename = Path::new(f)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(f)
                .to_lowercase();
            if !basename.is_empty() && lower.contains(&basename) {
                return true;
            }
        }
    }
    false
}

/// Resolve the workspace `events.sqlite3` path from an optional session log path
/// or the workspace `.nanocodex/sessions/events.sqlite3` location.
pub fn workspace_events_db_path(workspace: &Path, session_log_path: Option<&Path>) -> PathBuf {
    if let Some(log_path) = session_log_path {
        let dir = log_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        return dir.join("events.sqlite3");
    }
    workspace
        .join(".nanocodex")
        .join("sessions")
        .join("events.sqlite3")
}

/// Append a `TaskOutcome` to `ContextStore`.
pub fn append_task_outcome(store: &ContextStore, outcome: &TaskOutcome) -> Result<(), String> {
    let payload_json = serde_json::to_string(outcome).map_err(|e| e.to_string())?;
    let status = if outcome.stop_reason == "completed" {
        EventStatus::Completed
    } else {
        EventStatus::Failed
    };
    let mut ev = ContextEvent::new(
        format!("{}:task_outcome", outcome.task_id),
        outcome.session_id.clone(),
        EventType::TaskOutcome,
        status,
    );
    ev.task_id = Some(outcome.task_id.clone());
    ev.turn_id = Some(outcome.turn_id);
    ev.timestamp = outcome.timestamp_ms;
    ev.payload_json = payload_json;
    store.append(&ev).map(|_| ())
}

/// Append an `OutcomeFollowup` event to `ContextStore`.
pub fn append_outcome_followup(
    store: &ContextStore,
    session_id: &str,
    workspace: &str,
    followup: &OutcomeFollowup,
) -> Result<(), String> {
    let payload_json = serde_json::to_string(followup).map_err(|e| e.to_string())?;
    let sig_tag = match &followup.signal {
        FollowupSignal::Accepted => "accepted",
        FollowupSignal::Correction { .. } => "correction",
        FollowupSignal::Rollback { .. } => "rollback",
        FollowupSignal::ManualEdit { .. } => "manual_edit",
        FollowupSignal::Label { .. } => "label",
    };
    let event_id = format!(
        "{}:followup:{}:{}",
        followup.task_id, sig_tag, followup.timestamp_ms
    );
    let mut ev = ContextEvent::new(
        event_id,
        session_id.to_string(),
        EventType::OutcomeFollowup,
        EventStatus::Completed,
    );
    ev.task_id = Some(followup.task_id.clone());
    ev.timestamp = followup.timestamp_ms;
    ev.workspace = Some(workspace.to_string());
    ev.payload_json = payload_json;
    store.append(&ev).map(|_| ())
}

/// Record a `CheckpointRestore` event in `ContextStore`.
pub fn append_checkpoint_restore(
    store: &ContextStore,
    session_id: &str,
    workspace: &str,
    checkpoint_id: &str,
) -> Result<(), String> {
    let ts = now_millis();
    let mut ev = ContextEvent::new(
        format!("checkpoint_restore:{checkpoint_id}:{ts}"),
        session_id.to_string(),
        EventType::CheckpointRestore,
        EventStatus::Completed,
    );
    ev.timestamp = ts;
    ev.workspace = Some(workspace.to_string());
    ev.payload_json = serde_json::json!({
        "checkpoint_id": checkpoint_id,
        "timestamp_ms": ts,
    })
    .to_string();
    store.append(&ev).map(|_| ())
}

/// Load all `TaskOutcome` and `OutcomeFollowup` records from `ContextStore` since `since_ms`.
pub fn load_outcomes_and_followups(
    store: &ContextStore,
    since_ms: Option<u64>,
) -> Result<(Vec<TaskOutcome>, Vec<OutcomeFollowup>), String> {
    let outcome_events = store.events_by_type(EventType::TaskOutcome, since_ms)?;
    let followup_events = store.events_by_type(EventType::OutcomeFollowup, None)?;
    Ok((
        parse_outcomes(outcome_events),
        parse_followups(followup_events),
    ))
}

fn parse_outcomes(events: Vec<ContextEvent>) -> Vec<TaskOutcome> {
    events
        .into_iter()
        .filter_map(|ev| serde_json::from_str::<TaskOutcome>(&ev.payload_json).ok())
        .collect()
}

fn parse_followups(events: Vec<ContextEvent>) -> Vec<OutcomeFollowup> {
    events
        .into_iter()
        .filter_map(|ev| serde_json::from_str::<OutcomeFollowup>(&ev.payload_json).ok())
        .collect()
}

/// Followups already attached to one task, oldest first.
pub fn followups_for_task(
    store: &ContextStore,
    task_id: &str,
) -> Result<Vec<OutcomeFollowup>, String> {
    Ok(parse_followups(store.task_events_by_type(
        task_id,
        EventType::OutcomeFollowup,
    )?))
}

/// All primary outcomes recorded under `session_id` in chronological (append) order.
pub fn primary_outcomes_for_session(
    store: &ContextStore,
    session_id: &str,
) -> Result<Vec<TaskOutcome>, String> {
    Ok(
        parse_outcomes(store.session_events_by_type(session_id, EventType::TaskOutcome)?)
            .into_iter()
            .filter(|o| o.role == TaskRole::Primary)
            .collect(),
    )
}

/// The outcome recorded under exactly `task_id`, if any.
pub fn outcome_by_task_id(
    store: &ContextStore,
    task_id: &str,
) -> Result<Option<TaskOutcome>, String> {
    Ok(
        parse_outcomes(store.task_events_by_type(task_id, EventType::TaskOutcome)?)
            .into_iter()
            .last(),
    )
}

/// Most recent primary outcome recorded under `session_id` (append order).
pub fn latest_primary_outcome(
    store: &ContextStore,
    session_id: &str,
) -> Result<Option<TaskOutcome>, String> {
    Ok(primary_outcomes_for_session(store, session_id)?
        .into_iter()
        .last())
}

/// Resolve the previous primary task of `session_id` when the human speaks
/// again: manual edit (I5) > inferred correction (I4) > accepted. A task
/// that already has a followup (label, rollback, earlier resolution) is
/// left alone, so each task is auto-resolved at most once.
pub fn resolve_previous_primary_followup(
    store: &ContextStore,
    workspace: &Path,
    session_id: &str,
    next_query: &str,
    memory: Option<MemoryFeedback<'_>>,
) {
    let Ok(Some(prev)) = latest_primary_outcome(store, session_id) else {
        return;
    };
    if !followups_for_task(store, &prev.task_id)
        .map(|f| f.is_empty())
        .unwrap_or(false)
    {
        return;
    }
    let manual_edits = if prev.file_hashes.is_empty() {
        Vec::new()
    } else {
        detect_manual_edits(workspace, &prev.file_hashes)
    };
    let signal = if !manual_edits.is_empty() {
        FollowupSignal::ManualEdit {
            files: manual_edits,
        }
    } else if is_correction_prompt(next_query, &prev.files_touched) {
        FollowupSignal::Correction {
            inferred: true,
            note: None,
        }
    } else {
        FollowupSignal::Accepted
    };
    let ws_str = workspace.to_string_lossy().to_string();
    let _ = record_followup(store, &ws_str, &prev, signal, memory);
}

/// Verdict implied by a task's followups: the last human label wins;
/// otherwise any rollback / manual edit / correction rejects, and a bare
/// `Accepted` accepts. `None` while nothing has been observed.
pub fn followup_polarity(followups: &[OutcomeFollowup]) -> Option<bool> {
    if let Some(good) = followups.iter().rev().find_map(|f| match &f.signal {
        FollowupSignal::Label { good, .. } => Some(*good),
        _ => None,
    }) {
        return Some(good);
    }
    let mut verdict = None;
    for f in followups {
        match f.signal {
            FollowupSignal::Accepted => {
                verdict.get_or_insert(true);
            }
            FollowupSignal::Correction { .. }
            | FollowupSignal::Rollback { .. }
            | FollowupSignal::ManualEdit { .. } => verdict = Some(false),
            FollowupSignal::Label { .. } => {}
        }
    }
    verdict
}

/// Memory store and `[feedback] memory` mode used for outcome-driven scoring.
#[derive(Clone, Copy)]
pub struct MemoryFeedback<'a> {
    pub store: &'a MemoryStore,
    pub mode: &'a str,
}

/// Append a followup for `outcome` and, unless memory feedback is `off`, score
/// the notes that task recalled. Memory is scored once per task: the first
/// verdict records an injection plus its polarity; a later followup that flips
/// the task's overall verdict (e.g. a human label) revises that single count.
/// Demotion is only applied in mode `on`.
pub fn record_followup(
    store: &ContextStore,
    workspace: &str,
    outcome: &TaskOutcome,
    signal: FollowupSignal,
    memory: Option<MemoryFeedback<'_>>,
) -> Result<OutcomeFollowup, String> {
    let before = followup_polarity(&followups_for_task(store, &outcome.task_id)?);
    let followup = OutcomeFollowup {
        task_id: outcome.task_id.clone(),
        timestamp_ms: now_millis(),
        signal,
    };
    append_outcome_followup(store, &outcome.session_id, workspace, &followup)?;
    let after = followup_polarity(&followups_for_task(store, &outcome.task_id)?);
    let Some(memory) = memory.filter(|m| m.mode != "off") else {
        return Ok(followup);
    };
    let Ok(index) = crate::memory_index::MemoryIndex::for_store(memory.store) else {
        return Ok(followup);
    };
    let ws_key = memory.store.workspace_key();
    let allow_demote = memory.mode == "on";
    if allow_demote {
        let turn_prefix = format!("{}#turn-{}:", outcome.session_id, outcome.turn_id);
        match after {
            Some(true) => {
                for note in index.notes_for_source_prefix(&ws_key, &turn_prefix) {
                    let _ = memory
                        .store
                        .remember(&note.text, &note.tags, note.created_at);
                    let _ = index.confirm_candidate(&note.note_id);
                }
            }
            Some(false) => {
                for note in index.notes_for_source_prefix(&ws_key, &turn_prefix) {
                    let _ = memory.store.forget_text(&note.text);
                    let _ = index.remove_note(&note.note_id);
                }
            }
            None => {}
        }
    }
    if outcome.recalled_note_ids.is_empty() {
        return Ok(followup);
    }
    let attributed_ids = filter_attributed_note_ids(
        memory.store,
        &outcome.recalled_note_ids,
        &outcome.files_touched,
    );
    match (before, after) {
        (None, Some(accepted)) => {
            let target_ids = if accepted {
                &outcome.recalled_note_ids
            } else {
                &attributed_ids
            };
            index.record_outcome_feedback(
                &ws_key,
                target_ids,
                accepted,
                followup.timestamp_ms,
                allow_demote,
            );
        }
        (Some(prev), Some(now)) if prev != now => {
            let target_ids = if now {
                &outcome.recalled_note_ids
            } else {
                &attributed_ids
            };
            index.revise_outcome_feedback(
                &ws_key,
                target_ids,
                now,
                followup.timestamp_ms,
                allow_demote,
            );
        }
        _ => {}
    }
    Ok(followup)
}

fn filter_attributed_note_ids(
    store: &MemoryStore,
    recalled_note_ids: &[String],
    files_touched: &[String],
) -> Vec<String> {
    if files_touched.is_empty() {
        return recalled_note_ids.to_vec();
    }
    let stems: Vec<String> = files_touched
        .iter()
        .filter_map(|f| {
            std::path::Path::new(f)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.trim().to_ascii_lowercase())
        })
        .map(|s| s.strip_prefix("test_").unwrap_or(&s).to_string())
        .filter(|s| s.len() >= 3)
        .collect();
    if stems.is_empty() {
        return recalled_note_ids.to_vec();
    }
    let entries = store.entries();
    let mut kept = Vec::new();
    for nid in recalled_note_ids {
        let Some(entry) = entries
            .iter()
            .find(|e| MemoryStore::note_id(&e.text) == *nid)
        else {
            kept.push(nid.clone());
            continue;
        };
        let is_file_specific = entry.text.contains("modified `") || !entry.tags.is_empty();
        if !is_file_specific {
            kept.push(nid.clone());
            continue;
        }
        let lower_text = entry.text.to_ascii_lowercase();
        let matches_stem = stems.iter().any(|stem| {
            entry.tags.iter().any(|t| t.eq_ignore_ascii_case(stem)) || lower_text.contains(stem)
        });
        if matches_stem {
            kept.push(nid.clone());
        }
    }
    if kept.is_empty() {
        recalled_note_ids.to_vec()
    } else {
        kept
    }
}

/// I3 rollback. Records the restore event, then attributes it to the latest
/// primary task that started after the checkpoint was created (restricted to
/// `session_id` when known). Tasks older than the checkpoint were not undone,
/// so they are never blamed. Returns the task id that received the signal.
pub fn record_checkpoint_rollback(
    store: &ContextStore,
    workspace: &Path,
    checkpoint_id: &str,
    checkpoint_created_ms: u64,
    session_id: Option<&str>,
    memory: Option<MemoryFeedback<'_>>,
) -> Result<Option<String>, String> {
    let ws_str = workspace.to_string_lossy().to_string();
    append_checkpoint_restore(
        store,
        session_id.unwrap_or("checkpoint"),
        &ws_str,
        checkpoint_id,
    )?;
    let candidates = match session_id {
        Some(sid) => parse_outcomes(store.session_events_by_type(sid, EventType::TaskOutcome)?),
        None => load_outcomes_and_followups(store, Some(checkpoint_created_ms))?.0,
    };
    let Some(target) = candidates
        .into_iter()
        .filter(|o| o.role == TaskRole::Primary)
        .filter(|o| o.timestamp_ms.saturating_sub(o.duration_ms) >= checkpoint_created_ms)
        .max_by_key(|o| o.timestamp_ms)
    else {
        return Ok(None);
    };
    record_followup(
        store,
        &ws_str,
        &target,
        FollowupSignal::Rollback {
            checkpoint_id: Some(checkpoint_id.to_string()),
        },
        memory,
    )?;
    Ok(Some(target.task_id))
}

struct TaskEval {
    /// Has a verdict: either the turn itself failed (I1), or a followup arrived.
    /// Undetermined tasks are reported as pending, never counted as successes.
    determined: bool,
    strict_pass: bool,
    tolerant_pass: bool,
    reworked: bool,
    inferred_correction: bool,
    first_pass_verified: bool,
    interventions: u32,
}

fn evaluate_task(outcome: &TaskOutcome, followups: Option<&[OutcomeFollowup]>) -> TaskEval {
    let followups = followups.unwrap_or(&[]);
    let mut explicit_bad = false;
    let mut has_label = false;
    let mut has_rollback = false;
    let mut has_manual_edit = false;
    let mut has_correction = false;
    let mut inferred_correction = false;

    for f in followups {
        match &f.signal {
            FollowupSignal::Accepted => {}
            FollowupSignal::Rollback { .. } => has_rollback = true,
            FollowupSignal::ManualEdit { .. } => has_manual_edit = true,
            FollowupSignal::Correction { inferred, .. } => {
                has_correction = true;
                if *inferred {
                    inferred_correction = true;
                }
            }
            FollowupSignal::Label { good, .. } => {
                has_label = true;
                if !*good {
                    explicit_bad = true;
                }
            }
        }
    }

    let reworked = followup_polarity(followups) == Some(false);

    let mut interventions = outcome.user_questions + outcome.approvals_requested;
    if has_rollback {
        interventions += 1;
    }
    if has_manual_edit {
        interventions += 1;
    }
    if has_correction || explicit_bad {
        interventions += 1;
    }

    let completed_ok = outcome.stop_reason == "completed"
        && outcome.terminal_state.eq_ignore_ascii_case("completed")
        && outcome.verification.as_deref() != Some("failed");

    let strict_pass = completed_ok
        && outcome.user_questions == 0
        && outcome.approvals_requested == 0
        && outcome.approvals_denied == 0
        && !reworked;

    let tolerant_pass =
        completed_ok && outcome.user_questions == 0 && outcome.approvals_denied == 0 && !reworked;

    let first_pass_verified = completed_ok
        && outcome.verification.as_deref() == Some("passed")
        && outcome
            .route
            .as_ref()
            .map(|r| r.verify_rounds <= 1)
            .unwrap_or(true);

    TaskEval {
        determined: !completed_ok || !followups.is_empty(),
        strict_pass,
        tolerant_pass,
        reworked,
        inferred_correction: inferred_correction && !has_label,
        first_pass_verified,
        interventions,
    }
}

fn group_followups(followups: &[OutcomeFollowup]) -> BTreeMap<String, Vec<OutcomeFollowup>> {
    let mut map: BTreeMap<String, Vec<OutcomeFollowup>> = BTreeMap::new();
    for f in followups {
        map.entry(f.task_id.clone()).or_default().push(f.clone());
    }
    map
}

fn build_slice_report(
    key: String,
    outcomes: &[&TaskOutcome],
    followup_map: &BTreeMap<String, Vec<OutcomeFollowup>>,
) -> OutcomeSliceReport {
    let mut n = 0usize;
    let mut pending_count = 0usize;
    let mut strict_successes = 0usize;
    let mut tolerant_successes = 0usize;
    let mut rework_count = 0usize;
    let mut inferred_correction_count = 0usize;
    let mut first_pass_count = 0usize;
    let mut total_interventions = 0u64;
    let mut total_tokens = 0u64;

    for o in outcomes {
        let eval = evaluate_task(o, followup_map.get(&o.task_id).map(|v| v.as_slice()));
        if !eval.determined {
            pending_count += 1;
            continue;
        }
        n += 1;
        if eval.strict_pass {
            strict_successes += 1;
        }
        if eval.tolerant_pass {
            tolerant_successes += 1;
        }
        if eval.reworked {
            rework_count += 1;
        }
        if eval.inferred_correction {
            inferred_correction_count += 1;
        }
        if eval.first_pass_verified {
            first_pass_count += 1;
        }
        total_interventions += u64::from(eval.interventions);
        total_tokens += o.usage.total_tokens;
    }

    let acr_strict = if n > 0 {
        strict_successes as f64 / n as f64
    } else {
        0.0
    };
    let acr_tolerant = if n > 0 {
        tolerant_successes as f64 / n as f64
    } else {
        0.0
    };
    let rework_rate = if n > 0 {
        rework_count as f64 / n as f64
    } else {
        0.0
    };
    let first_pass_verify_rate = if n > 0 {
        first_pass_count as f64 / n as f64
    } else {
        0.0
    };
    let avg_interventions = if n > 0 {
        total_interventions as f64 / n as f64
    } else {
        0.0
    };
    let sufficient_sample = n >= MIN_SLICE_SAMPLE;
    let status = if sufficient_sample {
        "ok".to_string()
    } else {
        "insufficient_sample".to_string()
    };

    OutcomeSliceReport {
        key,
        sample_size: n,
        pending_count,
        sufficient_sample,
        status,
        strict_successes,
        acr_strict,
        acr_strict_ci: wilson_ci(strict_successes, n),
        tolerant_successes,
        acr_tolerant,
        acr_tolerant_ci: wilson_ci(tolerant_successes, n),
        rework_count,
        rework_rate,
        inferred_correction_count,
        first_pass_verify_rate,
        avg_interventions,
        tokens_per_accepted_task: (tolerant_successes > 0)
            .then(|| total_tokens / tolerant_successes as u64),
    }
}

/// Build an `OutcomeReport` from stored outcomes and followups, grouped by `group_by` dimensions
/// (e.g., `["category", "model", "policy"]`).
pub fn build_outcome_report(
    outcomes: &[TaskOutcome],
    followups: &[OutcomeFollowup],
    window_label: &str,
    group_by: &[String],
) -> OutcomeReport {
    let followup_map = group_followups(followups);
    let mut chat_turns_excluded = 0usize;
    let mut worker_turns_excluded = 0usize;
    let mut primary_non_chat: Vec<&TaskOutcome> = Vec::new();

    for o in outcomes {
        if o.role != TaskRole::Primary {
            worker_turns_excluded += 1;
            continue;
        }
        if o.category == TaskCategory::Chat {
            chat_turns_excluded += 1;
            continue;
        }
        primary_non_chat.push(o);
    }

    let overall = build_slice_report("overall".to_string(), &primary_non_chat, &followup_map);
    let singularity = singularity_status(&overall);
    let mut by_category: BTreeMap<TaskCategory, Vec<&TaskOutcome>> = BTreeMap::new();
    for o in &primary_non_chat {
        by_category.entry(o.category).or_default().push(*o);
    }
    let category_singularity: Vec<CategorySingularity> = by_category
        .into_iter()
        .map(|(category, items)| CategorySingularity {
            category: category.as_str().to_string(),
            status: singularity_status(&build_slice_report(
                category.as_str().to_string(),
                &items,
                &followup_map,
            )),
        })
        .collect();
    let shadow = shadow_summary(&primary_non_chat, &followup_map);
    let baseline = baseline_readiness(&primary_non_chat, &category_singularity, &shadow);

    let mut slices = Vec::new();
    if !group_by.is_empty() {
        let mut buckets: BTreeMap<String, Vec<&TaskOutcome>> = BTreeMap::new();
        for o in &primary_non_chat {
            let parts: Vec<String> = group_by
                .iter()
                .map(|dim| match dim.as_str() {
                    "category" => format!("category={}", o.category.as_str()),
                    "model" => format!("model={}", o.model),
                    "policy" | "permission_mode" => format!("policy={}", o.permission_mode),
                    "route" => format!(
                        "route={}",
                        o.route
                            .as_ref()
                            .map(|r| r.tier_config.as_str())
                            .unwrap_or("single")
                    ),
                    other => format!("{other}=unknown"),
                })
                .collect();
            buckets.entry(parts.join(",")).or_default().push(*o);
        }
        for (k, items) in buckets {
            slices.push(build_slice_report(k, &items, &followup_map));
        }
    }

    OutcomeReport {
        window_label: window_label.to_string(),
        total_events_scanned: outcomes.len(),
        chat_turns_excluded,
        worker_turns_excluded,
        overall,
        singularity,
        category_singularity,
        shadow,
        baseline,
        slices,
    }
}

/// Format an `OutcomeReport` for human CLI display.
pub fn format_outcome_report(report: &OutcomeReport) -> String {
    let mut lines = Vec::new();
    let o = &report.overall;
    lines.push(format!(
        "=== BugleCat ACR & Singularity Report (window: {}) ===",
        report.window_label
    ));
    lines.push(format!(
        "Tasks evaluated: {} (pending, no followup yet: {}; excluded: {} chat, {} worker/subagent)",
        o.sample_size, o.pending_count, report.chat_turns_excluded, report.worker_turns_excluded
    ));
    lines.push(format!(
        "ACR-strict:            {:>5.1}%  [95% Wilson CI: {:>5.1}% .. {:>5.1}%] ({}/{}) status={}",
        o.acr_strict * 100.0,
        o.acr_strict_ci.lower * 100.0,
        o.acr_strict_ci.upper * 100.0,
        o.strict_successes,
        o.sample_size,
        o.status
    ));
    lines.push(format!(
        "ACR-approval-tolerant: {:>5.1}%  [95% Wilson CI: {:>5.1}% .. {:>5.1}%] ({}/{})",
        o.acr_tolerant * 100.0,
        o.acr_tolerant_ci.lower * 100.0,
        o.acr_tolerant_ci.upper * 100.0,
        o.tolerant_successes,
        o.sample_size
    ));
    lines.push(format!(
        "Rework rate (I3-I5):   {:>5.1}%  (inferred I4 corrections: {})",
        o.rework_rate * 100.0,
        o.inferred_correction_count
    ));
    lines.push(format!(
        "First-pass verify:     {:>5.1}%  | Avg interventions/task: {:.2} | Tokens/accepted: {}",
        o.first_pass_verify_rate * 100.0,
        o.avg_interventions,
        o.tokens_per_accepted_task
            .map(|v| v.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    ));
    let badge = |s: &SingularityStatus| {
        if s.reached {
            "REACHED"
        } else if s.sample_size < s.min_required_sample {
            "INSUFFICIENT SAMPLE"
        } else {
            "IN PROGRESS"
        }
    };
    let s = &report.singularity;
    lines.push(format!(
        "60-Point Singularity (overall, reference): {} | n={} lower_bound={:.1}% (Δ {:+.1}% vs 60.0%), rework={:.1}% (max 15.0%)",
        badge(s),
        s.sample_size,
        s.acr_strict_lower_bound * 100.0,
        s.distance_to_singularity * 100.0,
        s.rework_rate * 100.0
    ));
    if !report.category_singularity.is_empty() {
        lines.push(
            "\n--- Singularity by category (n >= 30, lower bound >= 60%, rework <= 15%) ---"
                .to_string(),
        );
        for c in &report.category_singularity {
            let s = &c.status;
            lines.push(format!(
                "  {:<10} {:<19} n={:<4} lower_bound={:>5.1}% (Δ {:+.1}%) rework={:>5.1}%",
                c.category,
                badge(s),
                s.sample_size,
                s.acr_strict_lower_bound * 100.0,
                s.distance_to_singularity * 100.0,
                s.rework_rate * 100.0
            ));
        }
    }
    let sh = &report.shadow;
    lines.push("\n--- Shadow feedback (what `on` would change) ---".to_string());
    lines.push(format!(
        "  memory:  weighted recall differs on {}/{} tasks",
        sh.memory_rank_changed, sh.memory_observed
    ));
    lines.push(format!(
        "  routing: upgrade recommended on {}/{} orchestrated tasks; current route strict ACR on those = {}/{}",
        sh.routing_would_upgrade,
        sh.routing_observed,
        sh.routing_would_upgrade_strict_successes,
        sh.routing_would_upgrade_determined
    ));
    let b = &report.baseline;
    let verdict = |ready: bool| {
        if ready {
            "may consider `on`"
        } else {
            "keep shadow"
        }
    };
    lines.push(format!(
        "  baseline: {} day(s); ready categories: [{}]; memory -> {}; routing -> {}",
        b.days_observed,
        b.categories_ready.join(", "),
        verdict(b.memory_ready),
        verdict(b.routing_ready)
    ));
    for blocker in &b.blockers {
        lines.push(format!("    - {blocker}"));
    }
    if !report.slices.is_empty() {
        lines.push("\n--- Slices ---".to_string());
        for sl in &report.slices {
            lines.push(format!(
                "  {:<36} n={:<4} status={:<19} ACR-strict={:>5.1}% [{:>5.1}%..{:>5.1}%] rework={:>5.1}% (inferred={})",
                sl.key,
                sl.sample_size,
                sl.status,
                sl.acr_strict * 100.0,
                sl.acr_strict_ci.lower * 100.0,
                sl.acr_strict_ci.upper * 100.0,
                sl.rework_rate * 100.0,
                sl.inferred_correction_count
            ));
        }
    }
    lines.join("\n")
}

/// Parse a human duration string like `"30d"`, `"7d"`, `"24h"`, `"60m"` into milliseconds.
pub fn parse_since_duration_ms(spec: &str) -> Result<u64, String> {
    let s = spec.trim().to_lowercase();
    if s.is_empty() {
        return Err("empty duration".to_string());
    }
    let (num_str, multiplier_ms) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1u64)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1_000u64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60 * 1_000u64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600 * 1_000u64)
    } else if let Some(n) = s.strip_suffix('d') {
        (n, 86_400 * 1_000u64)
    } else {
        (s.as_str(), 86_400 * 1_000u64)
    };
    let count = num_str
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid duration '{spec}' (expected e.g. 30d, 7d, 24h)"))?;
    Ok(count.saturating_mul(multiplier_ms))
}

fn contains_term_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| contains_term(haystack, n))
}

/// Inflection suffixes accepted after an ASCII keyword ("fix" matches "fixed",
/// "fixes", "fixing" but not "fixture" or "prefix").
const TERM_SUFFIXES: &[&str] = &["", "s", "es", "d", "ed", "ing", "er", "ers"];

/// ASCII keywords match on word boundaries (with common inflections) so "ci" does
/// not fire on "decide" and "test" not on "latest". Non-ASCII keywords (CJK)
/// have no word separators and keep plain substring matching.
fn contains_term(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    if !needle.is_ascii() {
        return haystack.contains(needle);
    }
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let left_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        let tail: String = haystack[end..]
            .chars()
            .take_while(|c| is_word(*c))
            .collect();
        if left_ok && TERM_SUFFIXES.contains(&tail.as_str()) {
            return true;
        }
        from = start + needle.chars().next().map_or(1, char::len_utf8);
    }
    false
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "outcome/tests.rs"]
mod tests;
