use super::*;
use ncx_app_server::{GoalRoundDriveOutcome, GoalRoundDriver};
use ncx_protocol::{GoalRef, TurnUsage};

pub(super) async fn run(
    app: AppHandle,
    app_server: Arc<AppServer<JsonThreadStore>>,
    session_grants: GrantRegistry,
    session_id: String,
    workspace: PathBuf,
    cancel: CancelFlag,
    approver: Rc<dyn ApprovalHandler>,
    questioner: Rc<dyn UserQuestionHandler>,
) {
    let Ok(thread_id) = ThreadId::new(session_id.clone()) else {
        emit_safe_error(&app, &session_id, "长期目标会话标识无效。");
        return;
    };
    let started = true;
    let mut failed = false;
    let mut final_text = String::new();
    let mut aggregate_usage = std::collections::BTreeMap::new();
    let harness_profile = protocol_thread_profile(&app_server, &session_id);
    emit(
        &app,
        UiEvent::GoalRunStarted {
            session_id: session_id.clone(),
        },
    );

    loop {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        let turn_id = TurnId::new(format!("goal-turn-{}", new_session_id()))
            .expect("generated goal turn id is non-empty");
        let reserved = GoalRoundDriver::new(app_server.as_ref()).reserve_next(
            &thread_id,
            turn_id.clone(),
            || durable_checkpoint(app_server.as_ref(), &thread_id),
        );
        let (goal, prompt) = match reserved {
            Ok(GoalRoundDriveOutcome::Reserved { goal, prompt, .. }) => (goal, prompt),
            Ok(GoalRoundDriveOutcome::Idle | GoalRoundDriveOutcome::CompetingTurn) => break,
            Ok(GoalRoundDriveOutcome::Blocked(_)) => {
                final_text = "长期目标已达到自动续轮上限。".into();
                break;
            }
            Err(_) => {
                failed = true;
                emit_safe_error(
                    &app,
                    &session_id,
                    "长期目标续轮前的持久化检查失败，自动续轮已关闭。",
                );
                break;
            }
        };
        let messages = goal_seed_messages(&app_server, &thread_id, &turn_id);
        let initial_grants = session_grants
            .lock()
            .ok()
            .and_then(|registry| registry.get(&session_id).cloned())
            .unwrap_or_default();
        let grants = Rc::new(RefCell::new(initial_grants));
        let built = build_agent(
            approver.clone(),
            questioner.clone(),
            Some((session_id.clone(), messages)),
            grants.clone(),
            workspace.clone(),
            Some(harness_profile.clone()),
            app_server.clone(),
        )
        .await;
        let (mut agent, _, _, _) = match built {
            Ok(value) => value,
            Err(_) => {
                failed = true;
                fail_reserved(
                    &app_server,
                    &thread_id,
                    turn_id,
                    &goal,
                    "worker-build-failed",
                );
                emit_safe_error(
                    &app,
                    &session_id,
                    "长期目标执行环境启动失败，自动续轮已关闭。",
                );
                break;
            }
        };
        agent.set_event_sink(make_sink(
            app.clone(),
            session_id.clone(),
            Some(app_server.clone()),
            Some(turn_id.clone()),
        ));
        let is_cancelled = || cancel.load(Ordering::Acquire);
        let round = goal.goal.rounds_started;
        let result = agent
            .run_goal_round(
                json!(prompt),
                Some(&is_cancelled),
                goal.goal.id.clone(),
                goal.goal.revision,
                round,
            )
            .await;
        let round_estimated_cost = agent.estimated_cost(&result);
        final_text = result.final_text.clone();
        merge_usage(&mut aggregate_usage, &result.usage);
        if let Ok(mut registry) = session_grants.lock() {
            registry.insert(session_id.clone(), grants.borrow().clone());
        }

        if cancel.load(Ordering::Acquire)
            || matches!(result.stop_reason.as_str(), "cancelled" | "canceled")
        {
            let _ = GoalRoundDriver::new(app_server.as_ref()).cancel_reserved(
                &thread_id,
                turn_id,
                goal_ref(&goal),
            );
            break;
        }
        if result.stop_reason != "completed" {
            failed = true;
            fail_reserved(&app_server, &thread_id, turn_id, &goal, "round-failed");
            emit_safe_error(
                &app,
                &session_id,
                "长期目标本轮未正常完成，自动续轮已关闭。",
            );
            break;
        }
        if app_server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: thread_id.clone(),
                messages: agent.session.messages.clone(),
            })
            .is_err()
        {
            failed = true;
            fail_reserved(&app_server, &thread_id, turn_id, &goal, "checkpoint-failed");
            emit_safe_error(
                &app,
                &session_id,
                "长期目标模型上下文保存失败，自动续轮已关闭。",
            );
            break;
        }
        if let Ok(outcome) = app_server.dispatch(ClientRequest::TurnComplete {
            thread_id: thread_id.clone(),
            turn_id,
            status: TurnStatus::Completed,
            error: None,
            usage: TurnUsage {
                tokens: result.usage,
                estimated_cost: round_estimated_cost.as_ref().map(|(_, cost)| *cost),
                currency: round_estimated_cost.map(|(currency, _)| currency),
            },
        }) {
            emit_protocol_outcome(&app, &outcome);
        } else {
            failed = true;
            let _ = app_server.disarm_goal(&thread_id);
            emit_safe_error(
                &app,
                &session_id,
                "长期目标轮次状态保存失败，自动续轮已关闭。",
            );
            break;
        }
    }

    if started && !failed {
        emit(
            &app,
            UiEvent::Done {
                session_id,
                final_text,
                stop_reason: if cancel.load(Ordering::Acquire) {
                    "cancelled".into()
                } else {
                    "completed".into()
                },
                usage: serde_json::to_value(aggregate_usage).unwrap_or(Value::Null),
            },
        );
    }
}

