use super::*;
use crate::test_support::unique_temp_dir;
use std::fs;

fn empty_env() -> HashMap<String, String> {
    HashMap::new()
}

fn env1(k: &str, v: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(k.to_string(), v.to_string());
    m
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn no_paths(tmp: &Path) -> ConfigPaths {
    ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: tmp.join("nope-nano.toml"),
    }
}

#[test]
fn active_provider_route_replaces_all_stale_flat_connection_fields() {
    let tmp = unique_temp_dir("ncx_config_test_active_provider_route");
    let paths = ConfigPaths {
        nanocodex: tmp.join("config.toml"),
        ..no_paths(&tmp)
    };
    write(
        &paths.nanocodex,
        concat!(
            "active_provider_id = \"aigo\"\n",
            "api_key = \"stale-yunmo-key\"\n",
            "base_url = \"https://api.yunmo-ai.com/v1\"\n",
            "provider_protocol = \"anthropic\"\n",
            "model = \"stale-model\"\n",
        ),
    );
    write(
        &paths.nanocodex.with_file_name("providers.json"),
        r#"[{"id":"aigo","protocol":"openai","base_url":"https://api.aigocode.app/v1","api_key":"route-key","models":["gpt-5.6-sol","gpt-5.6-terra"],"selected_model":"gpt-5.6-sol"}]"#,
    );

    let cfg = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(cfg.active_provider_id, "aigo");
    assert_eq!(cfg.api_key, "route-key");
    assert_eq!(cfg.base_url, "https://api.aigocode.app/v1");
    assert_eq!(cfg.provider_protocol, "openai");
    assert_eq!(cfg.model, "gpt-5.6-sol");
    assert_eq!(cfg.available_models[0], "gpt-5.6-sol");
    assert!(cfg.available_models.contains(&"gpt-5.6-terra".to_string()));
}

#[test]
fn missing_active_provider_fails_instead_of_falling_back_to_stale_route() {
    let tmp = unique_temp_dir("ncx_config_test_missing_active_provider");
    let paths = ConfigPaths {
        nanocodex: tmp.join("config.toml"),
        ..no_paths(&tmp)
    };
    write(&paths.nanocodex, "active_provider_id = \"missing\"\n");
    write(&paths.nanocodex.with_file_name("providers.json"), "[]");

    let error = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap_err();
    assert!(error.to_string().contains("does not exist"));
}

#[test]
fn incomplete_active_provider_fails_without_exposing_credential() {
    let tmp = unique_temp_dir("ncx_config_test_incomplete_active_provider");
    let paths = ConfigPaths {
        nanocodex: tmp.join("config.toml"),
        ..no_paths(&tmp)
    };
    write(&paths.nanocodex, "active_provider_id = \"broken\"\n");
    write(
        &paths.nanocodex.with_file_name("providers.json"),
        r#"[{"id":"broken","protocol":"openai","base_url":"","api_key":"secret-route-key","models":[],"selected_model":""}]"#,
    );

    let error = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap_err();
    assert!(error.to_string().contains("is incomplete"));
    assert!(!error.to_string().contains("secret-route-key"));
}

#[test]
fn legacy_price_config_defaults_to_cny_and_explicit_usd_round_trips() {
    let tmp = unique_temp_dir("ncx_config_test_price_currency");
    let paths = no_paths(&tmp);
    write(&paths.nanocodex, "api_key = \"k\"\nprice_in = \"1.25\"\n");
    let legacy = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(legacy.price_currency, "CNY");

    write(
        &paths.nanocodex,
        "api_key = \"k\"\nprice_currency = \"USD\"\n",
    );
    let usd = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(usd.price_currency, "USD");
}

#[test]
fn config_redacts_api_key() {
    let cfg = Config {
        api_key: "sk-abcdef123456".into(),
        base_url: "u".into(),
        model: "m".into(),
        ..Config::default()
    };
    let red = cfg.redacted();
    assert_eq!(red["api_key"], "****3456");
    assert!(!red.values().any(|v| v.contains("abcdef")));
}

#[test]
fn validate_rejects_bad_sandbox_mode() {
    let cfg = Config {
        api_key: "k".into(),
        sandbox_mode: "banana".into(),
        ..Config::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("sandbox_mode"));
}

