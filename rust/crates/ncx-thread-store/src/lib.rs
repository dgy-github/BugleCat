//! Storage-neutral thread persistence with explicit per-thread turn ownership.

mod storage;

use ncx_protocol::{
    GoalRef, GoalSnapshot, StoredModelContext, Thread, ThreadId, ThreadItem, ThreadMetadata, Turn,
    TurnId, TurnStatus, TurnUsage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use storage::{
    acquire_global_lock, claim_turn_in_state, find_turn_mut, load_state, recover_orphaned_turns,
    require_owner, save_state,
};

pub trait ThreadStore: Send + Sync {
    fn create(&self, thread: Thread) -> Result<(), ThreadStoreError>;
    fn create_many(&self, threads: Vec<Thread>) -> Result<(), ThreadStoreError>;
    /// Persist a newly provisioned Thread and return a compare-and-delete
    /// receipt in the same store transaction. Runtime hosts use this when a
    /// durable create is followed by host-side activation.
    fn create_with_rollback(
        &self,
        thread: Thread,
    ) -> Result<ThreadRollbackSnapshot, ThreadStoreError>;
    /// Fork and return the exact post-fork state that can be compensated if a
    /// host runtime rejects activation. `created_at`/`updated_at` are applied
    /// as part of the operation's durable metadata.
    fn fork_with_rollback(
        &self,
        source: &ThreadId,
        target: ThreadId,
        created_at: i64,
        updated_at: i64,
    ) -> Result<(Thread, ThreadRollbackSnapshot), ThreadStoreError>;
    /// Remove the target and all of its side domains only when it still
    /// exactly matches a snapshot captured immediately after provisioning.
    ///
    /// This is intentionally a compare-and-delete operation: it must never
    /// erase a Thread that has accepted a turn or was changed by another
    /// caller while a host runtime operation was in flight. `false` means the
    /// rollback fence did not match and the caller must retain the durable
    /// state rather than guessing.
    fn discard_if_unchanged(
        &self,
        snapshot: &ThreadRollbackSnapshot,
    ) -> Result<bool, ThreadStoreError>;
    /// Record that a host is about to activate an existing durable Thread.
    ///
    /// The marker is monotonic and deliberately remains after the host call.
    /// It lets a provisioning receipt created by another process detect that
    /// the target was handed to a runtime, even though runtime ownership itself
    /// is process-local and does not otherwise change durable Thread data.
    fn mark_runtime_activation(&self, id: &ThreadId) -> Result<(), ThreadStoreError>;
    fn list(&self, include_archived: bool) -> Result<Vec<ThreadMetadata>, ThreadStoreError>;
    fn read(&self, id: &ThreadId) -> Result<Option<Thread>, ThreadStoreError>;
    /// Read a Thread and its durable Goal from one persisted snapshot. A
    /// caller that projects process-local Goal authority must not combine two
    /// independent reads, because another process may replace the Goal
    /// between them.
    fn read_with_goal(
        &self,
        id: &ThreadId,
    ) -> Result<Option<(Thread, Option<GoalSnapshot>)>, ThreadStoreError>;
    fn read_model_context(
        &self,
        id: &ThreadId,
    ) -> Result<Option<StoredModelContext>, ThreadStoreError>;
    fn replace_model_context(
        &self,
        id: &ThreadId,
        messages: Vec<Value>,
        updated_at: i64,
    ) -> Result<StoredModelContext, ThreadStoreError>;
    fn read_goal(&self, id: &ThreadId) -> Result<Option<GoalSnapshot>, ThreadStoreError>;
    /// Atomically compare and replace the thread's durable goal under the same
    /// cross-process transaction used for all other thread mutations.
    fn compare_and_set_goal(
        &self,
        id: &ThreadId,
        expected: GoalExpectation,
        replacement: Option<GoalSnapshot>,
    ) -> Result<Option<GoalSnapshot>, ThreadStoreError>;
    /// Atomically admit one exact automatic Goal round together with its Turn,
    /// synthetic prompt, lease, and durable `roundsStarted` increment.
    fn claim_goal_round(
        &self,
        thread: &ThreadId,
        expected: GoalRef,
        round: u32,
        turn: Turn,
    ) -> Result<GoalSnapshot, ThreadStoreError>;
    fn update_metadata(&self, metadata: ThreadMetadata) -> Result<(), ThreadStoreError>;
    /// Atomically set a thread's Harness Profile only while it has no turns.
    ///
    /// The idle check and metadata write must share one store transaction so a
    /// concurrent first `TurnStart` cannot slip between a caller's read/check
    /// and the profile update. `None` means the thread exists but is already
    /// locked by a prior (or currently active) turn.
    fn set_harness_profile_if_idle(
        &self,
        id: &ThreadId,
        harness_profile: String,
        updated_at: i64,
    ) -> Result<Option<ThreadMetadata>, ThreadStoreError>;
    fn fork(&self, source: &ThreadId, target: ThreadId) -> Result<Thread, ThreadStoreError>;
    fn claim_turn(&self, thread: &ThreadId, turn: Turn) -> Result<(), ThreadStoreError>;
    fn append_item(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        item: ThreadItem,
        updated_at: i64,
    ) -> Result<(), ThreadStoreError>;
    fn finish_turn(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        status: TurnStatus,
        completed_at: i64,
        error: Option<String>,
        usage: TurnUsage,
    ) -> Result<(), ThreadStoreError>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    threads: BTreeMap<String, Thread>,
    #[serde(default)]
    model_contexts: BTreeMap<String, StoredModelContext>,
    #[serde(default)]
    goals: BTreeMap<String, GoalSnapshot>,
    /// Monotonic per-Thread write epochs used by host-runtime handoff
    /// receipts. The serialized name is retained for compatibility with
    /// existing stores, but every successful mutation advances the epoch so a
    /// compare-and-delete receipt is safe from ABA changes.
    #[serde(default)]
    runtime_activation_epochs: BTreeMap<String, u64>,
}

/// Opaque compare-and-delete receipt for a just-provisioned Thread.
///
/// Thread creation and GUI/CLI runtime activation span two different state
/// owners. Keeping every persistent domain in this receipt lets the store
/// prove that no later operation touched the target before it compensates a
/// rejected activation.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadRollbackSnapshot {
    thread: Thread,
    model_context: Option<StoredModelContext>,
    goal: Option<GoalSnapshot>,
    runtime_activation_epoch: u64,
}

