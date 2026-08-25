use super::*;
use ncx_protocol::{ClientRequest, ItemId, ResponsePayload, ThreadId, ThreadItem, TurnId};
use ncx_thread_store::JsonThreadStore;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn server() -> AppServer<JsonThreadStore> {
    let path = std::env::temp_dir().join(format!(
        "ncx-app-server-{}-{}.json",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    AppServer::new(Arc::new(JsonThreadStore::open(path).unwrap()), || 100)
}

#[test]
fn create_and_start_turn_emit_owned_v2_events() {
    let server = server();
    let created = server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: None,
            workspace: "workspace".into(),
            title: "title".into(),
        })
        .unwrap();
    let ResponsePayload::Thread(thread) = created.response.payload else {
        panic!("expected created thread");
    };
    assert_eq!(created.events[0].thread_id, thread.metadata.id);
    assert_eq!(created.events[0].protocol_version, PROTOCOL_VERSION);

    let turn_id = TurnId::new("turn-1").unwrap();
    let started = server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread.metadata.id.clone(),
            turn_id: turn_id.clone(),
        })
        .unwrap();
    assert_eq!(started.events[0].thread_id, thread.metadata.id);
    assert_eq!(started.events[0].turn_id, Some(turn_id));
}

#[test]
fn second_concurrent_turn_is_rejected() {
    let server = server();
    let created = server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: None,
            workspace: "workspace".into(),
            title: "title".into(),
        })
        .unwrap();
    let ResponsePayload::Thread(thread) = created.response.payload else {
        panic!("expected thread");
    };
    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread.metadata.id.clone(),
            turn_id: TurnId::new("one").unwrap(),
        })
        .unwrap();
    assert!(server
        .dispatch(ClientRequest::TurnStart {
            thread_id: ThreadId::new(thread.metadata.id.as_str()).unwrap(),
            turn_id: TurnId::new("two").unwrap(),
        })
        .is_err());
}

#[test]
fn rename_and_fork_are_owned_by_the_app_server() {
    let server = server();
    let thread_id = ThreadId::new("source").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "old".into(),
        })
        .unwrap();
    let renamed = server
        .dispatch(ClientRequest::ThreadRename {
            thread_id: thread_id.clone(),
            title: "new title".into(),
        })
        .unwrap();
    assert!(matches!(
        renamed.events[0].event,
        Event::ThreadUpdated { .. }
    ));
    let forked = server
        .dispatch(ClientRequest::ThreadFork {
            thread_id,
            new_thread_id: ThreadId::new("target").unwrap(),
        })
        .unwrap();
    let ResponsePayload::Thread(forked_thread) = forked.response.payload else {
        panic!("expected forked thread");
    };
    assert_eq!(forked_thread.metadata.title, "new title");
    assert_eq!(forked.events[0].thread_id.as_str(), "target");
}

#[test]
fn model_context_is_replaced_without_rewriting_visible_turns() {
    let server = server();
    let thread_id = ThreadId::new("thread").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "title".into(),
        })
        .unwrap();
    let updated = server
        .dispatch(ClientRequest::ThreadModelContextReplace {
            thread_id: thread_id.clone(),
            messages: vec![serde_json::json!({"role":"user","content":"compact"})],
        })
        .unwrap();
    assert!(matches!(
        updated.events[0].event,
        Event::ModelContextUpdated { message_count: 1 }
    ));
    let read = server
        .dispatch(ClientRequest::ThreadModelContextRead { thread_id })
        .unwrap();
    let ResponsePayload::ModelContext(Some(context)) = read.response.payload else {
        panic!("expected stored model context");
    };
    assert_eq!(context.messages[0]["content"], "compact");
}

#[test]
fn visible_thread_never_returns_tool_logs_or_intermediate_assistant_text() {
    let server = server();
    let thread_id = ThreadId::new("visible").unwrap();
    let turn_id = TurnId::new("turn").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "title".into(),
        })
        .unwrap();
    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
        })
        .unwrap();
    for item in [
        ThreadItem::UserMessage {
            id: ItemId::new("user").unwrap(),
            text: "请求".into(),
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("intermediate").unwrap(),
            text: "正在执行".into(),
        },
        ThreadItem::ToolResult {
            id: ItemId::new("result").unwrap(),
            call_id: ItemId::new("call").unwrap(),
            output: "secret tool log".into(),
            success: true,
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("final").unwrap(),
            text: "最终结论".into(),
        },
    ] {
        server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
                item,
            })
            .unwrap();
    }
    let visible = server
        .dispatch(ClientRequest::ThreadReadVisible { thread_id })
        .unwrap();
    let ResponsePayload::Thread(thread) = visible.response.payload else {
        panic!("expected visible thread");
    };
    assert_eq!(thread.turns[0].items.len(), 2);
    assert!(matches!(
        &thread.turns[0].items[1],
        ThreadItem::AssistantMessage { text, .. } if text == "最终结论"
    ));
}