#[test]
fn validate_rejects_missing_key() {
    let cfg = Config::default();
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("API key"));
}

#[test]
fn compaction_defaults_on_with_1m_window() {
    let cfg = Config::default();
    assert!(cfg.context_token_budget > 0);
    assert_eq!(cfg.context_token_budget, 512_000);
    assert_eq!(cfg.context_window, 1_048_576);
}

#[test]
fn load_reads_deepseek_file() {
    let tmp = unique_temp_dir("ncx_config_test_deepseek");
    let ds = tmp.join("deepseek.toml");
    write(
        &ds,
        r#"
api_key = "sk-fromfile"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
sandbox_mode = "workspace-write"
approval_policy = "on-request"
"#,
    );
    let paths = ConfigPaths {
        deepseek: ds,
        codex: tmp.join("nope.toml"),
        nanocodex: tmp.join("nope-nano.toml"),
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    cfg.validate().unwrap();
    assert_eq!(cfg.api_key, "sk-fromfile");
    assert_eq!(cfg.base_url, "https://api.deepseek.com/beta");
    assert_eq!(cfg.model, "deepseek-v4-pro");
}

#[test]
fn overrides_win_over_file() {
    let tmp = unique_temp_dir("ncx_config_test_override");
    let ds = tmp.join("deepseek.toml");
    write(
        &ds,
        "api_key = \"k\"\ndefault_text_model = \"deepseek-v4-pro\"\n",
    );
    let paths = ConfigPaths {
        deepseek: ds,
        codex: tmp.join("nope.toml"),
        nanocodex: tmp.join("nope-nano.toml"),
    };
    let ovr = Overrides {
        workspace: Some(tmp.clone()),
        model: Some("deepseek-chat".into()),
        sandbox_mode: Some("read-only".into()),
        ..Default::default()
    };
    let cfg = load_config_impl(ovr, &paths, &empty_env()).unwrap();
    assert_eq!(cfg.model, "deepseek-chat");
    assert_eq!(cfg.sandbox_mode, "read-only");
}

#[test]
fn deepseek_nested_provider_key() {
    let tmp = unique_temp_dir("ncx_config_test_nested");
    let ds = tmp.join("deepseek.toml");
    write(
        &ds,
        "base_url = \"u\"\n[providers.deepseek]\napi_key = \"sk-nested\"\n",
    );
    let paths = ConfigPaths {
        deepseek: ds,
        codex: tmp.join("nope.toml"),
        nanocodex: tmp.join("nope-nano.toml"),
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    assert_eq!(cfg.api_key, "sk-nested");
}

#[test]
fn max_iterations_default_and_override() {
    let tmp = unique_temp_dir("ncx_config_test_maxiter");
    let paths = no_paths(&tmp);

    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    assert_eq!(cfg.max_iterations, 150);

    let cfg2 = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            max_iterations: Some(100),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    assert_eq!(cfg2.max_iterations, 100);
}

#[test]
fn orchestrator_budget_defaults_and_file_values() {
    let tmp = unique_temp_dir("ncx_config_test_orchestrator_budget");
    let paths = ConfigPaths {
        nanocodex: tmp.join("config.toml"),
        ..no_paths(&tmp)
    };
    write(
        &paths.nanocodex,
        concat!(
            "orchestrator_workers = 4\n",
            "orchestrator_high_workers = 5\n",
            "orchestrator_verify_retries = 2\n",
            "orchestrator_max_depth = 2\n",
            "orchestrator_max_subtasks = 10\n",
        ),
    );
    let cfg = load_config_impl(Overrides::default(), &paths, &empty_env()).unwrap();
    assert_eq!(cfg.orchestrator_workers, 4);
    assert_eq!(cfg.orchestrator_high_workers, 5);
    assert_eq!(cfg.orchestrator_verify_retries, 2);
    assert_eq!(cfg.orchestrator_max_depth, 2);
    assert_eq!(cfg.orchestrator_max_subtasks, 10);
}

#[test]
fn max_iterations_from_env() {
    let tmp = unique_temp_dir("ncx_config_test_maxiter_env");
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &no_paths(&tmp),
        &env1("NANOCODEX_MAX_ITERATIONS", "80"),
    )
    .unwrap();
    assert_eq!(cfg.max_iterations, 80);
}

#[test]
fn provider_protocol_can_be_isolated_by_the_host_environment() {
    let tmp = unique_temp_dir("ncx_config_test_provider_protocol_env");
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &no_paths(&tmp),
        &env1("NANOCODEX_PROVIDER_PROTOCOL", "openai"),
    )
    .unwrap();
    assert_eq!(cfg.provider_protocol, "openai");
}

