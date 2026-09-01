//! Versioned client/server contracts for nanocodex threads and turns.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 3;

fn default_harness_profile() -> String {
    "full".to_string()
}

macro_rules! durable_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ProtocolError::InvalidId($label));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

durable_id!(ThreadId, "threadId");
durable_id!(TurnId, "turnId");
durable_id!(ItemId, "itemId");
durable_id!(GoalId, "goalId");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoalPhase {
    Active,
    Paused,
    Blocked,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalBlockReason {
    pub code: String,
    pub message: String,
}

/// Durable goal state. Automatic scheduling authority is deliberately absent:
/// `armed` / `disarmed` belongs to the live App Server runtime and is reset on
/// every lifecycle replacement or restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSnapshot {
    pub id: GoalId,
    pub revision: u64,
    pub objective: String,
    pub phase: GoalPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<GoalBlockReason>,
    pub max_goal_rounds: u32,
    pub rounds_started: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalRef {
    pub id: GoalId,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoalActivation {
    Armed,
    Disarmed,
}

/// Runtime projection combining durable goal state with process-local
/// continuation authority. Only `goal` is persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalView {
    pub goal: GoalSnapshot,
    pub activation: GoalActivation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMetadata {
    pub id: ThreadId,
    pub workspace: String,
    pub title: String,
    pub archived: bool,
    #[serde(default = "default_harness_profile")]
    pub harness_profile: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ThreadItem {
    UserMessage {
        id: ItemId,
        text: String,
    },
    /// Synthetic continuation prompt admitted by the Goal round driver. It is
    /// retained for model replay and audit but excluded from visible history.
    GoalMessage {
        id: ItemId,
        text: String,
        goal_id: GoalId,
        revision: u64,
        round: u32,
    },
    AssistantMessage {
        id: ItemId,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirmed_model: Option<String>,
    },
    Reasoning {
        id: ItemId,
        summary: String,
    },
    ToolCall {
        id: ItemId,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: ItemId,
        call_id: ItemId,
        output: String,
        success: bool,
    },
    Artifact {
        id: ItemId,
        kind: String,
        name: String,
        url: String,
    },
    ContextCompaction {
        id: ItemId,
        summary: String,
        dropped_items: u32,
    },
}

impl ThreadItem {
    pub fn id(&self) -> &ItemId {
        match self {
            Self::UserMessage { id, .. }
            | Self::GoalMessage { id, .. }
            | Self::AssistantMessage { id, .. }
            | Self::Reasoning { id, .. }
            | Self::ToolCall { id, .. }
            | Self::ToolResult { id, .. }
            | Self::Artifact { id, .. }
            | Self::ContextCompaction { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    #[default]
    Agent,
    Orchestrator,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    #[serde(default)]
    pub tokens: BTreeMap<String, i64>,
    #[serde(default)]
    pub estimated_cost: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: TurnId,
    pub status: TurnStatus,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    pub items: Vec<ThreadItem>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    #[serde(default)]
    pub usage: TurnUsage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub metadata: ThreadMetadata,
    pub turns: Vec<Turn>,
}

impl Thread {
    /// Project a durable thread to the transcript safe for history clients.
    /// Every turn retains its user request and only its final assistant answer;
    /// tool traffic, reasoning, compaction details, and intermediate answers are removed.
    pub fn into_visible(mut self) -> Self {
        for turn in &mut self.turns {
            let mut visible = turn
                .items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        ThreadItem::UserMessage { .. } | ThreadItem::Artifact { .. }
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            if let Some(answer) = turn
                .items
                .iter()
                .rev()
                .find(|item| matches!(item, ThreadItem::AssistantMessage { .. }))
                .cloned()
            {
                visible.push(answer);
            }
            turn.items = visible;
        }
        self
    }
}

/// Provider-facing conversation state kept separately from the user-visible
/// Thread/Turn transcript. Compaction may replace this value without deleting
/// durable user messages or final answers from the transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredModelContext {
    pub thread_id: ThreadId,
    pub messages: Vec<Value>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientRequest {
    ThreadCreate {
        thread_id: Option<ThreadId>,
        workspace: String,
        title: String,
        #[serde(default = "default_harness_profile")]
        harness_profile: String,
    },
    ThreadCreateActivate {
        thread_id: ThreadId,
        workspace: String,
        title: String,
        #[serde(default = "default_harness_profile")]
        harness_profile: String,
    },
    ThreadImport {
        thread: Thread,
    },
    ThreadsImport {
        threads: Vec<Thread>,
    },
    ThreadList {
        include_archived: bool,
    },
    ThreadRead {
        thread_id: ThreadId,
    },
    ThreadReadVisible {
        thread_id: ThreadId,
    },
    ThreadModelContextRead {
        thread_id: ThreadId,
    },
    ThreadModelContextReplace {
        thread_id: ThreadId,
        messages: Vec<Value>,
    },
    ThreadArchive {
        thread_id: ThreadId,
        archived: bool,
    },
    ThreadRename {
        thread_id: ThreadId,
        title: String,
    },
    ThreadHarnessProfileSet {
        thread_id: ThreadId,
        harness_profile: String,
    },
    GoalRead {
        thread_id: ThreadId,
    },
    GoalCreate {
        thread_id: ThreadId,
        objective: String,
        max_goal_rounds: u32,
    },
    GoalEdit {
        thread_id: ThreadId,
        goal: GoalRef,
        objective: String,
        max_goal_rounds: u32,
    },
    GoalPause {
        thread_id: ThreadId,
        goal: GoalRef,
    },
    GoalResume {
        thread_id: ThreadId,
        goal: GoalRef,
    },
    GoalBlock {
        thread_id: ThreadId,
        goal: GoalRef,
        reason: GoalBlockReason,
    },
    GoalComplete {
        thread_id: ThreadId,
        goal: GoalRef,
    },
    GoalClear {
        thread_id: ThreadId,
        goal: GoalRef,
    },
    GoalRoundStart {
        thread_id: ThreadId,
        turn_id: TurnId,
        goal: GoalRef,
        round: u32,
        prompt: String,
    },
    ThreadFork {
        thread_id: ThreadId,
        new_thread_id: ThreadId,
    },
    ThreadForkActivate {
        thread_id: ThreadId,
        new_thread_id: ThreadId,
    },
    ThreadActivate {
        thread_id: ThreadId,
    },
    TurnStart {
        thread_id: ThreadId,
        turn_id: TurnId,
        #[serde(default)]
        execution_mode: ExecutionMode,
    },
    TurnSubmit {
        thread_id: ThreadId,
        text: String,
        #[serde(default)]
        images: Vec<String>,
        #[serde(default)]
        execution_mode: ExecutionMode,
    },
    TurnInterrupt {
        thread_id: ThreadId,
        turn_id: TurnId,
    },
    TurnInterruptLatest {
        thread_id: ThreadId,
    },
    RuntimeStatusRead,
    RuntimeReadyRefresh,
    WorkspaceSet {
        path: String,
    },
    InteractionApprove {
        thread_id: Option<ThreadId>,
        id: u64,
        decision: String,
    },
    InteractionAnswer {
        thread_id: Option<ThreadId>,
        id: u64,
        answer: Option<String>,
    },
    SettingsRead,
    SettingsUpdate {
        updates: BTreeMap<String, String>,
    },
    RuntimeModelSet {
        model: String,
    },
    /// Rebuild the exact durable Thread after changing the process-wide
    /// permission policy. The Thread identity prevents a queued request from
    /// rebuilding whichever session happens to become active meanwhile.
    RuntimePermissionModeSet {
        thread_id: ThreadId,
        mode: String,
    },
    ModelCatalogRead,
    ModelPresetApply {
        provider_id: String,
        model_id: String,
    },
    CustomProviderList,
    CustomProviderSave {
        id: Option<String>,
        name: String,
        protocol: String,
        base_url: String,
        api_key: Option<String>,
        models: Vec<String>,
    },
    CustomProviderDelete {
        id: String,
    },
    CustomProviderModelsDiscover {
        id: String,
    },
    CustomProviderActivate {
        id: String,
        model: String,
    },
    CustomProviderChatProbe {
        id: String,
        model: String,
    },
    HarnessDiagnosticsRead,
    ExternalPluginList,
    ExternalPluginInstall {
        source: String,
        upgrade: bool,
    },
    ExternalPluginSetEnabled {
        id: String,
        enabled: bool,
    },
    /// List project-memory notes for the caller's workspace snapshot.
    /// Runtime adapters must reject the request if the process workspace has
    /// changed before the read executes.
    MemoryList {
        workspace: String,
    },
    MemoryAdd {
        note: String,
        tags: Vec<String>,
        workspace: String,
    },
    MemoryConsolidate {
        workspace: String,
    },
    /// Start a model-memory merge only if the runtime still owns the caller's
    /// workspace, preventing queued work from mutating a newly selected project.
    MemoryMergeStart {
        workspace: String,
    },
    /// Read the model-memory merge projection for a workspace snapshot.
    /// `generation` is omitted for an initial refresh and supplied by a
    /// poller to avoid observing a replacement job after its own job ended.
    MemoryMergeStatusRead {
        workspace: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        generation: Option<u64>,
    },
    /// Cancel exactly the merge generation owned by this workspace snapshot.
    MemoryMergeCancel {
        workspace: String,
        generation: u64,
    },
    ForgeRuntimeStatusRead,
    ForgeJobStart {
        /// Runtime adapters must verify this caller snapshot before spawning,
        /// so queued work cannot write into a newly selected project.
        workspace: String,
        rounds: u8,
        repeats: u8,
        timeout_s: u64,
        budget_s: u64,
        teacher: String,
        accept_margin: u8,
    },
    /// Read a workspace snapshot; pollers provide their started generation.
    ForgeJobStatusRead {
        workspace: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        generation: Option<u64>,
    },
    /// Cancel exactly the Forge generation owned by this workspace snapshot.
    ForgeJobCancel {
        workspace: String,
        generation: u64,
    },
    TurnComplete {
        thread_id: ThreadId,
        turn_id: TurnId,
        status: TurnStatus,
        error: Option<String>,
        #[serde(default)]
        usage: TurnUsage,
    },
    ItemAppend {
        thread_id: ThreadId,
        turn_id: TurnId,
        item: ThreadItem,
    },
    CodexPluginList,
    CodexPluginInstall {
        source: String,
        upgrade: bool,
    },
    CodexPluginSetEnabled {
        name: String,
        enabled: bool,
    },
    CodexPluginUninstall {
        name: String,
    },
    MarketplaceList,
    MarketplacePluginInstall {
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    },
    DshMarketplaceSearch {
        source: String,
        manifest_url: Option<String>,
        query: String,
    },
    DshMarketplacePreview {
        item: Value,
    },
    DshMarketplaceInstall {
        item: Value,
        upgrade: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResponsePayload {
    Ack,
    Thread(Thread),
    Threads(Vec<ThreadMetadata>),
    ModelContext(Option<StoredModelContext>),
    Goal(Option<GoalView>),
    RuntimeStatus(Value),
    Workspace(String),
    Settings(Value),
    ModelCatalog(Value),
    ModelPreset(Value),
    CustomProviders(Value),
    CustomProvider(Value),
    ProviderChatProbe(Value),
    Models(Vec<String>),
    HarnessDiagnostics(Value),
    ExternalPlugins(Value),
    ExternalPlugin(Value),
    MemoryNotes(Value),
    MemoryMergeOperation(Value),
    ForgeRuntime(Value),
    ForgeJob(Value),
    Count(u64),
    Bool(bool),
    CodexPlugins(Value),
    CodexPlugin(Value),
    Marketplaces(Value),
    DshMarketplace(Value),
    DshMarketplacePreview(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerResponse {
    pub protocol_version: u32,
    pub payload: ResponsePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    ThreadCreated {
        metadata: ThreadMetadata,
    },
    ThreadUpdated {
        metadata: ThreadMetadata,
    },
    TurnStarted {
        status: TurnStatus,
    },
    TurnCompleted {
        status: TurnStatus,
        error: Option<String>,
    },
    ItemAdded {
        item: ThreadItem,
    },
    ModelContextUpdated {
        message_count: usize,
    },
    GoalChanged {
        goal: Option<GoalView>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub protocol_version: u32,
    pub sequence: u64,
    pub thread_id: ThreadId,
    pub turn_id: Option<TurnId>,
    pub event: Event,
}

impl EventEnvelope {
    pub fn new(sequence: u64, thread_id: ThreadId, turn_id: Option<TurnId>, event: Event) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            sequence,
            thread_id,
            turn_id,
            event,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidId(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(name) => write!(formatter, "{name} must not be empty"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