#[derive(Default)]
struct RecordingRuntime {
    calls: Mutex<Vec<String>>,
}

impl AppServerAdapter for RecordingRuntime {
    fn create_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("new:{thread_id}"));
        Ok(())
    }

    fn activate_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("activate:{thread_id}"));
        Ok(())
    }

    fn fork_thread(&self, source_id: &ThreadId, target_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("fork:{source_id}:{target_id}"));
        Ok(())
    }

    fn submit_turn(
        &self,
        thread_id: &ThreadId,
        text: String,
        images: Vec<String>,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("submit:{thread_id}:{text}:{}", images.len()));
        Ok(())
    }

    fn interrupt_latest(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("interrupt:{thread_id}"));
        Ok(())
    }

    fn runtime_status(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"model":"test-model"}))
    }

    fn refresh_ready(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("ready".into());
        Ok(())
    }

    fn set_workspace(&self, path: String) -> Result<String, String> {
        self.calls.lock().unwrap().push(format!("workspace:{path}"));
        Ok(path)
    }

    fn approve(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        decision: String,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!(
            "approve:{}:{id}:{decision}",
            thread_id.map(ToString::to_string).unwrap_or_default()
        ));
        Ok(())
    }

    fn answer(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        answer: Option<String>,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!(
            "answer:{}:{id}:{}",
            thread_id.map(ToString::to_string).unwrap_or_default(),
            answer.unwrap_or_default()
        ));
        Ok(())
    }

    fn read_settings(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"model":"test-model"}))
    }

    fn update_settings(
        &self,
        updates: std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("settings:{}", updates.len()));
        Ok(())
    }

    fn set_model(&self, model: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("model:{model}"));
        Ok(())
    }

    fn set_permission_mode(&self, mode: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("mode:{mode}"));
        Ok(())
    }

    fn read_model_catalog(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"providers":[]}))
    }

    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("preset:{provider_id}:{model_id}"));
        Ok(serde_json::json!({"model_id":model_id}))
    }

    fn harness_diagnostics(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"llm":true}))
    }

    fn list_external_plugins(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"id":"demo.echo"}]))
    }

    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("external-install:{source}:{upgrade}"));
        Ok(serde_json::json!({"id":"demo.echo"}))
    }

    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("external-enabled:{id}:{enabled}"));
        Ok(())
    }

    fn list_memory(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"note":"remember"}]))
    }

    fn add_memory(&self, note: String, tags: Vec<String>) -> Result<bool, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("memory-add:{note}:{}", tags.len()));
        Ok(true)
    }

    fn consolidate_memory(&self) -> Result<u64, String> {
        self.calls.lock().unwrap().push("memory-consolidate".into());
        Ok(2)
    }

    fn list_codex_plugins(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"name":"demo"}]))
    }

    fn install_codex_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-install:{source}:{upgrade}"));
        Ok(serde_json::json!({"name":"demo"}))
    }

    fn set_codex_plugin_enabled(&self, name: String, enabled: bool) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-enabled:{name}:{enabled}"));
        Ok(())
    }

    fn uninstall_codex_plugin(&self, name: String) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-uninstall:{name}"));
        Ok(())
    }

    fn list_marketplaces(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"name":"local"}]))
    }

    fn install_marketplace_plugin(
        &self,
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls.lock().unwrap().push(format!(
            "marketplace-install:{marketplace_path}:{plugin_name}:{upgrade}"
        ));
        Ok(serde_json::json!({"name":plugin_name}))
    }
}

#[test]
fn runtime_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("runtime-thread").unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadCreateActivate {
                thread_id: thread_id.clone(),
                workspace: "workspace".into(),
                title: "title".into(),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadActivate {
                thread_id: thread_id.clone(),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::TurnSubmit {
                thread_id: thread_id.clone(),
                text: "hello".into(),
                images: vec!["image.png".into()],
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::TurnInterruptLatest {
                thread_id: thread_id.clone(),
            },
            &runtime,
        )
        .unwrap();
    let target = ThreadId::new("forked-thread").unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadForkActivate {
                thread_id: thread_id.clone(),
                new_thread_id: target.clone(),
            },
            &runtime,
        )
        .unwrap();

    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "new:runtime-thread",
            "activate:runtime-thread",
            "submit:runtime-thread:hello:1",
            "interrupt:runtime-thread",
            "fork:runtime-thread:forked-thread",
        ]
    );
}