#[test]
fn runtime_budget_and_context_edit_fields_load_from_file_env_and_overrides() {
    let tmp = unique_temp_dir("ncx_config_test_runtime_control");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        concat!(
            "api_key = \"sk-base\"\n",
            "max_tool_calls = 33\n",
            "max_parallel_tool_calls = 3\n",
            "context_edit_enabled = false\n",
            "context_edit_max_chars = 9000\n",
            "context_edit_keep_recent_messages = 11\n",
            "context_edit_max_tool_result_chars = 700\n",
        ),
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };
    let mut env = HashMap::new();
    env.insert("NANOCODEX_MAX_TOOL_CALLS".into(), "44".into());
    env.insert("NANOCODEX_MAX_PARALLEL_TOOL_CALLS".into(), "5".into());
    env.insert("NANOCODEX_CONTEXT_EDIT_ENABLED".into(), "true".into());

    let env_cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &env,
    )
    .unwrap();
    assert_eq!(env_cfg.max_parallel_tool_calls, 5);

    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            max_parallel_tool_calls: Some(7),
            context_edit_max_chars: Some(12_345),
            ..Default::default()
        },
        &paths,
        &env,
    )
    .unwrap();

    assert_eq!(cfg.max_tool_calls, 44);
    assert_eq!(cfg.max_parallel_tool_calls, 7);
    assert!(cfg.context_edit_enabled);
    assert_eq!(cfg.context_edit_max_chars, 12_345);
    assert_eq!(cfg.context_edit_keep_recent_messages, 11);
    assert_eq!(cfg.context_edit_max_tool_result_chars, 700);
}

#[test]
fn hooks_load_from_nanocodex_file() {
    let tmp = unique_temp_dir("ncx_config_test_hooks");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        r#"
api_key = "sk-base"

[[hooks]]
event = "pre_tool"
matcher = "shell|apply_patch"
command = "echo hook"
timeout_s = 3

[[hooks]]
event = "post_tool"
command = "echo post"
"#,
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp),
            ..Default::default()
        },
        &paths,
        &HashMap::new(),
    )
    .unwrap();

    assert_eq!(cfg.hooks.len(), 2);
    assert_eq!(cfg.hooks[0].matcher, "shell|apply_patch");
    assert_eq!(cfg.hooks[0].timeout_s, 3);
    assert_eq!(cfg.hooks[1].matcher, "*");
}

#[test]
fn hook_event_aliases_are_normalized() {
    let tmp = unique_temp_dir("ncx_config_test_hook_aliases");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        r#"
api_key = "sk-base"

[[hooks]]
event = "UserPromptSubmit"
command = "echo prompt"

[[hooks]]
event = "Stop"
command = "echo stop"
"#,
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp),
            ..Default::default()
        },
        &paths,
        &HashMap::new(),
    )
    .unwrap();

    assert_eq!(cfg.hooks[0].event, "user_prompt");
    assert_eq!(cfg.hooks[1].event, "stop");
    cfg.validate().unwrap();
}

#[test]
fn hook_missing_command_fails_validation() {
    let tmp = unique_temp_dir("ncx_config_test_hook_missing_command");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        r#"
api_key = "sk-base"

[[hooks]]
event = "pre_tool"
matcher = "shell"
"#,
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp),
            ..Default::default()
        },
        &paths,
        &HashMap::new(),
    )
    .unwrap();

    assert_eq!(cfg.hooks.len(), 1);
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("command must not be empty"));
}

