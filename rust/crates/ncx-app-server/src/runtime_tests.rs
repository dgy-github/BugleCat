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
fn failed_runtime_create_is_compensated_and_same_id_can_retry() {
    let server = server();
    let id = ThreadId::new("create-rollback").unwrap();
    let rejected = RecordingRuntime {
        fail_create_thread: true,
        ..Default::default()
    };
    let request = ClientRequest::ThreadCreateActivate {
        thread_id: id.clone(),
        workspace: "workspace".into(),
        title: "title".into(),
        harness_profile: "full".into(),
    };

    assert!(server
        .dispatch_with_runtime(request.clone(), &rejected)
        .is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: id.clone(),
        })
        .is_err());
    let listed = server
        .dispatch(ClientRequest::ThreadList {
            include_archived: true,
        })
        .unwrap();
    assert!(matches!(
        listed.response.payload,
        ResponsePayload::Threads(ref threads) if threads.is_empty()
    ));

    // The failed activation must not reserve the caller-provided ID.
    server
        .dispatch_with_runtime(request, &RecordingRuntime::default())
        .unwrap();
}

#[test]
fn pending_runtime_create_rejects_a_concurrent_activation_before_compensation() {
    let server = Arc::new(server());
    let id = ThreadId::new("create-activation-race").unwrap();
    let gate = Arc::new(ProfileValidationGate::default());
    let rejected = Arc::new(RecordingRuntime {
        create_activation_gate: Some(gate.clone()),
        fail_create_thread: true,
        ..Default::default()
    });
    let request = ClientRequest::ThreadCreateActivate {
        thread_id: id.clone(),
        workspace: "workspace".into(),
        title: "title".into(),
        harness_profile: "full".into(),
    };
    let pending_server = server.clone();
    let pending_runtime = rejected.clone();
    let pending = std::thread::spawn(move || {
        pending_server.dispatch_with_runtime(request, pending_runtime.as_ref())
    });

    gate.wait_until_entered();
    let activation_error = server
        .dispatch_with_runtime(
            ClientRequest::ThreadActivate {
                thread_id: id.clone(),
            },
            &RecordingRuntime::default(),
        )
        .unwrap_err();
    assert!(matches!(
        activation_error,
        AppServerError::InvalidRequest(ref message)
            if message.contains("activation is still in progress")
    ));

    gate.release();
    assert!(pending.join().unwrap().is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadRead { thread_id: id })
        .is_err());
}

fn assert_cross_process_runtime_handoff_keeps_provisioned_thread(
    request: impl FnOnce(ThreadId) -> ClientRequest,
) {
    let path = std::env::temp_dir().join(format!(
        "ncx-app-server-cross-process-{}-{}.json",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let creating_server = Arc::new(AppServer::new(
        Arc::new(JsonThreadStore::open(&path).unwrap()),
        || 100,
    ));
    // A separate JsonThreadStore/AppServer simulates a second GUI or CLI
    // process. Its durable activation marker must survive the first process's
    // failed host handoff.
    let activating_server = AppServer::new(Arc::new(JsonThreadStore::open(&path).unwrap()), || 100);
    let id = ThreadId::new("cross-process-activation").unwrap();
    let gate = Arc::new(ProfileValidationGate::default());
    let rejected = Arc::new(RecordingRuntime {
        create_activation_gate: Some(gate.clone()),
        fail_create_thread: true,
        ..Default::default()
    });
    let create_request = ClientRequest::ThreadCreateActivate {
        thread_id: id.clone(),
        workspace: "workspace".into(),
        title: "title".into(),
        harness_profile: "full".into(),
    };
    let pending_server = creating_server.clone();
    let pending_runtime = rejected.clone();
    let pending = std::thread::spawn(move || {
        pending_server.dispatch_with_runtime(create_request, pending_runtime.as_ref())
    });

    gate.wait_until_entered();
    activating_server
        .dispatch_with_runtime(request(id.clone()), &RecordingRuntime::default())
        .unwrap();

    gate.release();
    let error = pending.join().unwrap().unwrap_err();
    assert!(matches!(
        error,
        AppServerError::Runtime(ref message) if message.contains("was changed during activation and was retained")
    ));
    assert!(activating_server
        .dispatch(ClientRequest::ThreadRead { thread_id: id })
        .is_ok());
}

#[test]
fn cross_process_runtime_handoffs_keep_a_thread_when_the_provisioning_host_fails() {
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::ThreadActivate { thread_id }
    });
    // The host queues the Turn before its worker starts a durable Turn. It
    // must therefore establish the same cross-process rollback fence.
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::TurnSubmit {
            thread_id,
            text: "queued while another host activates".into(),
            images: Vec::new(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        }
    });
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::RuntimePermissionModeSet {
            thread_id,
            mode: "default".into(),
        }
    });
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::TurnInterruptLatest { thread_id }
    });
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::InteractionApprove {
            thread_id: Some(thread_id),
            id: 7,
            decision: "once".into(),
        }
    });
    assert_cross_process_runtime_handoff_keeps_provisioned_thread(|thread_id| {
        ClientRequest::InteractionAnswer {
            thread_id: Some(thread_id),
            id: 8,
            answer: Some("keep the provisioned thread".into()),
        }
    });
}

