use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    app: AppHandle,
    app_server: Arc<AppServer<JsonThreadStore>>,
    session_grants: GrantRegistry,
    session_id: String,
    workspace: PathBuf,
    messages: Vec<Value>,
    text: String,
    images: Vec<String>,
    cancel: CancelFlag,
    approver: Rc<dyn ApprovalHandler>,
    questioner: Rc<dyn UserQuestionHandler>,
    grants: Rc<RefCell<SessionGrants>>,
    protocol_turn: &mut ProtocolTurnGuard,
    harness_profile: String,
) {
    if !images.is_empty() {
        fail(
            app,
            protocol_turn,
            &session_id,
            "多 Agent 模式暂不接受图片附件；请切换到普通 Agent，或移除附件后重试。".into(),
        );
        return;
    }
    let expanded = expand_file_mentions(&text, &workspace);
    save_auto_checkpoint(&workspace, &expanded);
    let cfg = match load_config(Overrides {
        workspace: Some(workspace.clone()),
        ..Default::default()
    })
    .and_then(|config| {
        config.validate()?;
        Ok(config)
    }) {
        Ok(config) => config,
        Err(error) => {
            fail(app, protocol_turn, &session_id, error.to_string());
            return;
        }
    };
    let bindings = RuntimeHostBindings {
        approver: Some(approver),
        questioner: Some(questioner),
        grants: Some(grants.clone()),
        goal_service: None,
    };
    let cancel_check = cancel.clone();
    let activity_app = app.clone();
    let activity_session = session_id.clone();
    let observer = Rc::new(move |event: HarnessRunnerEvent| {
        let (worker, tool, phase, failure) = match event {
            HarnessRunnerEvent::WorkerToolStarted { worker, tool } => {
                (worker, tool, "started".to_string(), None)
            }
            HarnessRunnerEvent::WorkerToolFinished {
                worker,
                tool,
                failure,
            } => (worker, tool, "finished".to_string(), failure),
        };
        emit(
            &activity_app,
            UiEvent::OrchestratorActivity {
                session_id: activity_session.clone(),
                worker: worker + 1,
                tool,
                phase,
                failure,
            },
        );
    });
    let runner = HarnessAgentRunner::new(cfg.clone())
        .with_harness_profile(harness_profile)
        .with_bindings(bindings)
        .with_cancel(Rc::new(move || cancel_check.load(Ordering::Acquire)))
        .with_observer(observer);
    let control = GuiOrchestratorControl {
        app: app.clone(),
        session_id: session_id.clone(),
        cancel,
    };
    let is_first_turn = messages_have_no_user(&messages);
    let task = contextual_task(&messages, &expanded);
    let outcome = Orchestrator::new(&runner, OrchestratorConfig::from_runtime_config(&cfg))
        .with_control(&control)
        .handle(&task)
        .await;
    if let Ok(mut registry) = session_grants.lock() {
        registry.insert(session_id.clone(), grants.borrow().clone());
    }
    let stop_reason = if outcome.cancelled {
        "cancelled"
    } else if outcome.verify_passed {
        "completed"
    } else {
        "orchestrator verification failed"
    };
    let model = outcome
        .telemetry
        .requested_models
        .last()
        .cloned()
        .unwrap_or_else(|| cfg.model.clone());
    let confirmed_model = outcome.telemetry.confirmed_models.last().cloned();
    if !outcome.final_text.trim().is_empty() {
        append_answer(
            &app,
            app_server.as_ref(),
            protocol_turn,
            &session_id,
            &outcome.final_text,
            &model,
            confirmed_model.clone(),
        );
    }
    emit(
        &app,
        UiEvent::Done {
            session_id: session_id.clone(),
            final_text: outcome.final_text.clone(),
            stop_reason: stop_reason.into(),
            usage: serde_json::to_value(&outcome.telemetry.usage).unwrap_or(Value::Null),
        },
    );
    let mut context = messages;
    context.push(json!({"role":"user","content":expanded}));
    context.push(json!({"role":"assistant","content":outcome.final_text,"_ncx_model":model,"_ncx_confirmed_model":confirmed_model}));
    if let Ok(result) = app_server.dispatch(ClientRequest::ThreadModelContextReplace {
        thread_id: protocol_turn.thread_id.clone(),
        messages: context,
    }) {
        emit_protocol_outcome(&app, &result);
    }
    let estimated_cost = orchestration_cost(&cfg, &outcome.telemetry.usage);
    protocol_turn.complete_with_usage(
        stop_reason,
        TurnUsage {
            tokens: outcome.telemetry.usage,
            estimated_cost,
            currency: estimated_cost.map(|_| cfg.price_currency.clone()),
        },
    );
    if should_generate_session_title(is_first_turn, stop_reason) {
        spawn_title_generation(app, app_server, session_id, workspace, text);
    }
}

fn append_answer(
    app: &AppHandle,
    server: &AppServer<JsonThreadStore>,
    turn: &ProtocolTurnGuard,
    session_id: &str,
    text: &str,
    model: &str,
    confirmed_model: Option<String>,
) {
    let item = ThreadItem::AssistantMessage {
        id: protocol_item_id("assistant"),
        text: text.into(),
        model: Some(model.into()),
        confirmed_model: confirmed_model.clone(),
    };
    if let Ok(outcome) = server.dispatch(ClientRequest::ItemAppend {
        thread_id: turn.thread_id.clone(),
        turn_id: turn.turn_id.clone(),
        item,
    }) {
        emit_protocol_outcome(app, &outcome);
    }
    emit(
        app,
        UiEvent::Assistant {
            session_id: session_id.into(),
            text: text.into(),
            model: model.into(),
            confirmed_model,
        },
    );
}

fn contextual_task(messages: &[Value], current: &str) -> String {
    let history = messages
        .iter()
        .rev()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?;
            if !matches!(role, "user" | "assistant") {
                return None;
            }
            let content = message.get("content")?.as_str()?.trim();
            (!content.is_empty()).then(|| format!("{role}: {content}"))
        })
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if history.is_empty() {
        current.into()
    } else {
        format!("Conversation context (preserve intent; do not repeat completed work):\n{history}\n\nCurrent task:\n{current}")
    }
}

fn orchestration_cost(
    cfg: &ncx_config::Config,
    usage: &std::collections::BTreeMap<String, i64>,
) -> Option<f64> {
    if cfg.price_in <= 0.0 && cfg.price_out <= 0.0 {
        return None;
    }
    let input = *usage.get("prompt_tokens").unwrap_or(&0) as f64;
    let output = *usage.get("completion_tokens").unwrap_or(&0) as f64;
    Some((input * cfg.price_in + output * cfg.price_out) / 1_000_000.0)
}

fn messages_have_no_user(messages: &[Value]) -> bool {
    messages
        .iter()
        .all(|message| message.get("role").and_then(Value::as_str) != Some("user"))
}

fn fail(app: AppHandle, turn: &mut ProtocolTurnGuard, session_id: &str, message: String) {
    emit(
        &app,
        UiEvent::Error {
            session_id: session_id.into(),
            message: message.clone(),
        },
    );
    turn.complete(&message);
}
