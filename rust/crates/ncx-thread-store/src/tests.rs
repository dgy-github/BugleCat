use super::*;
use ncx_protocol::{ItemId, ThreadItem};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_store(name: &str) -> JsonThreadStore {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    JsonThreadStore::open(std::env::temp_dir().join(format!("ncx-{name}-{unique}.json"))).unwrap()
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
        usage: TurnUsage::default(),
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
            TurnUsage::default(),
        )
        .unwrap();
    store.claim_turn(&id, turn("turn-2")).unwrap();
}

#[test]
fn different_threads_hold_active_turns_concurrently() {
    let store = temp_store("parallel-ownership");
    let first = ThreadId::new("first").unwrap();
    let second = ThreadId::new("second").unwrap();
    store.create(thread("first")).unwrap();
    store.create(thread("second")).unwrap();
    store.claim_turn(&first, turn("turn-first")).unwrap();
    store.claim_turn(&second, turn("turn-second")).unwrap();
    assert_eq!(
        store.read(&first).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert_eq!(
        store.read(&second).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
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
        let usage = TurnUsage {
            tokens: [
                ("prompt_tokens".to_string(), 12),
                ("completion_tokens".to_string(), 3),
            ]
            .into_iter()
            .collect(),
            estimated_cost: Some(0.02),
            currency: Some("CNY".into()),
        };
        store
            .finish_turn(&thread_id, &turn_id, TurnStatus::Completed, 3, None, usage)
            .unwrap();
    }
    let reopened = JsonThreadStore::open(&path).unwrap();
    let stored = reopened.read(&thread_id).unwrap().unwrap();
    assert_eq!(stored.turns[0].status, TurnStatus::Completed);
    assert_eq!(stored.turns[0].usage.tokens["prompt_tokens"], 12);
    assert_eq!(stored.turns[0].usage.estimated_cost, Some(0.02));
}

#[test]
fn compacted_model_context_is_replaced_and_survives_reopen() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-model-context-{unique}.json"));
    let thread_id = ThreadId::new("thread").unwrap();
    {
        let store = JsonThreadStore::open(&path).unwrap();
        store.create(thread("thread")).unwrap();
        store
            .replace_model_context(
                &thread_id,
                vec![serde_json::json!({"role":"user","content":"summary"})],
                7,
            )
            .unwrap();
    }
    let reopened = JsonThreadStore::open(&path).unwrap();
    let context = reopened.read_model_context(&thread_id).unwrap().unwrap();
    assert_eq!(context.updated_at, 7);
    assert_eq!(context.messages[0]["content"], "summary");
}

#[test]
fn fork_copies_model_context_under_the_new_thread_identity() {
    let store = temp_store("fork-model-context");
    let source = ThreadId::new("source").unwrap();
    let target = ThreadId::new("target").unwrap();
    store.create(thread("source")).unwrap();
    store
        .replace_model_context(
            &source,
            vec![serde_json::json!({"role":"assistant","content":"done"})],
            4,
        )
        .unwrap();
    store.fork(&source, target.clone()).unwrap();
    let copied = store.read_model_context(&target).unwrap().unwrap();
    assert_eq!(copied.thread_id, target);
    assert_eq!(copied.messages[0]["content"], "done");
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
fn second_store_does_not_recover_or_overwrite_a_live_owner() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-cross-process-{unique}.json"));
    let first = JsonThreadStore::open(&path).unwrap();
    first.create(thread("first")).unwrap();
    let first_id = ThreadId::new("first").unwrap();
    first.claim_turn(&first_id, turn("turn-first")).unwrap();

    let second = JsonThreadStore::open(&path).unwrap();
    assert_eq!(
        second.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert!(matches!(
        second.claim_turn(&first_id, turn("overlap")),
        Err(ThreadStoreError::Busy { .. })
    ));
    second.create(thread("second")).unwrap();
    assert!(first
        .read(&ThreadId::new("second").unwrap())
        .unwrap()
        .is_some());
    assert_eq!(
        first.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );

    drop(first);
    assert_eq!(
        second.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Failed
    );
    second.claim_turn(&first_id, turn("next")).unwrap();
}

#[test]
fn cross_process_lease_helper() {
    let Ok(path) = std::env::var("NCX_THREAD_STORE_HELPER_PATH") else {
        return;
    };
    let ready = PathBuf::from(format!("{path}.ready"));
    let release = PathBuf::from(format!("{path}.release"));
    let store = JsonThreadStore::open(&path).unwrap();
    let id = ThreadId::new("owned").unwrap();
    store.create(thread("owned")).unwrap();
    store.claim_turn(&id, turn("child-turn")).unwrap();
    fs::write(&ready, b"ready").unwrap();
    for _ in 0..500 {
        if release.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("parent did not release helper");
}

#[test]
fn live_turn_lease_is_respected_across_processes() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-process-lease-{unique}.json"));
    let path_text = path.display().to_string();
    let ready = PathBuf::from(format!("{path_text}.ready"));
    let release = PathBuf::from(format!("{path_text}.release"));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::cross_process_lease_helper",
            "--nocapture",
        ])
        .env("NCX_THREAD_STORE_HELPER_PATH", &path_text)
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        assert!(child.try_wait().unwrap().is_none(), "helper exited early");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "helper did not acquire the turn lease");

    let observer = JsonThreadStore::open(&path).unwrap();
    let id = ThreadId::new("owned").unwrap();
    assert_eq!(
        observer.read(&id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert!(matches!(
        observer.claim_turn(&id, turn("overlap")),
        Err(ThreadStoreError::Busy { .. })
    ));
    observer.create(thread("observer-write")).unwrap();
    fs::write(&release, b"release").unwrap();
    assert!(child.wait().unwrap().success());

    assert_eq!(
        observer.read(&id).unwrap().unwrap().turns[0].status,
        TurnStatus::Failed
    );
    assert!(observer
        .read(&ThreadId::new("observer-write").unwrap())
        .unwrap()
        .is_some());
    let _ = fs::remove_file(ready);
    let _ = fs::remove_file(release);
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