#[test]
fn interaction_and_desktop_runtime_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("interaction-thread").unwrap();
    let status = server
        .dispatch_with_runtime(ClientRequest::RuntimeStatusRead, &runtime)
        .unwrap();
    assert!(matches!(
        status.response.payload,
        ResponsePayload::RuntimeStatus(ref value) if value["model"] == "test-model"
    ));
    server
        .dispatch_with_runtime(ClientRequest::RuntimeReadyRefresh, &runtime)
        .unwrap();
    let workspace = server
        .dispatch_with_runtime(
            ClientRequest::WorkspaceSet {
                path: "D:/workspace".into(),
            },
            &runtime,
        )
        .unwrap();
    assert!(matches!(
        workspace.response.payload,
        ResponsePayload::Workspace(ref path) if path == "D:/workspace"
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::InteractionApprove {
                thread_id: Some(thread_id.clone()),
                id: 7,
                decision: "once".into(),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::InteractionAnswer {
                thread_id: Some(thread_id),
                id: 8,
                answer: Some("yes".into()),
            },
            &runtime,
        )
        .unwrap();
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "ready",
            "workspace:D:/workspace",
            "approve:interaction-thread:7:once",
            "answer:interaction-thread:8:yes",
        ]
    );
}

#[test]
fn settings_and_model_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::SettingsRead, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::Settings(ref value) if value["model"] == "test-model"
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::SettingsUpdate {
                updates: std::collections::BTreeMap::from([(
                    "reasoning_effort".into(),
                    "high".into(),
                )]),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::RuntimeModelSet {
                model: "deepseek-v4".into(),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::RuntimePermissionModeSet {
                mode: "plan".into(),
            },
            &runtime,
        )
        .unwrap();
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::ModelCatalogRead, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::ModelCatalog(ref value) if value["providers"].is_array()
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::ModelPresetApply {
                    provider_id: "deepseek".into(),
                    model_id: "deepseek-v4".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::ModelPreset(ref value) if value["model_id"] == "deepseek-v4"
    ));
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "settings:1",
            "model:deepseek-v4",
            "mode:plan",
            "preset:deepseek:deepseek-v4",
        ]
    );
}

#[test]
fn plugin_and_marketplace_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let listed = server
        .dispatch_with_runtime(ClientRequest::CodexPluginList, &runtime)
        .unwrap();
    assert!(matches!(
        listed.response.payload,
        ResponsePayload::CodexPlugins(_)
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::CodexPluginInstall {
                source: "plugin-dir".into(),
                upgrade: false,
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::CodexPluginSetEnabled {
                name: "demo".into(),
                enabled: false,
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::CodexPluginUninstall {
                name: "demo".into(),
            },
            &runtime,
        )
        .unwrap();
    let marketplaces = server
        .dispatch_with_runtime(ClientRequest::MarketplaceList, &runtime)
        .unwrap();
    assert!(matches!(
        marketplaces.response.payload,
        ResponsePayload::Marketplaces(_)
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::MarketplacePluginInstall {
                marketplace_path: "marketplace.json".into(),
                plugin_name: "demo".into(),
                upgrade: true,
            },
            &runtime,
        )
        .unwrap();
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "plugin-install:plugin-dir:false",
            "plugin-enabled:demo:false",
            "plugin-uninstall:demo",
            "marketplace-install:marketplace.json:demo:true",
        ]
    );
}

#[test]
fn harness_diagnostics_and_external_plugins_use_the_same_protocol_boundary() {
    let server = server();
    let runtime = RecordingRuntime::default();
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::HarnessDiagnosticsRead, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::HarnessDiagnostics(ref value) if value["llm"] == true
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::ExternalPluginList, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::ExternalPlugins(ref value) if value.is_array()
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::ExternalPluginInstall {
                source: "external-dir".into(),
                upgrade: true,
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ExternalPluginSetEnabled {
                id: "demo.echo".into(),
                enabled: false,
            },
            &runtime,
        )
        .unwrap();
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "external-install:external-dir:true",
            "external-enabled:demo.echo:false",
        ]
    );
}

#[test]
fn memory_service_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::MemoryList, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::MemoryNotes(ref value) if value.is_array()
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::MemoryAdd {
                    note: "remember".into(),
                    tags: vec!["project".into()],
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::Bool(true)
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::MemoryConsolidate, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::Count(2)
    ));
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec!["memory-add:remember:1", "memory-consolidate"]
    );
}