#[test]
fn failed_runtime_fork_removes_target_thread_context_and_goal() {
    let server = server();
    let source = ThreadId::new("fork-source").unwrap();
    let target = ThreadId::new("fork-rollback").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(source.clone()),
            workspace: "workspace".into(),
            title: "source".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    server
        .dispatch(ClientRequest::ThreadModelContextReplace {
            thread_id: source.clone(),
            messages: vec![serde_json::json!({"role":"assistant","content":"seed"})],
        })
        .unwrap();
    server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: source.clone(),
            objective: "finish fork rollback".into(),
            max_goal_rounds: 3,
        })
        .unwrap();

    let rejected = RecordingRuntime {
        fail_fork_thread: true,
        ..Default::default()
    };
    assert!(server
        .dispatch_with_runtime(
            ClientRequest::ThreadForkActivate {
                thread_id: source,
                new_thread_id: target.clone(),
            },
            &rejected,
        )
        .is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: target.clone(),
        })
        .is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadModelContextRead {
            thread_id: target.clone(),
        })
        .is_err());
    assert!(server
        .dispatch(ClientRequest::GoalRead { thread_id: target })
        .is_err());
}

#[test]
fn pending_runtime_fork_rejects_activation_of_the_new_target() {
    let server = Arc::new(server());
    let source = ThreadId::new("fork-activation-source").unwrap();
    let target = ThreadId::new("fork-activation-target").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(source.clone()),
            workspace: "workspace".into(),
            title: "source".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let gate = Arc::new(ProfileValidationGate::default());
    let rejected = Arc::new(RecordingRuntime {
        fork_activation_gate: Some(gate.clone()),
        fail_fork_thread: true,
        ..Default::default()
    });
    let pending_server = server.clone();
    let pending_runtime = rejected.clone();
    let pending_source = source.clone();
    let pending_target = target.clone();
    let pending = std::thread::spawn(move || {
        pending_server.dispatch_with_runtime(
            ClientRequest::ThreadForkActivate {
                thread_id: pending_source,
                new_thread_id: pending_target,
            },
            pending_runtime.as_ref(),
        )
    });

    gate.wait_until_entered();
    let activation_error = server
        .dispatch_with_runtime(
            ClientRequest::ThreadActivate {
                thread_id: target.clone(),
            },
            &RecordingRuntime::default(),
        )
        .unwrap_err();
    assert!(matches!(
        activation_error,
        AppServerError::InvalidRequest(ref message)
            if message.contains("activation is still in progress")
    ));

    gate.release();
    assert!(pending.join().unwrap().is_err());
    assert!(server
        .dispatch(ClientRequest::ThreadRead { thread_id: target })
        .is_err());
}

#[test]
fn harness_profile_uses_the_last_serialized_selection_before_the_first_turn() {
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
    assert_eq!(
        runtime.validated_profiles.lock().unwrap().as_slice(),
        &[
            ("full".into(), "workspace".into()),
            ("coding".into(), "workspace".into()),
        ]
    );
    // The GUI serializes rapid selections. Verify the durable endpoint is
    // last-write-wins in that request order, so A -> B persists B.
    server
        .dispatch_with_runtime(
            ClientRequest::ThreadHarnessProfileSet {
                thread_id: thread_id.clone(),
                harness_profile: "minimal".into(),
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
        matches!(read.response.payload, ResponsePayload::Thread(ref thread) if thread.metadata.harness_profile == "minimal")
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
                harness_profile: "readonly".into(),
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
    let thread_id = ThreadId::new("runtime-thread").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "runtime".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
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
                thread_id,
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
            "mode:runtime-thread:plan",
            "preset:deepseek:deepseek-v4",
        ]
    );
}

#[test]
fn permission_mode_requires_a_durable_thread_target() {
    let server = server();
    let runtime = RecordingRuntime::default();

    assert!(server
        .dispatch_with_runtime(
            ClientRequest::RuntimePermissionModeSet {
                thread_id: ThreadId::new("missing-permission-target").unwrap(),
                mode: "plan".into(),
            },
            &runtime,
        )
        .is_err());
    assert!(runtime.calls.lock().unwrap().is_empty());
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
