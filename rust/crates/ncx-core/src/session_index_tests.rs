use super::*;

#[test]
fn parse_ts_ms_orders_legacy_iso_before_ms_epoch() {
    let iso = parse_ts_ms("2026-06-08T20:08:39"); // legacy ISO (older)
    let ms = parse_ts_ms("1783184340626"); // ms-epoch ~2026-07 (newer)
    assert!(iso > 0 && ms > 0);
    assert!(iso < ms, "ISO {iso} should sort older than ms {ms}");
    // 2026-06-08 is ~1.78e12 ms since epoch.
    assert!(
        (1_700_000_000_000..1_800_000_000_000).contains(&iso),
        "{iso}"
    );
    assert_eq!(parse_ts_ms(""), 0);
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ncx_session_index_{name}_{}", now_stamp()))
}

fn msgs() -> Vec<Value> {
    vec![
        json!({"role": "system", "content": "sys"}),
        json!({"role": "user", "content": format!("{COMPACTED_HISTORY_PREFIX}；测试]\n用户：旧任务")}),
        json!({"role": "user", "content": "fix login"}),
        json!({"role": "assistant", "content": "looking", "tool_calls": [
            {"id": "1", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}
        ]}),
        json!({"role": "tool", "tool_call_id": "1", "name": "read_file", "content": "..."}),
        json!({"role": "assistant", "content": "fixed"}),
    ]
}

#[test]
fn summarize_pulls_title_snippet_counts_and_tools() {
    let s = summarize(
        "sid",
        "/proj",
        &msgs(),
        "/proj/.nanocodex/session.jsonl",
        Some("2026-06-01T10:00:00".into()),
        None,
        true,
    );
    assert_eq!(s.title, "fix login");
    assert_eq!(s.snippet, "fixed");
    assert_eq!(s.user_messages, 1);
    assert_eq!(s.assistant_messages, 2);
    assert_eq!(s.tool_calls, 1);
    assert_eq!(s.recent_tools, vec!["read_file"]);
    assert_eq!(s.created_at, "2026-06-01T10:00:00");
    assert!(s.has_snapshot);
}

#[test]
fn generated_title_is_persisted_and_survives_later_turns() {
    let path = tmp_path("generated_title").join("sessions.jsonl");
    let mut idx = SessionIndex::new(path);
    let workspace = PathBuf::from("/project");
    let log_path = workspace.join("session.jsonl");
    let mut session = Session::new("sys");
    session.add_user(json!("这里是很长的背景资料，帮我整理成 PDF"));
    session.add_assistant("完成", None, "");

    let first = idx.record_turn_with_title(
        "sid",
        &workspace,
        &session,
        &log_path,
        Some("整理背景资料 PDF"),
    );
    assert_eq!(first.title, "整理背景资料 PDF");

    session.add_user(json!("再补充一页"));
    session.add_assistant("已补充", None, "");
    let second = idx.record_turn("sid", &workspace, &session, &log_path);
    assert_eq!(second.title, "整理背景资料 PDF");
}

#[test]
fn index_upserts_and_sorts_newest_first() {
    let path = tmp_path("sort").join("sessions.jsonl");
    let mut idx = SessionIndex::new(path);
    idx.record(summarize(
        "old",
        "/p",
        &msgs(),
        "",
        Some("2026-06-01T09:00:00".into()),
        None,
        false,
    ));
    idx.record(summarize(
        "new",
        "/p",
        &msgs(),
        "",
        Some("2026-06-01T11:00:00".into()),
        None,
        false,
    ));
    idx.record(summarize(
        "old",
        "/p",
        &msgs(),
        "",
        Some("2026-06-01T12:00:00".into()),
        Some("2026-06-01T09:00:00"),
        false,
    ));

    let entries = idx.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].session_id, "old");
    assert_eq!(entries[0].created_at, "2026-06-01T09:00:00");
    assert_eq!(entries[1].session_id, "new");
}

#[test]
fn latest_resumable_session_is_scoped_to_workspace_and_skips_archived() {
    let dir = tmp_path("latest_resumable");
    let workspace = dir.join("project");
    let other_workspace = dir.join("other");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&other_workspace).unwrap();
    let mut idx = SessionIndex::new(dir.join("sessions.jsonl"));

    for (id, ws, updated, archived) in [
        ("wanted", workspace.as_path(), "1787000000001", false),
        ("archived", workspace.as_path(), "1787000000003", true),
        ("other", other_workspace.as_path(), "1787000000004", false),
    ] {
        let session = Session::new("sys");
        assert!(idx.save_snapshot(id, &session));
        let mut summary = summarize(
            id,
            &ws.display().to_string(),
            &session.full_messages(),
            "",
            Some(updated.into()),
            None,
            true,
        );
        summary.archived = archived;
        idx.record(summary);
    }

    let (summary, messages) = idx
        .latest_resumable_for_workspace(&workspace)
        .expect("the latest active session in this workspace");
    assert_eq!(summary.session_id, "wanted");
    assert_eq!(messages[0]["role"], "system");
}

#[test]
fn persists_and_loads_legacy_rows() {
    let dir = tmp_path("legacy");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sessions.jsonl");
    std::fs::write(
        &path,
        "{\"workspace\":\"/old\",\"title\":\"legacy\",\"updated_at\":\"2026\"}\nnot json\n",
    )
    .unwrap();

    let idx = SessionIndex::new(path);
    let entries = idx.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "legacy:/old");
    assert_eq!(entries[0].title, "legacy");
}

#[test]
fn snapshot_round_trip_redacts_image_data() {
    let dir = tmp_path("snapshot");
    let mut idx = SessionIndex::new(dir.join("sessions.jsonl"));
    let mut session = Session::new("sys");
    session.add_user(json!([
        {"type": "text", "text": "describe"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
    ]));

    idx.record_turn(
        "sid",
        Path::new("/p"),
        &session,
        Path::new("/p/.nanocodex/session.jsonl"),
    );
    let loaded = idx.load_snapshot("sid").unwrap();
    let text = serde_json::to_string(&loaded).unwrap();
    assert!(text.contains("[image omitted from snapshot]"));
    assert!(!text.contains("data:image"));
    assert!(idx.get("sid").unwrap().has_snapshot);
}

#[test]
fn session_ids_are_unique() {
    assert_ne!(new_session_id(), new_session_id());
}