#[test]
fn nanocodex_file_wins_over_deepseek() {
    let tmp = unique_temp_dir("ncx_config_test_nanowins");
    let ds = tmp.join("deepseek.toml");
    let nano = tmp.join("nano.toml");
    write(
        &ds,
        "api_key = \"sk-ds\"\ndefault_text_model = \"deepseek-v4-pro\"\n",
    );
    write(&nano, "api_key = \"sk-nano\"\nmodel = \"deepseek-chat\"\n");
    let paths = ConfigPaths {
        deepseek: ds,
        codex: tmp.join("nope.toml"),
        nanocodex: nano,
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    assert_eq!(cfg.api_key, "sk-nano");
    assert_eq!(cfg.model, "deepseek-chat");
}

#[test]
fn env_wins_over_nanocodex_file() {
    let tmp = unique_temp_dir("ncx_config_test_envwins");
    let nano = tmp.join("nano.toml");
    write(&nano, "api_key = \"sk-nano\"\n");
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &env1("DEEPSEEK_API_KEY", "sk-env"),
    )
    .unwrap();
    assert_eq!(cfg.api_key, "sk-env");
}

#[test]
fn max_retries_default_and_env() {
    let tmp = unique_temp_dir("ncx_config_test_retries");
    // Default 3
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &no_paths(&tmp),
        &env1("DEEPSEEK_API_KEY", "sk-env"),
    )
    .unwrap();
    assert_eq!(cfg.max_retries, 3);

    // Override via env
    let mut e = env1("DEEPSEEK_API_KEY", "sk-env");
    e.insert("NANOCODEX_MAX_RETRIES".into(), "5".into());
    let cfg2 = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &no_paths(&tmp),
        &e,
    )
    .unwrap();
    assert_eq!(cfg2.max_retries, 5);

    // Garbage falls back to default
    let mut e3 = env1("DEEPSEEK_API_KEY", "sk-env");
    e3.insert("NANOCODEX_MAX_RETRIES".into(), "not-a-number".into());
    let cfg3 = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &no_paths(&tmp),
        &e3,
    )
    .unwrap();
    assert_eq!(cfg3.max_retries, 3);
}

#[test]
fn profile_overrides_base_but_below_env() {
    let tmp = unique_temp_dir("ncx_config_test_profile");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        concat!(
            "api_key = \"sk-base\"\n",
            "model = \"deepseek-chat\"\n",
            "reasoning_effort = \"auto\"\n",
            "\n",
            "[profiles.fast]\n",
            "model = \"deepseek-v4-pro\"\n",
            "reasoning_effort = \"high\"\n",
            "sandbox_mode = \"read-only\"\n",
        ),
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };

    // Profile applied
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            profile: Some("fast".into()),
            ..Default::default()
        },
        &paths,
        &empty_env(),
    )
    .unwrap();
    assert_eq!(cfg.model, "deepseek-v4-pro");
    assert_eq!(cfg.reasoning_effort, "high");
    assert_eq!(cfg.sandbox_mode, "read-only");

    // Env beats profile
    let cfg2 = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            profile: Some("fast".into()),
            ..Default::default()
        },
        &paths,
        &env1("NANOCODEX_MODEL", "deepseek-reasoner"),
    )
    .unwrap();
    assert_eq!(cfg2.model, "deepseek-reasoner");
}

#[test]
fn profile_name_from_env_and_unknown_raises() {
    let tmp = unique_temp_dir("ncx_config_test_profile_env");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        "api_key = \"sk-base\"\n[profiles.fast]\nmodel = \"m-fast\"\n",
    );
    let paths = ConfigPaths {
        deepseek: tmp.join("nope-ds.toml"),
        codex: tmp.join("nope-cx.toml"),
        nanocodex: nano,
    };

    // Name from env
    let cfg = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &env1("NANOCODEX_PROFILE", "fast"),
    )
    .unwrap();
    assert_eq!(cfg.model, "m-fast");

    // Unknown name -> error mentioning the name
    let err = load_config_impl(
        Overrides {
            workspace: Some(tmp.clone()),
            ..Default::default()
        },
        &paths,
        &env1("NANOCODEX_PROFILE", "ghost"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("ghost"), "error: {err}");
}

#[test]
fn list_profiles_returns_sorted_names() {
    let tmp = unique_temp_dir("ncx_config_test_listprof");
    let nano = tmp.join("nano.toml");
    write(
        &nano,
        "[profiles.a]\nmodel=\"x\"\n[profiles.b]\nmodel=\"y\"\n",
    );
    let names = list_profiles_at(&nano);
    assert_eq!(names, vec!["a", "b"]);
}