impl ThreadRollbackSnapshot {
    pub fn thread_id(&self) -> &ThreadId {
        &self.thread.metadata.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalExpectation {
    Absent,
    Exact(GoalRef),
}

struct ActiveTurn {
    turn_id: TurnId,
    _lease: File,
}

#[derive(Default)]
struct StoreState {
    persisted: PersistedState,
    active_turns: HashMap<String, ActiveTurn>,
}

pub struct JsonThreadStore {
    path: PathBuf,
    state: Mutex<StoreState>,
}

impl JsonThreadStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ThreadStoreError> {
        let path = path.into();
        let _global = acquire_global_lock(&path)?;
        let mut persisted = load_state(&path)?;
        let recovered = recover_orphaned_turns(&path, &mut persisted)?;
        if recovered {
            save_state(&path, &persisted)?;
        }
        Ok(Self {
            path,
            state: Mutex::new(StoreState {
                persisted,
                active_turns: HashMap::new(),
            }),
        })
    }

    fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut StoreState) -> Result<T, ThreadStoreError>,
    ) -> Result<T, ThreadStoreError> {
        let mut state = self.state.lock().map_err(|_| ThreadStoreError::Poisoned)?;
        let _global = acquire_global_lock(&self.path)?;
        state.persisted = load_state(&self.path)?;
        if recover_orphaned_turns(&self.path, &mut state.persisted)? {
            save_state(&self.path, &state.persisted)?;
        }
        let result = operation(&mut state)?;
        save_state(&self.path, &state.persisted)?;
        Ok(result)
    }

    fn inspect<T>(
        &self,
        operation: impl FnOnce(&PersistedState) -> T,
    ) -> Result<T, ThreadStoreError> {
        let mut state = self.state.lock().map_err(|_| ThreadStoreError::Poisoned)?;
        let _global = acquire_global_lock(&self.path)?;
        state.persisted = load_state(&self.path)?;
        if recover_orphaned_turns(&self.path, &mut state.persisted)? {
            save_state(&self.path, &state.persisted)?;
        }
        Ok(operation(&state.persisted))
    }
}

