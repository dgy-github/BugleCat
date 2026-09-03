use super::*;
fn temp(name: &str) -> PathBuf {
    crate::test_support::unique_temp_dir(&format!("ncx-codex-plugin-{name}"))
}

fn fixture(root: &Path, name: &str) {
    fs::create_dir_all(root.join(".codex-plugin")).unwrap();
    fs::create_dir_all(root.join("skills")).unwrap();
    fs::write(root.join("skills/SKILL.md"), "---\nname: demo\n---\n").unwrap();
    fs::write(
        root.join(MANIFEST),
        format!(r#"{{"name":"{name}","skills":["./skills/SKILL.md"]}}"#),
    )
    .unwrap();
}

#[test]
fn codex_plugin_installs_discovers_toggles_and_uninstalls() {
    let source = temp("source");
    let target = temp("target");
    fixture(&source, "demo.plugin");
    let catalog = CodexPluginCatalog::new(&target);
    assert_eq!(
        catalog.install(&source).unwrap().manifest.name,
        "demo.plugin"
    );
    catalog.set_enabled("demo.plugin", false).unwrap();
    assert!(!catalog.discover().unwrap()[0].enabled);
    catalog.uninstall("demo.plugin").unwrap();
    assert!(catalog.discover().unwrap().is_empty());
}

#[test]
fn global_plugins_are_available_and_workspace_plugins_shadow_same_name() {
    let home = temp("global-home");
    let workspace = temp("global-workspace");
    let global = home.join(".ncx/codex-plugins/shared");
    let local = workspace.join(".ncx/codex-plugins/shared");
    fixture(&global, "shared");
    fixture(&local, "shared");
    fs::write(local.join("local-marker"), "workspace").unwrap();

    let plugins = discover_enabled_codex_plugins_with_home(&workspace, Some(&home)).unwrap();

    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].root.join("local-marker").is_file());
    let _ = fs::remove_dir_all(home);
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn codex_plugin_upgrade_replaces_resources_and_preserves_disabled_state() {
    let source = temp("upgrade-source");
    let target = temp("upgrade-target");
    fixture(&source, "demo.plugin");
    let catalog = CodexPluginCatalog::new(&target);
    catalog.install(&source).unwrap();
    catalog.set_enabled("demo.plugin", false).unwrap();
    fs::write(
        source.join(MANIFEST),
        r#"{"name":"demo.plugin","version":"2"}"#,
    )
    .unwrap();
    let upgraded = catalog.install_or_upgrade(&source, true).unwrap();
    assert_eq!(upgraded.manifest.version.as_deref(), Some("2"));
    assert!(!upgraded.enabled);
}

#[test]
fn catalog_recovers_interrupted_upgrade_backup_and_hides_staging() {
    let source = temp("recovery-source");
    let target = temp("recovery-target");
    fixture(&source, "demo.plugin");
    let catalog = CodexPluginCatalog::new(&target);
    catalog.install(&source).unwrap();
    let installed = target.join("demo.plugin");
    let backup = target.join(".demo.plugin.backup");
    let staging = target.join(".demo.plugin.staging");
    fs::rename(&installed, &backup).unwrap();
    fixture(&staging, "demo.plugin");

    let discovered = catalog.discover().unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest.name, "demo.plugin");
    assert!(installed.is_dir());
    assert!(!backup.exists());
    assert!(!staging.exists());
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(target);
}

#[test]
fn plugin_and_marketplace_paths_cannot_escape_their_roots() {
    let source = temp("escape");
    fs::create_dir_all(source.join(".codex-plugin")).unwrap();
    fs::write(
        source.join(MANIFEST),
        r#"{"name":"bad","skills":"../secret"}"#,
    )
    .unwrap();
    assert!(load_record(source).unwrap_err().contains("越界"));
}