fn durable_checkpoint(
    app_server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
) -> Result<(), String> {
    let thread = app_server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .map_err(|_| "thread checkpoint unavailable".to_string())?;
    let ResponsePayload::Thread(thread) = thread.response.payload else {
        return Err("thread checkpoint returned another payload".into());
    };
    if thread
        .turns
        .last()
        .is_some_and(|turn| turn.status == TurnStatus::Running)
    {
        return Err("previous turn is still running".into());
    }
    app_server
        .dispatch(ClientRequest::ThreadModelContextRead {
            thread_id: thread_id.clone(),
        })
        .map(|_| ())
        .map_err(|_| "model context checkpoint unavailable".to_string())
}

fn goal_seed_messages(
    app_server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
    current_turn: &TurnId,
) -> Vec<Value> {
    if let Ok(outcome) = app_server.dispatch(ClientRequest::ThreadModelContextRead {
        thread_id: thread_id.clone(),
    }) {
        if let ResponsePayload::ModelContext(Some(context)) = outcome.response.payload {
            return context.messages;
        }
    }
    app_server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .ok()
        .and_then(|outcome| match outcome.response.payload {
            ResponsePayload::Thread(mut thread) => {
                thread.turns.retain(|turn| &turn.id != current_turn);
                Some(protocol_thread_messages(&thread, false))
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn goal_ref(goal: &ncx_protocol::GoalView) -> GoalRef {
    GoalRef {
        id: goal.goal.id.clone(),
        revision: goal.goal.revision,
    }
}

fn fail_reserved(
    app_server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
    turn_id: TurnId,
    goal: &ncx_protocol::GoalView,
    code: &str,
) {
    let _ = GoalRoundDriver::new(app_server).fail_reserved(
        thread_id,
        turn_id,
        goal_ref(goal),
        code,
        "The local automatic Goal worker stopped before the round could be committed.",
    );
}

fn merge_usage(
    aggregate: &mut std::collections::BTreeMap<String, i64>,
    current: &std::collections::BTreeMap<String, i64>,
) {
    for (key, value) in current {
        *aggregate.entry(key.clone()).or_default() += value;
    }
}

fn emit_safe_error(app: &AppHandle, session_id: &str, message: &str) {
    emit(
        app,
        UiEvent::Error {
            session_id: session_id.to_string(),
            message: message.to_string(),
        },
    );
}
