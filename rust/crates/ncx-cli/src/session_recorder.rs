use super::*;

pub(crate) struct SessionRecorder {
    pub(crate) server: AppServer<JsonThreadStore>,
    pub(crate) thread_id: ThreadId,
    workspace: PathBuf,
    model_context: Option<Vec<Value>>,
}

impl SessionRecorder {
    pub(crate) fn open(workspace: PathBuf, resume: bool) -> Result<Self, String> {
        Self::open_at(workspace, resume, default_thread_store_path())
    }

    pub(crate) fn open_at(
        workspace: PathBuf,
        resume: bool,
        store_path: PathBuf,
    ) -> Result<Self, String> {
        let store = Arc::new(JsonThreadStore::open(store_path).map_err(|e| e.to_string())?);
        let server = AppServer::new(store, now_epoch_millis);
        let workspace_text = workspace.display().to_string();
        let existing = if resume {
            match server
                .dispatch(ClientRequest::ThreadList {
                    include_archived: false,
                })
                .map_err(|e| e.to_string())?
                .response
                .payload
            {
                ResponsePayload::Threads(threads) => threads
                    .into_iter()
                    .find(|metadata| metadata.workspace == workspace_text),
                _ => None,
            }
        } else {
            None
        };
        let thread_id = if let Some(metadata) = existing {
            metadata.id
        } else {
            let thread_id = ThreadId::new(new_session_id()).map_err(|e| e.to_string())?;
            server
                .dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(thread_id.clone()),
                    workspace: workspace_text,
                    title: "(no prompt yet)".to_string(),
                })
                .map_err(|e| e.to_string())?;
            thread_id
        };
        let model_context = match server
            .dispatch(ClientRequest::ThreadModelContextRead {
                thread_id: thread_id.clone(),
            })
            .map_err(|e| e.to_string())?
            .response
            .payload
        {
            ResponsePayload::ModelContext(Some(context)) => Some(context.messages),
            _ if resume => Some(read_protocol_messages(&server, &thread_id)?),
            _ => None,
        };
        Ok(Self {
            server,
            thread_id,
            workspace,
            model_context,
        })
    }

    pub(crate) fn model_context(&self) -> Option<Vec<Value>> {
        self.model_context
            .clone()
            .filter(|messages| !messages.is_empty())
    }

    pub(crate) fn log_path(&self) -> PathBuf {
        self.workspace
            .join(".nanocodex")
            .join("sessions")
            .join(format!(
                "{}.jsonl",
                safe_thread_file_stem(self.thread_id.as_str())
            ))
    }

    pub(crate) fn start_turn(&mut self, user_text: &str) -> Result<TurnId, String> {
        let turn_id =
            TurnId::new(format!("turn-{}", new_session_id())).map_err(|error| error.to_string())?;
        self.server
            .dispatch(ClientRequest::TurnStart {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::UserMessage {
                    id: ItemId::new(format!("user-{}", new_session_id()))
                        .map_err(|error| error.to_string())?,
                    text: user_text.to_string(),
                },
            })
            .map_err(|error| error.to_string())?;
        let current = self
            .server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: self.thread_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        if matches!(current.response.payload, ResponsePayload::Thread(Thread { metadata: ThreadMetadata { ref title, .. }, .. }) if title == "(no prompt yet)")
        {
            self.server
                .dispatch(ClientRequest::ThreadRename {
                    thread_id: self.thread_id.clone(),
                    title: clipped_label(user_text, 80),
                })
                .map_err(|error| error.to_string())?;
        }
        Ok(turn_id)
    }

    pub(crate) fn session_id(&self) -> &str {
        self.thread_id.as_str()
    }

    pub(crate) fn finish_turn(
        &mut self,
        turn_id: &TurnId,
        result: &TurnResult,
        agent: &AgentLoop,
    ) -> Result<(), String> {
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::AssistantMessage {
                    id: ItemId::new(format!("assistant-{}", new_session_id()))
                        .map_err(|error| error.to_string())?,
                    text: result.final_text.clone(),
                },
            })
            .map_err(|error| error.to_string())?;
        let messages = agent.session.full_messages();
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.model_context = Some(messages);
        let estimated = agent.estimated_cost(result);
        let (currency, estimated_cost) = estimated
            .map(|(currency, amount)| (Some(currency), Some(amount)))
            .unwrap_or((None, None));
        let status = if result.stop_reason == "error" {
            TurnStatus::Failed
        } else {
            TurnStatus::Completed
        };
        self.server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                status,
                error: (status == TurnStatus::Failed).then(|| result.final_text.clone()),
                usage: ProtocolTurnUsage {
                    tokens: result.usage.clone(),
                    estimated_cost,
                    currency,
                },
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn replace_model_context(&mut self, session: &Session) -> Result<(), String> {
        let messages = session.full_messages();
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.model_context = Some(messages);
        Ok(())
    }

    pub(crate) fn finish_external_turn(
        &mut self,
        turn_id: &TurnId,
        user_text: &str,
        final_text: &str,
        status: TurnStatus,
        error: Option<String>,
    ) -> Result<(), String> {
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::AssistantMessage {
                    id: ItemId::new(format!("assistant-{}", new_session_id()))
                        .map_err(|failure| failure.to_string())?,
                    text: final_text.to_string(),
                },
            })
            .map_err(|failure| failure.to_string())?;
        let mut messages = self.model_context.take().unwrap_or_default();
        messages.push(json!({"role": "user", "content": user_text}));
        messages.push(json!({"role": "assistant", "content": final_text}));
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|failure| failure.to_string())?;
        self.model_context = Some(messages);
        self.server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                status,
                error,
                usage: ProtocolTurnUsage::default(),
            })
            .map_err(|failure| failure.to_string())?;
        Ok(())
    }
}