fn snapshot_for_state(
    persisted: &PersistedState,
    id: &str,
) -> Result<ThreadRollbackSnapshot, ThreadStoreError> {
    let thread = persisted
        .threads
        .get(id)
        .cloned()
        .ok_or_else(|| ThreadStoreError::NotFound(id.to_string()))?;
    Ok(ThreadRollbackSnapshot {
        thread,
        model_context: persisted.model_contexts.get(id).cloned(),
        goal: persisted.goals.get(id).cloned(),
        runtime_activation_epoch: persisted
            .runtime_activation_epochs
            .get(id)
            .copied()
            .unwrap_or_default(),
    })
}

/// Advance the durable version for one Thread after a successful mutation.
///
/// The backing field retains its original `runtime_activation_epochs` name so
/// older processes preserve it when they rewrite the JSON file. Its purpose is
/// broader than runtime activation: a rollback receipt must reject a target
/// that was changed and later restored to byte-for-byte identical state.
fn advance_thread_write_epoch(persisted: &mut PersistedState, id: &str) {
    let epoch = persisted
        .runtime_activation_epochs
        .entry(id.to_string())
        .or_default();
    *epoch = epoch.saturating_add(1);
}

fn fork_into_state(
    state: &mut StoreState,
    source: &ThreadId,
    target: ThreadId,
    timestamps: Option<(i64, i64)>,
) -> Result<Thread, ThreadStoreError> {
    if state.persisted.threads.contains_key(target.as_str()) {
        return Err(ThreadStoreError::AlreadyExists(target.to_string()));
    }
    let mut forked = state
        .persisted
        .threads
        .get(source.as_str())
        .cloned()
        .ok_or_else(|| ThreadStoreError::NotFound(source.to_string()))?;
    forked.metadata.id = target.clone();
    forked.metadata.archived = false;
    if let Some((created_at, updated_at)) = timestamps {
        forked.metadata.created_at = created_at;
        forked.metadata.updated_at = updated_at;
    }
    for turn in &mut forked.turns {
        if turn.status == TurnStatus::Running {
            turn.status = TurnStatus::Cancelled;
            turn.error = Some("forked while source turn was still running".to_string());
        }
    }
    state
        .persisted
        .threads
        .insert(target.as_str().to_string(), forked.clone());
    if let Some(source_context) = state.persisted.model_contexts.get(source.as_str()).cloned() {
        state.persisted.model_contexts.insert(
            target.as_str().to_string(),
            StoredModelContext {
                thread_id: target.clone(),
                ..source_context
            },
        );
    }
    if let Some(source_goal) = state.persisted.goals.get(source.as_str()).cloned() {
        state
            .persisted
            .goals
            .insert(target.as_str().to_string(), source_goal);
    }
    // Forking writes a new durable target, so its epoch must be initialized
    // even when the caller does not request a rollback receipt.
    advance_thread_write_epoch(&mut state.persisted, target.as_str());
    Ok(forked)
}

pub fn default_thread_store_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".nanocodex").join("threads-v2.json")
}

