//! Storage-neutral thread persistence with explicit per-thread turn ownership.

use ncx_protocol::{Thread, ThreadId, ThreadItem, ThreadMetadata, Turn, TurnId, TurnStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait ThreadStore: Send + Sync {
    fn create(&self, thread: Thread) -> Result<(), ThreadStoreError>;
    fn create_many(&self, threads: Vec<Thread>) -> Result<(), ThreadStoreError>;
    fn list(&self, include_archived: bool) -> Result<Vec<ThreadMetadata>, ThreadStoreError>;
    fn read(&self, id: &ThreadId) -> Result<Option<Thread>, ThreadStoreError>;
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
    ) -> Result<(), ThreadStoreError>;
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    threads: BTreeMap<String, Thread>,
}

#[derive(Default)]
struct StoreState {
    persisted: PersistedState,
    active_turns: HashMap<String, TurnId>,
}

pub struct JsonThreadStore {
    path: PathBuf,
    state: Mutex<StoreState>,
}

impl JsonThreadStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ThreadStoreError> {
        let path = path.into();
        let mut persisted = load_state(&path)?;
        let mut recovered = false;
        for thread in persisted.threads.values_mut() {
            for turn in &mut thread.turns {
                if matches!(turn.status, TurnStatus::Queued | TurnStatus::Running) {
                    turn.status = TurnStatus::Failed;
                    turn.completed_at = Some(thread.metadata.updated_at);
                    turn.error = Some("runtime restarted before turn completion".to_string());
                    recovered = true;
                }
            }
        }
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
        let result = operation(&mut state)?;
        save_state(&self.path, &state.persisted)?;
        Ok(result)
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
        let state = self.state.lock().map_err(|_| ThreadStoreError::Poisoned)?;
        let mut rows = state
            .persisted
            .threads
            .values()
            .map(|thread| thread.metadata.clone())
            .filter(|metadata| include_archived || !metadata.archived)
            .collect::<Vec<_>>();
        rows.sort_by_key(|metadata| std::cmp::Reverse(metadata.updated_at));
        Ok(rows)
    }

    fn read(&self, id: &ThreadId) -> Result<Option<Thread>, ThreadStoreError> {
        let state = self.state.lock().map_err(|_| ThreadStoreError::Poisoned)?;
        Ok(state.persisted.threads.get(id.as_str()).cloned())
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
            Ok(forked)
        })
    }

    fn claim_turn(&self, thread: &ThreadId, turn: Turn) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            let key = thread.as_str().to_string();
            if let Some(owner) = state.active_turns.get(&key) {
                return Err(ThreadStoreError::Busy {
                    thread: key,
                    turn: owner.clone(),
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
            state.active_turns.insert(key, turn.id.clone());
            stored.metadata.updated_at = turn.started_at;
            stored.turns.push(turn);
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
    ) -> Result<(), ThreadStoreError> {
        self.mutate(|state| {
            require_owner(state, thread, turn)?;
            let stored_turn = find_turn_mut(state, thread, turn)?;
            stored_turn.status = status;
            stored_turn.completed_at = Some(completed_at);
            stored_turn.error = error;
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

fn require_owner(
    state: &StoreState,
    thread: &ThreadId,
    turn: &TurnId,
) -> Result<(), ThreadStoreError> {
    match state.active_turns.get(thread.as_str()) {
        Some(owner) if owner == turn => Ok(()),
        Some(owner) => Err(ThreadStoreError::Busy {
            thread: thread.to_string(),
            turn: owner.clone(),
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
    Busy { thread: String, turn: TurnId },
    TurnNotActive(String),
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
            Self::Poisoned => write!(formatter, "thread store lock is poisoned"),
            Self::Io(error) => error.fmt(formatter),
            Self::Decode(error) | Self::Encode(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ThreadStoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_protocol::{ItemId, ThreadItem};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store(name: &str) -> JsonThreadStore {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        JsonThreadStore::open(std::env::temp_dir().join(format!("ncx-{name}-{unique}.json")))
            .unwrap()
    }

    fn thread(id: &str) -> Thread {
        Thread {
            metadata: ThreadMetadata {
                id: ThreadId::new(id).unwrap(),
                workspace: "workspace".into(),
                title: "title".into(),
                archived: false,
                created_at: 1,
                updated_at: 1,
            },
            turns: Vec::new(),
        }
    }

    fn turn(id: &str) -> Turn {
        Turn {
            id: TurnId::new(id).unwrap(),
            status: TurnStatus::Running,
            items: Vec::new(),
            started_at: 2,
            completed_at: None,
            error: None,
        }
    }

    #[test]
    fn one_thread_accepts_only_one_active_turn() {
        let store = temp_store("ownership");
        let id = ThreadId::new("thread").unwrap();
        store.create(thread("thread")).unwrap();
        store.claim_turn(&id, turn("turn-1")).unwrap();
        assert!(matches!(
            store.claim_turn(&id, turn("turn-2")),
            Err(ThreadStoreError::Busy { .. })
        ));
        store
            .finish_turn(
                &id,
                &TurnId::new("turn-1").unwrap(),
                TurnStatus::Completed,
                3,
                None,
            )
            .unwrap();
        store.claim_turn(&id, turn("turn-2")).unwrap();
    }

    #[test]
    fn items_are_owned_by_the_claimed_turn_and_persisted() {
        let store = temp_store("items");
        let thread_id = ThreadId::new("thread").unwrap();
        let turn_id = TurnId::new("turn").unwrap();
        store.create(thread("thread")).unwrap();
        store.claim_turn(&thread_id, turn("turn")).unwrap();
        store
            .append_item(
                &thread_id,
                &turn_id,
                ThreadItem::UserMessage {
                    id: ItemId::new("item").unwrap(),
                    text: "hello".into(),
                },
                3,
            )
            .unwrap();
        assert_eq!(
            store.read(&thread_id).unwrap().unwrap().turns[0]
                .items
                .len(),
            1
        );
    }

    #[test]
    fn fork_keeps_history_but_changes_durable_identity() {
        let store = temp_store("fork");
        store.create(thread("source")).unwrap();
        store
            .claim_turn(&ThreadId::new("source").unwrap(), turn("running"))
            .unwrap();
        let forked = store
            .fork(
                &ThreadId::new("source").unwrap(),
                ThreadId::new("target").unwrap(),
            )
            .unwrap();
        assert_eq!(forked.metadata.id.as_str(), "target");
        assert_eq!(forked.turns[0].status, TurnStatus::Cancelled);
        assert!(store
            .read(&ThreadId::new("source").unwrap())
            .unwrap()
            .is_some());
    }

    #[test]
    fn completed_turn_survives_store_reopen() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ncx-reopen-{unique}.json"));
        let thread_id = ThreadId::new("thread").unwrap();
        let turn_id = TurnId::new("turn").unwrap();
        {
            let store = JsonThreadStore::open(&path).unwrap();
            store.create(thread("thread")).unwrap();
            store.claim_turn(&thread_id, turn("turn")).unwrap();
            store
                .finish_turn(&thread_id, &turn_id, TurnStatus::Completed, 3, None)
                .unwrap();
        }
        let reopened = JsonThreadStore::open(&path).unwrap();
        let stored = reopened.read(&thread_id).unwrap().unwrap();
        assert_eq!(stored.turns[0].status, TurnStatus::Completed);
    }

    #[test]
    fn running_turn_is_recovered_as_failed_after_restart() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ncx-recover-running-{unique}.json"));
        let thread_id = ThreadId::new("thread").unwrap();
        {
            let store = JsonThreadStore::open(&path).unwrap();
            store.create(thread("thread")).unwrap();
            store.claim_turn(&thread_id, turn("turn")).unwrap();
        }
        let reopened = JsonThreadStore::open(&path).unwrap();
        let stored = reopened.read(&thread_id).unwrap().unwrap();
        assert_eq!(stored.turns[0].status, TurnStatus::Failed);
        assert!(stored.turns[0]
            .error
            .as_deref()
            .unwrap()
            .contains("runtime restarted"));
        reopened.claim_turn(&thread_id, turn("next")).unwrap();
    }

    #[test]
    fn corrupt_primary_recovers_from_last_backup() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ncx-recover-backup-{unique}.json"));
        let store = JsonThreadStore::open(&path).unwrap();
        store.create(thread("thread")).unwrap();
        fs::copy(&path, path.with_extension("bak")).unwrap();
        fs::write(&path, "not-json").unwrap();

        let reopened = JsonThreadStore::open(&path).unwrap();
        assert!(reopened
            .read(&ThreadId::new("thread").unwrap())
            .unwrap()
            .is_some());
    }
}
