use super::*;

pub(crate) async fn run(args: Args) -> i32 {
    let cfg = match load_cli_config(&args) {
        Ok(cfg) => cfg,
        Err(error) => {
            eprintln!("ncx: config error: {error}");
            return 1;
        }
    };
    if let Some(exit) = early_exit(&cfg, &args).await {
        return exit;
    }
    let (runtime, mut tools, system_prompt, base_prompt) = match build_registry(&cfg, &args) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ncx: {error}");
            return 1;
        }
    };
    if args.dump_genome {
        print!(
            "{}",
            dump_genome_toml(&base_prompt, &tools.ctx.tool_catalog.borrow())
        );
        return 0;
    }
    let mut mcp_tool_names = match attach_mcp(&cfg, args.mcp, &mut tools).await {
        Ok(names) => names,
        Err(error) => {
            eprintln!("mcp: {error}");
            return 1;
        }
    };
    let (mut agent, mut recorder) = match open_agent(&cfg, &args, &runtime, tools, system_prompt) {
        Ok(started) => started,
        Err(error) => {
            eprintln!("ncx: {error}");
            return 1;
        }
    };
    if let Some(prompt) = &args.prompt {
        return run_prompt(&args, cfg, prompt, &mut agent, &mut recorder).await;
    }
    repl(&mut agent, &cfg, &mut recorder, &mut mcp_tool_names).await;
    0
}

fn load_cli_config(args: &Args) -> Result<Config, String> {
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    load_config(Overrides {
        workspace: Some(workspace),
        model: args.model.clone(),
        sandbox_mode: args.sandbox.clone(),
        approval_policy: args.approval.clone(),
        max_iterations: args.max_iterations,
        max_tool_calls: args.max_tool_calls,
        max_parallel_tool_calls: args.max_parallel_tool_calls,
        context_edit_enabled: args.disable_context_edit.then_some(false),
        context_edit_max_chars: args.context_edit_max_chars,
        context_edit_keep_recent_messages: args.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: args.context_edit_max_tool_result_chars,
        profile: args.profile.clone(),
        ..Default::default()
    })
    .map_err(|error| error.to_string())
}

async fn early_exit(cfg: &Config, args: &Args) -> Option<i32> {
    if args.history {
        return Some(match protocol_history(20) {
            Ok(history) => {
                println!("{history}");
                0
            }
            Err(error) => {
                eprintln!("ncx: history error: {error}");
                1
            }
        });
    }
    if let Err(error) = validate_cli_config(cfg, args) {
        eprintln!("ncx: {error}");
        return Some(1);
    }
    if !args.memory_merge {
        return None;
    }
    let memory = MemoryStore::new(cfg.workspace.join(".ncx").join("memory"));
    let summarizer = memory_summarizer(cfg);
    Some(
        match memory.summarize_consolidate(&summarizer, 0.85).await {
            Ok(count) => {
                println!("memory: folded {count} near-duplicate note(s) via the LLM.");
                0
            }
            Err(error) => {
                eprintln!("memory merge failed: {error}");
                1
            }
        },
    )
}

fn validate_cli_config(cfg: &Config, args: &Args) -> Result<(), String> {
    cfg.validate().map_err(|error| error.to_string())?;
    if let Some(mode) = args.permission_mode.as_deref() {
        if !VALID_PERMISSION_MODES.contains(&mode) {
            return Err(format!(
                "invalid --permission-mode {mode:?}; expected one of {VALID_PERMISSION_MODES:?}"
            ));
        }
    }
    Ok(())
}

fn build_registry(
    cfg: &Config,
    args: &Args,
) -> Result<
    (
        ConfiguredHarnessRuntime,
        ncx_core::ToolRegistry,
        String,
        String,
    ),
    String,