fn safe_thread_file_stem(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn now_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// Print a stable, parseable token-usage line to stderr (one-shot mode).
/// Format: `[ncx-usage] prompt_tokens=P completion_tokens=C total_tokens=T`.
/// `total_tokens` is P+C (the provider does not report a total directly).
pub(crate) fn emit_usage_line(usage: &std::collections::BTreeMap<String, i64>) {
    let prompt = usage.get("prompt_tokens").copied().unwrap_or(0);
    let completion = usage.get("completion_tokens").copied().unwrap_or(0);
    eprintln!(
        "[ncx-usage] prompt_tokens={prompt} completion_tokens={completion} total_tokens={}",
        prompt + completion
    );
}

pub(crate) fn protocol_history(limit: usize) -> Result<String, String> {
    let store =
        Arc::new(JsonThreadStore::open(default_thread_store_path()).map_err(|e| e.to_string())?);
    let server = AppServer::new(store, now_epoch_millis);
    let entries = match server
        .dispatch(ClientRequest::ThreadList {
            include_archived: false,
        })
        .map_err(|e| e.to_string())?
        .response
        .payload
    {
        ResponsePayload::Threads(entries) => entries,
        _ => return Err("threadList returned an unexpected response".to_string()),
    };
    Ok(render_history(&entries, limit))
}

pub(crate) fn render_history(entries: &[ThreadMetadata], limit: usize) -> String {
    if entries.is_empty() {
        return "No saved sessions.".into();
    }
    let mut out = String::from("Saved sessions:");
    for summary in entries.iter().take(limit) {
        let title = if summary.title.trim().is_empty() {
            "(no prompt yet)"
        } else {
            summary.title.as_str()
        };
        out.push_str(&format!(
            "\n  {}  {}  {}",
            summary.updated_at, summary.id, title,
        ));
    }
    out
}

fn read_protocol_messages(
    server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
) -> Result<Vec<Value>, String> {
    let thread = match server
        .dispatch(ClientRequest::ThreadReadVisible {
            thread_id: thread_id.clone(),
        })
        .map_err(|error| error.to_string())?
        .response
        .payload
    {
        ResponsePayload::Thread(thread) => thread,
        _ => return Err("threadReadVisible returned an unexpected response".to_string()),
    };
    Ok(thread
        .turns
        .into_iter()
        .flat_map(|turn| turn.items)
        .filter_map(|item| match item {
            ThreadItem::UserMessage { text, .. } => Some(json!({"role": "user", "content": text})),
            ThreadItem::AssistantMessage { text, .. } => {
                Some(json!({"role": "assistant", "content": text}))
            }
            _ => None,
        })
        .collect())
}