#[test]
fn local_marketplace_sources_resolve_from_repository_root_for_all_layouts() {
    for layout in [
        ".agents/plugins/marketplace.json",
        ".claude-plugin/marketplace.json",
    ] {
        let root = temp("marketplace-layout");
        let manifest = root.join(layout);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        let plugin_root = root.join("plugins/demo");
        fixture(&plugin_root, "demo");
        fs::write(&manifest, r#"{"name":"demo","plugins":[]}"#).unwrap();
        let plugin = MarketplacePlugin {
            name: "demo".into(),
            source: MarketplaceSource::Local {
                path: "./plugins/demo".into(),
            },
        };
        assert_eq!(
            resolve_local_marketplace_plugin(&manifest, &plugin).unwrap(),
            plugin_root.canonicalize().unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn official_marketplace_source_shapes_are_accepted() {
    let marketplace: Marketplace = serde_json::from_str(
            r#"{
                "name":"official-shapes",
                "plugins":[
                    {"name":"local","source":"./plugins/local"},
                    {"name":"url","source":{"source":"url","url":"https://example.com/repo.git","path":"plugin","ref":"main","sha":"abc"}},
                    {"name":"subdir","source":{"source":"git-subdir","url":"git@example.com:repo.git","path":"plugins/sub"}},
                    {"name":"npm","source":{"source":"npm","package":"@scope/plugin","version":"1.0.0","registry":"https://registry.example.com"}}
                ]
            }"#,
        )
        .unwrap();
    assert!(matches!(
        &marketplace.plugins[0].source,
        MarketplaceSource::Local { path } if path == "./plugins/local"
    ));
    assert!(matches!(
        &marketplace.plugins[1].source,
        MarketplaceSource::Git { path: Some(path), ref_name: Some(reference), sha: Some(sha), .. }
            if path == "plugin" && reference == "main" && sha == "abc"
    ));
    assert!(matches!(
        &marketplace.plugins[2].source,
        MarketplaceSource::Git { path: Some(path), .. } if path == "plugins/sub"
    ));
    assert!(matches!(
        &marketplace.plugins[3].source,
        MarketplaceSource::Npm { registry: Some(registry), .. }
            if registry == "https://registry.example.com"
    ));

    let legacy: Marketplace = serde_json::from_str(
            r#"{"name":"legacy","plugins":[{"name":"git","source":{"source":"git","url":"https://example.com/repo.git"}}]}"#,
        )
        .unwrap();
    assert!(matches!(
        legacy.plugins[0].source,
        MarketplaceSource::Git { .. }
    ));
}

#[test]
fn conventional_codex_mcp_and_hook_resources_feed_existing_runtime_types() {
    let workspace = temp("runtime-resources");
    let plugin = workspace.join(".ncx/codex-plugins/demo");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::create_dir_all(plugin.join("hooks")).unwrap();
    fs::write(plugin.join(MANIFEST), r#"{"name":"demo"}"#).unwrap();
    fs::write(
        plugin.join(".mcp.json"),
        r#"{"mcpServers":{"files":{"command":"server","args":["--stdio"],"env":{"MODE":"test"}}}}"#,
    )
    .unwrap();
    fs::write(
            plugin.join("hooks/hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"shell","hooks":[{"type":"command","command":"check","timeout":12}]}],"PreCompact":[{"hooks":[{"type":"command","command":"before-compact"}]}],"PostCompact":[{"hooks":[{"type":"command","command":"after-compact"}]}]}}"#,
        )
        .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers[0].name, "demo:files");
    assert_eq!(servers[0].args, vec!["--stdio"]);
    assert_eq!(servers[0].env.get("MODE").map(String::as_str), Some("test"));
    let hooks = discover_codex_hooks_with_home(&workspace, None).unwrap();
    let pre_tool = hooks
        .iter()
        .find(|hook| hook.event == "pre_tool")
        .expect("pre-tool hook");
    assert_eq!(pre_tool.matcher, "shell");
    assert_eq!(pre_tool.timeout_s, 12);
    assert!(hooks
        .iter()
        .any(|hook| hook.event == "pre_compact" && hook.command == "before-compact"));
    assert!(hooks
        .iter()
        .any(|hook| hook.event == "post_compact" && hook.command == "after-compact"));

    let inline = workspace.join(".ncx/codex-plugins/inline");
    fs::create_dir_all(inline.join(".codex-plugin")).unwrap();
    fs::write(
            inline.join(MANIFEST),
            r#"{"name":"inline","mcpServers":{"web":{"command":"web-server"}},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"finish"}]}]}}"#,
        )
        .unwrap();
    assert!(discover_codex_mcp_servers_with_home(&workspace, None)
        .unwrap()
        .iter()
        .any(|server| server.name == "inline:web"));
    assert!(discover_codex_hooks_with_home(&workspace, None)
        .unwrap()
        .iter()
        .any(|hook| hook.event == "stop" && hook.command == "finish"));

    fs::write(inline.join(".disabled"), "disabled\n").unwrap();
    assert!(!discover_codex_mcp_servers_with_home(&workspace, None)
        .unwrap()
        .iter()
        .any(|server| server.name == "inline:web"));
    assert!(!discover_codex_hooks_with_home(&workspace, None)
        .unwrap()
        .iter()
        .any(|hook| hook.command == "finish"));
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn official_path_based_mcp_hooks_and_interface_resources_are_supported() {
    let workspace = temp("path-resources");
    let plugin = workspace.join(".ncx/codex-plugins/path-plugin");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::create_dir_all(plugin.join("hooks")).unwrap();
    fs::create_dir_all(plugin.join("assets")).unwrap();
    fs::write(plugin.join("assets/logo.png"), b"logo").unwrap();
    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        r#"{
                "name":"path-plugin",
                "mcpServers":"./mcp.json",
                "hooks":["./hooks/pre.json","./hooks/post.json"],
                "interface":{"logo":"./assets/logo.png","screenshots":["./assets/logo.png"]}
            }"#,
    )
    .unwrap();
    fs::write(
        plugin.join("mcp.json"),
        r#"{"mcpServers":{"files":{"command":"server"}}}"#,
    )
    .unwrap();
    fs::write(
        plugin.join("hooks/pre.json"),
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"pre"}]}]}}"#,
    )
    .unwrap();
    fs::write(
        plugin.join("hooks/post.json"),
        r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"post"}]}]}}"#,
    )
    .unwrap();

    assert_eq!(
        discover_codex_mcp_servers_with_home(&workspace, None)
            .unwrap()
            .len(),
        1
    );
    let hooks = discover_codex_hooks_with_home(&workspace, None).unwrap();
    assert!(hooks.iter().any(|hook| hook.event == "pre_tool"));
    assert!(hooks.iter().any(|hook| hook.event == "post_tool"));

    fs::write(
        plugin.join(".codex-plugin/plugin.json"),
        r#"{"name":"path-plugin","interface":{"logo":"../outside.png"}}"#,
    )
    .unwrap();
    assert!(load_record(plugin.clone()).unwrap_err().contains("越界"));
    assert!(
        CodexPluginCatalog::new(workspace.join(".ncx/codex-plugins"))
            .discover()
            .unwrap_err()
            .contains("越界")
    );
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn installed_plugin_resolves_relative_mcp_script_from_target_root() {
    let source = temp("mcp-source");
    let target = temp("mcp-target");
    let plugin = source.join(".codex-plugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::create_dir_all(source.join("scripts")).unwrap();
    fs::write(source.join("scripts/server.py"), b"# mock").unwrap();
    fs::write(
        source.join(".mcp.json"),
        r#"{"mcpServers":{"windows":{"command":"python","args":["./scripts/server.py","https://example.test/api"]}}}"#,
    )
    .unwrap();
    fs::write(plugin.join("plugin.json"), r#"{"name":"portable"}"#).unwrap();

    let catalog = CodexPluginCatalog::new(target.join(".ncx/codex-plugins"));
    catalog.install(&source).unwrap();
    let installed = target.join(".ncx/codex-plugins/portable");
    fs::remove_dir_all(&source).unwrap();

    let servers = discover_codex_mcp_servers_with_home(&target, None).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].command, "python");
    assert_eq!(
        Path::new(&servers[0].args[0]),
        installed.join("scripts/server.py").canonicalize().unwrap()
    );
    assert_eq!(servers[0].args[1], "https://example.test/api");

    let _ = fs::remove_dir_all(target);
}