impl ThreadStore for JsonThreadStore {
    fn create(&self, thread: Thread) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            let id = thread.metadata.id.as_str().to_string();
            if state.persisted.threads.contains_key(&id) {
                return Err(ThreadStoreError::AlreadyExists(id));
            }
            state.persisted.threads.insert(id.clone(), thread);
            // A newly created Thread starts a fresh durable write history.
            // Keep that history even when no host handoff receipt is used so
            // every later compare-and-delete fence has a real version base.
            advance_thread_write_epoch(&mut state.persisted, &id);
            Ok(())
        })
    }

    fn create_with_rollback(
        &self,
        thread: Thread,
    ) -> Result<ThreadRollbackSnapshot, ThreadStoreError> {
        self.mutate(|state| {
            let id = thread.metadata.id.as_str().to_string();
            if state.persisted.threads.contains_key(&id) {
                return Err(ThreadStoreError::AlreadyExists(id));
            }
            state.persisted.threads.insert(id.clone(), thread);
            // Provisioning establishes the receipt's first write version.
            // Any later mutation or host activation advances it, preventing a
            // failed activation from deleting a Thread another caller used.
            advance_thread_write_epoch(&mut state.persisted, &id);
            snapshot_for_state(&state.persisted, &id)
        })
    }

    fn create_many(&self, threads: Vec<Thread>) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            let mut incoming = std::collections::HashSet::new();
            for thread in &threads {
                let id = thread.metadata.id.as_str();
                if state.persisted.threads.contains_key(id) || !incoming.insert(id.to_string()) {
                    return Err(ThreadStoreError::AlreadyExists(id.to_string()));
                }
            }
            for thread in threads {
                let id = thread.metadata.id.as_str().to_string();
                state.persisted.threads.insert(id.clone(), thread);
                advance_thread_write_epoch(&mut state.persisted, &id);
            }
            Ok(())
        })
    }

    fn discard_if_unchanged(
        &self,
        snapshot: &ThreadRollbackSnapshot,
    ) -> Result<bool, ThreadStoreError> {
        self.mutate(|state| {
            let id = snapshot.thread_id().as_str();
            if state.active_turns.contains_key(id)
                || state.persisted.threads.get(id) != Some(&snapshot.thread)
                || state.persisted.model_contexts.get(id) != snapshot.model_context.as_ref()
                || state.persisted.goals.get(id) != snapshot.goal.as_ref()
                || state
                    .persisted
                    .runtime_activation_epochs
                    .get(id)
                    .copied()
                    .unwrap_or_default()
                    != snapshot.runtime_activation_epoch
            {
                return Ok(false);
            }
            state.persisted.threads.remove(id);
            state.persisted.model_contexts.remove(id);
            state.persisted.goals.remove(id);
            // Keep the epoch as a tombstone. If an old receipt is retried
            // after this ID is provisioned again, creation advances this
            // value and the old receipt cannot delete the new Thread.
            Ok(true)
        })
    }

    fn mark_runtime_activation(&self, id: &ThreadId) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            if !state.persisted.threads.contains_key(id.as_str()) {
                return Err(ThreadStoreError::NotFound(id.to_string()));
            }
            advance_thread_write_epoch(&mut state.persisted, id.as_str());
            Ok(())
        })
    }

    fn list(&self, include_archived: bool) -> Result<Vec<ThreadMetadata>, ThreadStoreError> {
        self.inspect(|persisted| {
            let mut rows = persisted
                .threads
                .values()
                .map(|thread| thread.metadata.clone())
                .filter(|metadata| include_archived || !metadata.archived)
                .collect::<Vec<_>>();
            rows.sort_by_key(|metadata| std::cmp::Reverse(metadata.updated_at));
            rows
        })
    }

    fn read(&self, id: &ThreadId) -> Result<Option<Thread>, ThreadStoreError> {
        self.inspect(|persisted| persisted.threads.get(id.as_str()).cloned())
    }

    fn read_with_goal(
        &self,
        id: &ThreadId,
    ) -> Result<Option<(Thread, Option<GoalSnapshot>)>, ThreadStoreError> {
        self.inspect(|persisted| {
            persisted
                .threads
                .get(id.as_str())
                .cloned()
                .map(|thread| (thread, persisted.goals.get(id.as_str()).cloned()))
        })
    }

    fn read_model_context(
        &self,
        id: &ThreadId,
    ) -> Result<Option<StoredModelContext>, ThreadStoreError> {
        self.inspect(|persisted| persisted.model_contexts.get(id.as_str()).cloned())
    }

    fn replace_model_context(
        &self,
        id: &ThreadId,
        messages: Vec<Value>,
        updated_at: i64,
    ) -> Result<StoredModelContext, ThreadStoreError> {
        self.mutate(|state| {
            if !state.persisted.threads.contains_key(id.as_str()) {
                return Err(ThreadStoreError::NotFound(id.to_string()));
            }
            let context = StoredModelContext {
                thread_id: id.clone(),
                messages,
                updated_at,
            };
            state
                .persisted
                .model_contexts
                .insert(id.as_str().to_string(), context.clone());
            advance_thread_write_epoch(&mut state.persisted, id.as_str());
            Ok(context)
        })
    }

    fn read_goal(&self, id: &ThreadId) -> Result<Option<GoalSnapshot>, ThreadStoreError> {
        self.inspect(|persisted| persisted.goals.get(id.as_str()).cloned())
    }

    fn compare_and_set_goal(
        &self,
        id: &ThreadId,
        expected: GoalExpectation,
        replacement: Option<GoalSnapshot>,
    ) -> Result<Option<GoalSnapshot>, ThreadStoreError> {
        self.mutate(|state| {
            if !state.persisted.threads.contains_key(id.as_str()) {
                return Err(ThreadStoreError::NotFound(id.to_string()));
            }
            let current = state.persisted.goals.get(id.as_str());
            let matches = match (&expected, current) {
                (GoalExpectation::Absent, None) => true,
                (GoalExpectation::Exact(expected), Some(actual)) => {
                    expected.id == actual.id && expected.revision == actual.revision
                }
                _ => false,
            };
            if !matches {
                return Err(ThreadStoreError::StaleGoal {
                    expected,
                    actual: current.map(|goal| GoalRef {
                        id: goal.id.clone(),
                        revision: goal.revision,
                    }),
                });
            }
            let result = match replacement {
                Some(goal) => {
                    state
                        .persisted
                        .goals
                        .insert(id.as_str().to_string(), goal.clone());
                    Ok(Some(goal))
                }
                None => {
                    state.persisted.goals.remove(id.as_str());
                    Ok(None)
                }
            };
            // A successful compare-and-set is a write event even when the
            // replacement is equal to the current value (or both are absent).
            // This closes the ABA gap for rollback receipts.
            advance_thread_write_epoch(&mut state.persisted, id.as_str());
            result
        })
    }

    fn update_metadata(&self, metadata: ThreadMetadata) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            let id = metadata.id.as_str().to_string();
            {
                let thread = state
                    .persisted
                    .threads
                    .get_mut(&id)
                    .ok_or_else(|| ThreadStoreError::NotFound(id.to_string()))?;
                thread.metadata = metadata;
            }
            advance_thread_write_epoch(&mut state.persisted, &id);
            Ok(())
        })
    }

    fn set_harness_profile_if_idle(
        &self,
        id: &ThreadId,
        harness_profile: String,
        updated_at: i64,
    ) -> Result<Option<ThreadMetadata>, ThreadStoreError> {
        self.mutate(|state| {
            let key = id.as_str();
            if !state.persisted.threads.contains_key(key) {
                return Err(ThreadStoreError::NotFound(id.to_string()));
            }
            // Check both durable history and this process' active-turn owner.
            // The global transaction lock also makes this decision atomic with
            // a concurrent TurnStart from another process.
            if state.active_turns.contains_key(key) {
                return Ok(None);
            }
            let thread = state
                .persisted
                .threads
                .get_mut(key)
                .expect("thread existence was checked above");
            if !thread.turns.is_empty() {
                return Ok(None);
            }
            thread.metadata.harness_profile = harness_profile;
            thread.metadata.updated_at = updated_at;
            let metadata = thread.metadata.clone();
            advance_thread_write_epoch(&mut state.persisted, key);
            Ok(Some(metadata))
        })
    }

    fn claim_goal_round(
        &self,
        thread: &ThreadId,
        expected: GoalRef,
        round: u32,
        turn: Turn,
    ) -> Result<GoalSnapshot, ThreadStoreError> {
        let store_path = self.path.clone();
        self.mutate(|state| {
            let current = state.persisted.goals.get(thread.as_str()).cloned();
            let Some(mut goal) = current else {
                return Err(ThreadStoreError::StaleGoal {
                    expected: GoalExpectation::Exact(expected),
                    actual: None,
                });
            };
            if goal.id != expected.id || goal.revision != expected.revision {
                return Err(ThreadStoreError::StaleGoal {
                    expected: GoalExpectation::Exact(expected),
                    actual: Some(GoalRef {
                        id: goal.id,
                        revision: goal.revision,
                    }),
                });
            }
            if goal.phase != ncx_protocol::GoalPhase::Active {
                return Err(ThreadStoreError::InvalidGoalRound(
                    "goal must be active before admitting a round".into(),
                ));
            }
            if round == 0 || round != goal.rounds_started.saturating_add(1) {
                return Err(ThreadStoreError::InvalidGoalRound(format!(
                    "round {round} is not the next round after {}",
                    goal.rounds_started
                )));
            }
            if round > goal.max_goal_rounds {
                return Err(ThreadStoreError::InvalidGoalRound(format!(
                    "round {round} exceeds the configured limit {}",
                    goal.max_goal_rounds
                )));
            }
            let valid_prompt = matches!(
                turn.items.as_slice(),
                [ThreadItem::GoalMessage {
                    goal_id,
                    revision,
                    round: item_round,
                    text,
                    ..
                }] if goal_id == &goal.id
                    && *revision == goal.revision
                    && *item_round == round
                    && !text.trim().is_empty()
            );
            if turn.status != TurnStatus::Running || !valid_prompt {
                return Err(ThreadStoreError::InvalidGoalRound(
                    "goal round turn must start running with one matching non-empty goal message"
                        .into(),
                ));
            }
            let started_at = turn.started_at;
            claim_turn_in_state(state, &store_path, thread, turn)?;
            goal.rounds_started = round;
            goal.updated_at = started_at.max(goal.updated_at);
            state
                .persisted
                .goals
                .insert(thread.as_str().to_string(), goal.clone());
            advance_thread_write_epoch(&mut state.persisted, thread.as_str());
            Ok(goal)
        })
    }

    fn fork(&self, source: &ThreadId, target: ThreadId) -> Result<Thread, ThreadStoreError> {
        self.mutate(|state| fork_into_state(state, source, target, None))
    }

    fn fork_with_rollback(
        &self,
        source: &ThreadId,
        target: ThreadId,
        created_at: i64,
        updated_at: i64,
    ) -> Result<(Thread, ThreadRollbackSnapshot), ThreadStoreError> {
        self.mutate(|state| {
            let forked = fork_into_state(
                state,
                source,
                target.clone(),
                Some((created_at, updated_at)),
            )?;
            let receipt = snapshot_for_state(&state.persisted, target.as_str())?;
            Ok((forked, receipt))
        })
    }

    fn claim_turn(&self, thread: &ThreadId, turn: Turn) -> Result<(), ThreadStoreError> {
        let store_path = self.path.clone();
        self.mutate(|state| {
            claim_turn_in_state(state, &store_path, thread, turn)?;
            advance_thread_write_epoch(&mut state.persisted, thread.as_str());
            Ok(())
        })
    }

    fn append_item(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        item: ThreadItem,
        updated_at: i64,
    ) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            require_owner(state, thread, turn)?;
            let stored_turn = find_turn_mut(state, thread, turn)?;
            if stored_turn
                .items
                .iter()
                .any(|current| current.id() == item.id())
            {
                return Err(ThreadStoreError::AlreadyExists(item.id().to_string()));
            }
            stored_turn.items.push(item);
            state
                .persisted
                .threads
                .get_mut(thread.as_str())
                .expect("owned turn must belong to a stored thread")
                .metadata
                .updated_at = updated_at;
            advance_thread_write_epoch(&mut state.persisted, thread.as_str());
            Ok(())
        })
    }

    fn finish_turn(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        status: TurnStatus,
        completed_at: i64,
        error: Option<String>,
        usage: TurnUsage,
    ) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            require_owner(state, thread, turn)?;
            let stored_turn = find_turn_mut(state, thread, turn)?;
            stored_turn.status = status;
            stored_turn.completed_at = Some(completed_at);
            stored_turn.error = error;
            stored_turn.usage = usage;
            state
                .persisted
                .threads
                .get_mut(thread.as_str())
                .expect("owned turn must belong to a stored thread")
                .metadata
                .updated_at = completed_at;
            state.active_turns.remove(thread.as_str());
            advance_thread_write_epoch(&mut state.persisted, thread.as_str());
            Ok(())
        })
    }
}

