//! Storage-neutral thread persistence with explicit per-thread turn ownership.

use fs2::FileExt;
use ncx_protocol::{
    GoalRef, GoalSnapshot, StoredModelContext, Thread, ThreadId, ThreadItem, ThreadMetadata, Turn,
    TurnId, TurnStatus, TurnUsage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

fn claim_turn_in_state(
    state: &mut StoreState,
    store_path: &Path,
    thread: &ThreadId,
    turn: Turn,
) -> Result<(), ThreadStoreError> {
    let key = thread.as_str().to_string();
    if let Some(owner) = state.active_turns.get(&key) {
        return Err(ThreadStoreError::Busy {
            thread: key,
            turn: owner.turn_id.clone(),
        });
    }
    let stored = state
        .persisted
        .threads
        .get_mut(&key)
        .ok_or_else(|| ThreadStoreError::NotFound(key.clone()))?;
    if stored.turns.iter().any(|current| current.id == turn.id) {
        return Err(ThreadStoreError::AlreadyExists(turn.id.to_string()));
    }
    if let Some(owner) = stored
        .turns
        .iter()
        .rev()
        .find(|current| matches!(current.status, TurnStatus::Queued | TurnStatus::Running))
    {
        return Err(ThreadStoreError::Busy {
            thread: key,
            turn: owner.id.clone(),
        });
    }
    let lease = acquire_turn_lease(store_path, &key)?;
    state.active_turns.insert(
        key,
        ActiveTurn {
            turn_id: turn.id.clone(),
            _lease: lease,
        },
    );
    stored.metadata.updated_at = turn.started_at;
    stored.turns.push(turn);
    Ok(())
}

fn require_owner(
    state: &StoreState,
    thread: &ThreadId,
    turn: &TurnId,
) -> Result<(), ThreadStoreError> {
    match state.active_turns.get(thread.as_str()) {
        Some(owner) if &owner.turn_id == turn => Ok(()),
        Some(owner) => Err(ThreadStoreError::Busy {
            thread: thread.to_string(),
            turn: owner.turn_id.clone(),
        }),
        None => Err(ThreadStoreError::TurnNotActive(turn.to_string())),
    }
}

fn find_turn_mut<'a>(
    state: &'a mut StoreState,
    thread: &ThreadId,
    turn: &TurnId,
) -> Result<&'a mut Turn, ThreadStoreError> {
    state
        .persisted
        .threads
        .get_mut(thread.as_str())
        .ok_or_else(|| ThreadStoreError::NotFound(thread.to_string()))?
        .turns
        .iter_mut()
        .find(|current| &current.id == turn)
        .ok_or_else(|| ThreadStoreError::NotFound(turn.to_string()))
}

fn acquire_global_lock(path: &Path) -> Result<File, ThreadStoreError> {
    let lock_path = path.with_extension("lock");
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(ThreadStoreError::Io)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(ThreadStoreError::Io)?;
    file.lock_exclusive().map_err(ThreadStoreError::Io)?;
    Ok(file)
}

fn acquire_turn_lease(path: &Path, thread: &str) -> Result<File, ThreadStoreError> {
    let lock_path = turn_lock_path(path, thread);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(ThreadStoreError::Io)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(ThreadStoreError::Io)?;
    file.try_lock_exclusive().map_err(|error| {
        if lock_is_contended(&error) {
            ThreadStoreError::LeaseBusy(thread.to_string())
        } else {
            ThreadStoreError::Io(error)
        }
    })?;
    Ok(file)
}

fn lock_is_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock || matches!(error.raw_os_error(), Some(32 | 33))
}

fn recover_orphaned_turns(
    path: &Path,
    persisted: &mut PersistedState,
) -> Result<bool, ThreadStoreError> {
    let mut recovered = false;
    for (id, thread) in &mut persisted.threads {
        if !thread
            .turns
            .iter()
            .any(|turn| matches!(turn.status, TurnStatus::Queued | TurnStatus::Running))
        {
            continue;
        }
        let lease = match acquire_turn_lease(path, id) {
            Ok(lease) => lease,
            Err(ThreadStoreError::LeaseBusy(_)) => continue,
            Err(error) => return Err(error),
        };
        for turn in &mut thread.turns {
            if matches!(turn.status, TurnStatus::Queued | TurnStatus::Running) {
                turn.status = TurnStatus::Failed;
                turn.completed_at = Some(thread.metadata.updated_at);
                turn.error = Some("runtime restarted before turn completion".to_string());
                recovered = true;
            }
        }
        drop(lease);
    }
    Ok(recovered)
}

fn turn_lock_path(path: &Path, thread: &str) -> PathBuf {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path
        .as_os_str()
        .to_string_lossy()
        .bytes()
        .chain(std::iter::once(0))
        .chain(thread.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent
        .join("thread-leases-v2")
        .join(format!("{hash:016x}.lock"))
}

fn load_state(path: &Path) -> Result<PersistedState, ThreadStoreError> {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(state) => Ok(state),
            Err(primary_error) => match recover_state(path)? {
                Some(state) => Ok(state),
                None => Err(ThreadStoreError::Decode(primary_error)),
            },
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(recover_state(path)?.unwrap_or_default())
        }
        Err(error) => Err(ThreadStoreError::Io(error)),
    }
}

fn recover_state(path: &Path) -> Result<Option<PersistedState>, ThreadStoreError> {
    for candidate in [path.with_extension("bak"), path.with_extension("tmp")] {
        let text = match fs::read_to_string(&candidate) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ThreadStoreError::Io(error)),
        };
        let state: PersistedState = match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(_) => continue,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ThreadStoreError::Io)?;
        }
        fs::write(path, text).map_err(ThreadStoreError::Io)?;
        return Ok(Some(state));
    }
    Ok(None)
}

fn save_state(path: &Path, state: &PersistedState) -> Result<(), ThreadStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ThreadStoreError::Io)?;
    }
    let temporary = path.with_extension("tmp");
    let backup = path.with_extension("bak");
    let bytes = serde_json::to_vec(state).map_err(ThreadStoreError::Encode)?;
    fs::write(&temporary, bytes).map_err(ThreadStoreError::Io)?;
    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(ThreadStoreError::Io)?;
        }
        fs::rename(path, &backup).map_err(ThreadStoreError::Io)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(ThreadStoreError::Io(error));
    }
    if backup.exists() {
        fs::remove_file(backup).map_err(ThreadStoreError::Io)?;
    }
    Ok(())
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