#[test]
fn damaged_mcp_server_is_skipped_without_hiding_valid_servers() {
    let workspace = temp("damaged-mcp-server");
    let plugin = workspace.join(".ncx/codex-plugins/demo");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(plugin.join(MANIFEST), r#"{"name":"demo"}"#).unwrap();
    fs::write(
        plugin.join(".mcp.json"),
        r#"{"mcpServers":{"broken":{"command":"../outside.exe"},"valid":{"command":"python","args":["--version"]}}}"#,
    )
    .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "demo:valid");
    assert_eq!(servers[0].args, vec!["--version"]);
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn malformed_mcp_args_and_env_are_skipped_without_hiding_valid_servers() {
    let workspace = temp("malformed-mcp-values");
    let plugin = workspace.join(".ncx/codex-plugins/demo");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(plugin.join(MANIFEST), r#"{"name":"demo"}"#).unwrap();
    fs::write(
        plugin.join(".mcp.json"),
        r#"{
            "mcpServers": {
                "bad_args_type": {"command":"python","args":"--version"},
                "bad_args_item": {"command":"python","args":["--version",3]},
                "bad_env_type": {"command":"python","env":["MODE=test"]},
                "bad_env_value": {"command":"python","env":{"MODE":true}},
                "valid": {"command":"python","args":["--version"],"env":{"MODE":"test"}}
            }
        }"#,
    )
    .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "demo:valid");
    assert_eq!(servers[0].args, vec!["--version"]);
    assert_eq!(servers[0].env.get("MODE").map(String::as_str), Some("test"));
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn damaged_mcp_plugin_resource_is_skipped_without_hiding_valid_servers() {
    let workspace = temp("damaged-mcp-plugin-resource");
    let broken = workspace.join(".ncx/codex-plugins/broken");
    fs::create_dir_all(broken.join(".codex-plugin")).unwrap();
    fs::write(
        broken.join(MANIFEST),
        r#"{"name":"broken","mcpServers":"./missing-mcp.json"}"#,
    )
    .unwrap();

    let valid = workspace.join(".ncx/codex-plugins/valid");
    fs::create_dir_all(valid.join(".codex-plugin")).unwrap();
    fs::write(valid.join(MANIFEST), r#"{"name":"valid"}"#).unwrap();
    fs::write(
        valid.join(".mcp.json"),
        r#"{"mcpServers":{"server":{"command":"python","args":["--version"]}}}"#,
    )
    .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "valid:server");
    assert_eq!(servers[0].args, vec!["--version"]);
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn damaged_hook_resource_is_skipped_without_hiding_valid_hooks() {
    let workspace = temp("damaged-hook-resource");
    let broken = workspace.join(".ncx/codex-plugins/broken");
    fs::create_dir_all(broken.join(".codex-plugin")).unwrap();
    fs::create_dir_all(broken.join("hooks")).unwrap();
    fs::write(broken.join(MANIFEST), r#"{"name":"broken"}"#).unwrap();
    fs::write(broken.join("hooks/hooks.json"), b"{ not valid json").unwrap();

    let valid = workspace.join(".ncx/codex-plugins/valid");
    fs::create_dir_all(valid.join(".codex-plugin")).unwrap();
    fs::create_dir_all(valid.join("hooks")).unwrap();
    fs::write(valid.join(MANIFEST), r#"{"name":"valid"}"#).unwrap();
    fs::write(
        valid.join("hooks/hooks.json"),
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"check"}]}]}}"#,
    )
    .unwrap();

    let hooks = discover_codex_hooks_with_home(&workspace, None).unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].command, "check");
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn damaged_app_resource_and_entries_are_isolated_from_valid_apps() {
    let workspace = temp("damaged-app-resource");
    let broken = workspace.join(".ncx/codex-plugins/broken");
    fs::create_dir_all(broken.join(".codex-plugin")).unwrap();
    fs::write(broken.join(MANIFEST), r#"{"name":"broken"}"#).unwrap();
    fs::write(broken.join(".app.json"), b"{ not valid json").unwrap();

    let valid = workspace.join(".ncx/codex-plugins/valid");
    fs::create_dir_all(valid.join(".codex-plugin")).unwrap();
    fs::write(
        valid.join(MANIFEST),
        r#"{"name":"valid","apps":{"good":{"id":"connector-good"},"bad":{"id":true},"scalar":"invalid"}}"#,
    )
    .unwrap();

    let apps = discover_codex_apps_with_home(&workspace, None).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].plugin, "valid");
    assert_eq!(apps[0].name, "good");
    assert_eq!(apps[0].connector_id, "connector-good");
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn damaged_plugin_manifest_is_skipped_without_hiding_valid_mcp_servers() {
    let workspace = temp("damaged-plugin-manifest");
    let broken = workspace.join(".ncx/codex-plugins/broken");
    fs::create_dir_all(broken.join(".codex-plugin")).unwrap();
    fs::write(broken.join(MANIFEST), "{ not valid JSON").unwrap();

    let valid = workspace.join(".ncx/codex-plugins/valid");
    fs::create_dir_all(valid.join(".codex-plugin")).unwrap();
    fs::write(valid.join(MANIFEST), r#"{"name":"valid"}"#).unwrap();
    fs::write(
        valid.join(".mcp.json"),
        r#"{"mcpServers":{"server":{"command":"python","args":["--version"]}}}"#,
    )
    .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "valid:server");
    assert_eq!(servers[0].args, vec!["--version"]);
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn bare_mcp_argument_keeps_process_cwd_semantics_even_when_file_exists_in_plugin() {
    let workspace = temp("bare-mcp-argument");
    let plugin = workspace.join(".ncx/codex-plugins/demo");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(plugin.join(MANIFEST), r#"{"name":"demo"}"#).unwrap();
    fs::write(plugin.join("settings.toml"), "mode = 'plugin'").unwrap();
    fs::write(
        plugin.join(".mcp.json"),
        r#"{"mcpServers":{"valid":{"command":"python","args":["settings.toml"]}}}"#,
    )
    .unwrap();

    let servers = discover_codex_mcp_servers_with_home(&workspace, None).unwrap();
    assert_eq!(servers[0].args, vec!["settings.toml"]);
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn codex_apps_are_parsed_as_hosted_connector_resources() {
    let workspace = temp("apps-resources");
    let plugin = workspace.join(".ncx/codex-plugins/demo");
    fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
    fs::write(
        plugin.join(MANIFEST),
        r#"{"name":"demo","apps":"./.app.json"}"#,
    )
    .unwrap();
    fs::write(
            plugin.join(".app.json"),
            r#"{"apps":{"calendar":{"id":"connector-calendar"},"docs":{"connector_id":"connector-docs"}}}"#,
        )
        .unwrap();

    let apps = discover_codex_apps_with_home(&workspace, None).unwrap();
    assert_eq!(apps.len(), 2);
    assert!(apps.iter().any(|app| {
        app.plugin == "demo" && app.name == "calendar" && app.connector_id == "connector-calendar"
    }));
    fs::write(plugin.join(".disabled"), "disabled\n").unwrap();
    assert!(discover_codex_apps_with_home(&workspace, None)
        .unwrap()
        .is_empty());
    let _ = fs::remove_dir_all(workspace);
}
