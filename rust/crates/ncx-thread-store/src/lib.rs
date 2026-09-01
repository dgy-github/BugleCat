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
    fn list(&self, include_archived: bool) -> Result<Vec<ThreadMetadata>, ThreadStoreError>;
    fn read(&self, id: &ThreadId) -> Result<Option<Thread>, ThreadStoreError>;
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
            state.persisted.threads.insert(id, thread);
            Ok(())
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
                state
                    .persisted
                    .threads
                    .insert(thread.metadata.id.as_str().to_string(), thread);
            }
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
            match replacement {
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
            }
        })
    }

    fn update_metadata(&self, metadata: ThreadMetadata) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            let id = metadata.id.as_str();
            let thread = state
                .persisted
                .threads
                .get_mut(id)
                .ok_or_else(|| ThreadStoreError::NotFound(id.to_string()))?;
            thread.metadata = metadata;
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
            Ok(Some(thread.metadata.clone()))
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
            Ok(goal)
        })
    }

    fn fork(&self, source: &ThreadId, target: ThreadId) -> Result<Thread, ThreadStoreError> {
        self.mutate(|state| {
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
            if let Some(source_context) =
                state.persisted.model_contexts.get(source.as_str()).cloned()
            {
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
            Ok(forked)
        })
    }

    fn claim_turn(&self, thread: &ThreadId, turn: Turn) -> Result<(), ThreadStoreError> {
        let store_path = self.path.clone();
        self.mutate(|state| claim_turn_in_state(state, &store_path, thread, turn))
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