> {
    let runtime_profile = runtime_profile_for_args(cfg, args);
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    let _ = memory.consolidate(0.85);
    let genome = Genome::from_env();
    if !genome.is_empty() {
        eprintln!(
            "[ncx] NCX_GENOME active: system_prompt={}, tool_desc overrides={}",
            genome.system_prompt.is_some(),
            genome.tool_desc.len()
        );
    }
    let base_prompt = genome.base_system_prompt(SYSTEM_PROMPT).to_string();
    let plan_note = if runtime_profile.permissions.plan_mode {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let mut hooks = cfg.hooks.clone();
    hooks.extend(
        discover_codex_hooks(&cfg.workspace)
            .map_err(|error| format!("Codex 插件 Hooks 加载失败: {error}"))?,
    );
    let sources = RuntimeContextSources::new(
        load_project_instructions(&cfg.workspace, 16_000),
        discover_skills(&cfg.workspace),
        plan_note,
    )
    .with_memory(memory)
    .with_hooks(hooks)
    .with_genome(genome);
    let runtime = ConfiguredHarnessRuntime::new(cfg.clone(), cfg.model.clone(), runtime_profile);
    let tools = runtime
        .build_tools(
            cfg.workspace.clone(),
            sources,
            RuntimeHostBindings::default(),
        )
        .map_err(|error| format!("Harness 配置错误: {error}"))?;
    let system_prompt = tools
        .service::<ContextServiceDescriptor>("context")
        .ok_or_else(|| "Harness Context 服务未启用".to_string())?
        .assemble(base_prompt.clone());
    Ok((runtime, tools, system_prompt, base_prompt))
}

async fn attach_mcp(
    cfg: &Config,
    enabled: bool,
    tools: &mut ncx_core::ToolRegistry,
) -> Result<Vec<String>, String> {
    if !enabled {
        return Ok(Vec::new());
    }
    let mut servers = load_mcp_servers();
    let plugin_servers = discover_codex_mcp_servers(&cfg.workspace)
        .map_err(|error| format!("Codex 插件资源加载失败: {error}"))?;
    servers.extend(plugin_servers);
    if servers.is_empty() {
        eprintln!("mcp: --mcp set but no servers found in ~/.nanocodex/mcp.toml");
    }
    let prepared = prepare_configured_mcp_tools(&servers).await;
    report_mcp_server_failures(&prepared.failures);
    if !servers.is_empty() && prepared.successful_servers == 0 {
        eprintln!(
            "mcp: all {} configured server(s) failed; continuing without MCP tools",
            servers.len()
        );
        return Ok(Vec::new());
    }
    match tools.replace_tools(&[], prepared.tools) {
        Ok(names) => {
            tools.replace_service(
                "mcp",
                Rc::new(McpServiceDescriptor {
                    enabled: true,
                    configured_servers: servers.len(),
                    active_tools: names.len(),
                }),
            );
            eprintln!(
                "mcp: {} server(s), {} tool(s) registered",
                servers.len(),
                names.len()
            );
            Ok(names)
        }
        Err(error) => {
            eprintln!("mcp: registration rejected: {error}");
            Ok(Vec::new())
        }
    }
}

fn open_agent(
    cfg: &Config,
    args: &Args,
    runtime: &ConfiguredHarnessRuntime,
    tools: ncx_core::ToolRegistry,
    system_prompt: String,
) -> Result<(AgentLoop, SessionRecorder), String> {
    let recorder = SessionRecorder::open(cfg.workspace.clone(), args.resume)
        .map_err(|error| format!("thread store error: {error}"))?;
    let log_path = recorder.log_path();
    let session = match recorder.model_context() {
        Some(messages) => Session::fork(system_prompt, messages, Some(log_path)),
        None => Session::with_log(system_prompt, Some(log_path)),
    };
    let restored_count = session.restored_count;
    let agent = runtime
        .profile()
        .clone()
        .apply(AgentLoop::from_runtime_services(tools, session)?);
    if args.resume {
        if restored_count > 0 {
            eprintln!("resumed {restored_count} message(s) from the workspace session log.");
        } else {
            eprintln!("no previous workspace session log found; starting fresh.");
        }
    }
    Ok((agent, recorder))
}

async fn run_prompt(
    args: &Args,
    cfg: Config,
    prompt: &str,
    agent: &mut AgentLoop,
    recorder: &mut SessionRecorder,
) -> i32 {
    let expanded = expand_file_mentions(prompt, &cfg.workspace);
    checkpoint_before_turn(&cfg.workspace, &expanded);
    if args.orchestrate {
        if !args.images.is_empty() {
            eprintln!("ncx: --image is ignored with --orchestrate (text-only path).");
        }
        return run_orchestrated(cfg, &expanded, recorder).await;
    }
    if let Err(error) = validate_attachments(&agent.tools, &args.images) {
        eprintln!("ncx: {error}");
        return 1;
    }
    let user_input = match build_image_user_input(&expanded, &args.images) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("ncx: {error}");
            return 1;
        }
    };
    let turn_id = match recorder.start_turn(&expanded) {
        Ok(turn_id) => turn_id,
        Err(error) => {
            eprintln!("ncx: cannot start turn: {error}");
            return 1;
        }
    };
    let result = agent.run_turn(user_input, None).await;
    if let Err(error) = recorder.finish_turn(&turn_id, &result, agent) {
        eprintln!("ncx: cannot persist turn: {error}");
        return 1;
    }
    println!("{}", result.final_text);
    emit_usage_line(&result.usage);
    i32::from(result.stop_reason == "error")
}