#[derive(Debug)]
pub enum ThreadStoreError {
    AlreadyExists(String),
    NotFound(String),
    Busy {
        thread: String,
        turn: TurnId,
    },
    TurnNotActive(String),
    LeaseBusy(String),
    StaleGoal {
        expected: GoalExpectation,
        actual: Option<GoalRef>,
    },
    InvalidGoalRound(String),
    Poisoned,
    Io(std::io::Error),
    Decode(serde_json::Error),
    Encode(serde_json::Error),
}

impl fmt::Display for ThreadStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(id) => write!(formatter, "{id} already exists"),
            Self::NotFound(id) => write!(formatter, "{id} was not found"),
            Self::Busy { thread, turn } => write!(formatter, "thread {thread} is owned by {turn}"),
            Self::TurnNotActive(id) => write!(formatter, "turn {id} is not active"),
            Self::LeaseBusy(id) => write!(formatter, "thread {id} is owned by another process"),
            Self::StaleGoal { expected, actual } => {
                write!(
                    formatter,
                    "stale goal revision: expected {expected:?}, actual {actual:?}"
                )
            }
            Self::InvalidGoalRound(message) => message.fmt(formatter),
            Self::Poisoned => write!(formatter, "thread store lock is poisoned"),
            Self::Io(error) => error.fmt(formatter),
            Self::Decode(error) | Self::Encode(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ThreadStoreError {}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
