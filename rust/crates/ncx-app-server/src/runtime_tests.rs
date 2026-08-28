use super::*;

#[test]
fn runtime_goal_resume_arms_and_schedules_the_exact_thread() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("runtime-goal-resume").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "finish the migration".into(),
            max_goal_rounds: 3,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected goal");
    };

    let resumed = server
        .dispatch_with_runtime(
            ClientRequest::GoalResume {
                thread_id: thread_id.clone(),
                goal: ncx_protocol::GoalRef {
                    id: created.goal.id,
                    revision: created.goal.revision,
                },
            },
            &runtime,
        )
        .unwrap();

    let ResponsePayload::Goal(Some(resumed)) = resumed.response.payload else {
        panic!("expected resumed goal");
    };
    assert_eq!(resumed.activation, ncx_protocol::GoalActivation::Armed);
    assert_eq!(
        runtime.calls.lock().unwrap().as_slice(),
        ["goal-continue:runtime-goal-resume"]
    );
}

#[test]
fn runtime_goal_resume_failure_revokes_process_authority() {
    let server = server();
    let runtime = RecordingRuntime {
        fail_goal_continue: true,
        ..Default::default()
    };
    let thread_id = ThreadId::new("runtime-goal-rejected").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "finish the migration".into(),
            max_goal_rounds: 3,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected goal");
    };

    assert!(server
        .dispatch_with_runtime(
            ClientRequest::GoalResume {
                thread_id: thread_id.clone(),
                goal: ncx_protocol::GoalRef {
                    id: created.goal.id,
                    revision: created.goal.revision,
                },
            },
            &runtime,
        )
        .is_err());
    let read = server
        .dispatch(ClientRequest::GoalRead { thread_id })
        .unwrap();
    let ResponsePayload::Goal(Some(read)) = read.response.payload else {
        panic!("expected goal");
    };
    assert_eq!(read.goal.phase, ncx_protocol::GoalPhase::Active);
    assert_eq!(read.activation, ncx_protocol::GoalActivation::Disarmed);
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
                harness_profile: "full".into(),
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
                execution_mode: ncx_protocol::ExecutionMode::Orchestrator,
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
            "submit:runtime-thread:hello:1:Orchestrator",
            "interrupt:runtime-thread",
            "fork:runtime-thread:forked-thread",
        ]
    );
}

#[test]
fn harness_profile_can_change_only_before_the_first_turn() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("profile-thread").unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadCreateActivate {
                thread_id: thread_id.clone(),
                workspace: "workspace".into(),
                title: "title".into(),
                harness_profile: "full".into(),
            },
            &runtime,
        )
        .unwrap();
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadHarnessProfileSet {
                thread_id: thread_id.clone(),
                harness_profile: "coding".into(),
            },
            &runtime,
        )
        .unwrap();
    let read = server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    assert!(
        matches!(read.response.payload, ResponsePayload::Thread(ref thread) if thread.metadata.harness_profile == "coding")
    );

    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread_id.clone(),
            turn_id: TurnId::new("turn").unwrap(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .unwrap();
    assert!(server
        .dispatch_with_runtime(
            ClientRequest::ThreadHarnessProfileSet {
                thread_id,
                harness_profile: "minimal".into(),
            },
            &runtime,
        )
        .is_err());
}

#[test]
fn invalid_harness_profile_does_not_create_a_thread() {
    let server = server();
    let runtime = RecordingRuntime::default();
    let thread_id = ThreadId::new("invalid-profile").unwrap();
    assert!(server
        .dispatch_with_runtime(
            ClientRequest::ThreadCreateActivate {
                thread_id: thread_id.clone(),
                workspace: "workspace".into(),
                title: "title".into(),
                harness_profile: "unknown".into(),
            },
            &runtime,
        )
        .is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .is_err());
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
fn custom_provider_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
    assert!(matches!(
        server
            .dispatch_with_runtime(ClientRequest::CustomProviderList, &runtime)
            .unwrap()
            .response
            .payload,
        ResponsePayload::CustomProviders(ref value) if value[0]["id"] == "relay"
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::CustomProviderSave {
                id: Some("relay".into()),
                name: "Relay".into(),
                protocol: "openai".into(),
                base_url: "https://relay.example/v1".into(),
                api_key: Some("not-recorded".into()),
                models: vec!["gpt-5.6-sol".into()],
            },
            &runtime,
        )
        .unwrap();
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::CustomProviderModelsDiscover { id: "relay".into() },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::Models(ref models) if models == &["gpt-5.6-sol"]
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::CustomProviderActivate {
                id: "relay".into(),
                model: "gpt-5.6-sol".into(),
            },
            &runtime,
        )
        .unwrap();
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::CustomProviderChatProbe {
                    id: "relay".into(),
                    model: "gpt-5.6-sol".into(),
                },
                &runtime,
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::ProviderChatProbe(ref value) if value["confirmed_model"] == "gpt-5.6-sol"
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::CustomProviderDelete { id: "relay".into() },
            &runtime,
        )
        .unwrap();
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "provider-save:relay:Relay:openai:https://relay.example/v1:1",
            "provider-discover:relay",
            "provider-activate:relay:gpt-5.6-sol",
            "provider-chat-probe:relay:gpt-5.6-sol",
            "provider-delete:relay",
        ]
    );
}

#[test]
fn codex_plugin_requests_are_routed_by_the_app_server() {
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
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "plugin-install:plugin-dir:false",
            "plugin-enabled:demo:false",
            "plugin-uninstall:demo",
        ]
    );
}

#[test]
fn marketplace_requests_are_routed_by_the_app_server() {
    let server = server();
    let runtime = RecordingRuntime::default();
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
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::DshMarketplaceSearch {
                    source: "dshfind".into(),
                    manifest_url: None,
                    query: "demo".into()
                },
                &runtime
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::DshMarketplace(_)
    ));
    assert!(matches!(
        server
            .dispatch_with_runtime(
                ClientRequest::DshMarketplacePreview {
                    item: serde_json::json!({"id":"demo"})
                },
                &runtime
            )
            .unwrap()
            .response
            .payload,
        ResponsePayload::DshMarketplacePreview(_)
    ));
    server
        .dispatch_with_runtime(
            ClientRequest::DshMarketplaceInstall {
                item: serde_json::json!({"id":"demo"}),
                upgrade: false,
            },
            &runtime,
        )
        .unwrap();
    assert_eq!(
        *runtime.calls.lock().unwrap(),
        vec![
            "marketplace-install:marketplace.json:demo:true",
            "dsh-search:dshfind:demo",
            "dsh-preview:demo",
            "dsh-install:demo:false",
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
