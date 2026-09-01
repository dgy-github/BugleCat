//! Private JSON persistence, locking, and turn-ownership helpers.

use super::{ActiveTurn, PersistedState, StoreState, ThreadStoreError};
use fs2::FileExt;
use ncx_protocol::{ThreadId, Turn, TurnId, TurnStatus};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

pub(super) fn claim_turn_in_state(
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

pub(super) fn require_owner(
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

pub(super) fn find_turn_mut<'a>(
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

pub(super) fn acquire_global_lock(path: &Path) -> Result<File, ThreadStoreError> {
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

pub(super) fn recover_orphaned_turns(
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

pub(super) fn load_state(path: &Path) -> Result<PersistedState, ThreadStoreError> {
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

pub(super) fn save_state(path: &Path, state: &PersistedState) -> Result<(), ThreadStoreError> {
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
