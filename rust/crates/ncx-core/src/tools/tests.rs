#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

    fn tmp_ws(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncx_approve_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    struct Answer(bool);
    #[async_trait(?Send)]
    impl ApprovalHandler for Answer {
        async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
            if self.0 {
                ApprovalDecision::Once
            } else {
                ApprovalDecision::Deny
            }
        }
    }

    struct AlwaysAnswer;
    #[async_trait(?Send)]
    impl ApprovalHandler for AlwaysAnswer {
        async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Always
        }
    }

    // A patch whose Add-File path climbs out of the workspace needs approval.
    const ESCAPING: &str = "*** Begin Patch\n*** Add File: ../escape.txt\n+x\n*** End Patch";

    #[tokio::test]
    async fn denied_escaping_patch_is_blocked() {
        let ws = tmp_ws("deny");
        let ctx = ToolContext::new(
            ws,
            SandboxPolicy::new(WORKSPACE_WRITE, std::env::temp_dir()),
        )
        .with_approver(Rc::new(Answer(false)));
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": ESCAPING }))
            .await;
        assert!(out.contains("not approved"), "{out}");
    }

    #[tokio::test]
    async fn no_approver_escaping_patch_errors_out_of_sandbox() {
        let ws = tmp_ws("noapprover");
        let ctx = ToolContext::new(
            ws,
            SandboxPolicy::new(WORKSPACE_WRITE, std::env::temp_dir()),
        );
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": ESCAPING }))
            .await;
        // Without an approver the write is simply rejected by the policy.
        assert!(out.contains("Error applying patch"), "{out}");
        assert!(out.contains("outside the writable sandbox"), "{out}");
    }

    #[tokio::test]
    async fn in_workspace_patch_needs_no_approval() {
        let ws = tmp_ws("inws");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let patch = "*** Begin Patch\n*** Add File: ok.txt\n+hi\n*** End Patch";
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("Patch applied successfully"), "{out}");
        assert_eq!(std::fs::read_to_string(ws.join("ok.txt")).unwrap(), "hi\n");
    }

    #[tokio::test]
    async fn plan_mode_refuses_edits() {
        let ws = tmp_ws("planmode");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(true))) // even with an approver saying yesâ€¦
            .with_plan_mode(true);
        let patch = "*** Begin Patch\n*** Add File: nope.txt\n+x\n*** End Patch";
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("plan mode"), "{out}");
        assert!(!ws.join("nope.txt").exists(), "no file should be written");
    }

    #[tokio::test]
    async fn require_edit_approval_prompts_in_workspace() {
        let ws = tmp_ws("editapprove");
        let patch = "*** Begin Patch\n*** Add File: gated.txt\n+hi\n*** End Patch";
        // Denied â†’ not applied, even though the path is inside the workspace.
        let denied = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)))
            .with_require_edit_approval(true);
        let out = ApplyPatchTool
            .execute(&denied, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("not approved"), "{out}");
        assert!(!ws.join("gated.txt").exists(), "denied edit must not write");

        // Approved â†’ applied.
        let ok = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(true)))
            .with_require_edit_approval(true);
        let out2 = ApplyPatchTool
            .execute(&ok, &json!({ "patch": patch }))
            .await;
        assert!(out2.contains("Patch applied successfully"), "{out2}");
        assert_eq!(
            std::fs::read_to_string(ws.join("gated.txt")).unwrap(),
            "hi\n"
        );
    }

    #[tokio::test]
    async fn always_allow_edits_skips_later_prompts() {
        let ws = tmp_ws("alwaysedit");
        let grants = Rc::new(RefCell::new(SessionGrants::default()));
        // First edit: user picks "Always" â†’ applied + grant remembered.
        let ctx1 = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(AlwaysAnswer))
            .with_require_edit_approval(true)
            .with_session_grants(grants.clone());
        let p1 = "*** Begin Patch\n*** Add File: a.txt\n+1\n*** End Patch";
        let o1 = ApplyPatchTool.execute(&ctx1, &json!({ "patch": p1 })).await;
        assert!(o1.contains("Patch applied successfully"), "{o1}");
        assert!(grants.borrow().allow_edits, "edit grant remembered");
        // Second edit shares the grants but has a DENY approver â€” the session grant
        // means it is never consulted, so the edit still applies.
        let ctx2 = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)))
            .with_require_edit_approval(true)
            .with_session_grants(grants.clone());
        let p2 = "*** Begin Patch\n*** Add File: b.txt\n+2\n*** End Patch";
        let o2 = ApplyPatchTool.execute(&ctx2, &json!({ "patch": p2 })).await;
        assert!(o2.contains("Patch applied successfully"), "{o2}");
        assert!(
            ws.join("b.txt").exists(),
            "second edit applied without prompt"
        );
    }

    #[tokio::test]
    async fn shell_read_only_command_auto_runs() {
        // A read-only command under on-request auto-approves and runs â€” no approver.
        let ws = tmp_ws("shell_ro");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "echo ncx_shell_ok" }))
            .await;
        assert!(out.contains("ncx_shell_ok"), "{out}");
        assert!(out.contains("Exit code: 0"), "{out}");
    }

    #[tokio::test]
    async fn shell_escalating_command_denied_without_approval() {
        // read-only sandbox: a write-ish command escalates; a denying approver blocks it.
        let ws = tmp_ws("shell_esc");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approver(Rc::new(Answer(false)));
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "rm -rf build" }))
            .await;
        assert!(out.contains("not approved"), "{out}");
    }

    #[tokio::test]
    async fn shell_escalating_command_runs_when_approved() {
        let ws = tmp_ws("shell_ok");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approver(Rc::new(Answer(true)));
        // `mkdir` isn't read-only -> escalates; approved -> actually runs (cross-platform).
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "mkdir ncxsub" }))
            .await;
        assert!(!out.contains("not approved"), "{out}");
        assert!(out.contains("Exit code: 0"), "{out}");
        assert!(ws.join("ncxsub").is_dir());
    }

    struct NamedTool(&'static str, &'static str);
    #[async_trait(?Send)]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            self.1
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            "ok".into()
        }
    }

    #[test]
    fn tool_subset_replacement_is_atomic_and_rebuilds_catalog() {
        let ws = tmp_ws("replace_tools");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(NamedTool("core", "core tool")));
        reg.register(Box::new(NamedTool("mcp_old", "old MCP tool")));

        let names = reg
            .replace_tools(
                &["mcp_old".to_string()],
                vec![Box::new(NamedTool("mcp_new", "new MCP tool"))],
            )
            .unwrap();
        assert_eq!(names, ["mcp_new"]);
        assert!(reg.get("core").is_some());
        assert!(reg.get("mcp_old").is_none());
        assert!(reg.get("mcp_new").is_some());
        assert!(reg
            .ctx
            .tool_catalog
            .borrow()
            .iter()
            .any(|entry| entry.name == "mcp_new"));

        let error = reg
            .replace_tools(
                &["mcp_new".to_string()],
                vec![Box::new(NamedTool("core", "collision"))],
            )
            .unwrap_err();
        assert!(error.contains("conflicts"), "{error}");
        assert!(
            reg.get("mcp_new").is_some(),
            "failed commit kept old MCP tool"
        );
        assert_eq!(reg.get("core").unwrap().description(), "core tool");
    }

    #[tokio::test]
    async fn tool_search_returns_matches_and_hints_schema_exposure() {
        let ws = tmp_ws("tool_search");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ToolSearchTool));
        reg.register(Box::new(NamedTool("alpha", "general alpha helper")));
        reg.register(Box::new(NamedTool(
            "deploy",
            "build release packages and installers",
        )));
        reg.register(Box::new(NamedTool("debugger", "inspect failures")));

        let out = reg
            .execute("tool_search", &json!({"query": "installer release"}))
            .await;
        assert!(out.contains("deploy"), "{out}");
        assert!(reg.ctx.tool_hints.borrow().contains(&"deploy".to_string()));

        let schemas = reg.schemas_limited_for_query("", 2);
        let names: Vec<String> = schemas
            .iter()
            .filter_map(|s| s["function"]["name"].as_str().map(String::from))
            .collect();
        assert!(names.contains(&"tool_search".to_string()));
        assert!(names.contains(&"deploy".to_string()));
    }

    #[test]
    fn mixed_harness_task_exposes_lsp_background_and_terminal_tools() {
        let ws = tmp_ws("mixed_harness_visibility");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let reg = ToolRegistry::new(ctx);
        let query = "Use lsp, background_start, background_poll, terminal_open, \
                     terminal_write, and terminal_read to verify the runtime.";
        let names = reg
            .schemas_for_query(query)
            .into_iter()
            .filter_map(|schema| schema["function"]["name"].as_str().map(String::from))
            .collect::<HashSet<_>>();

        for expected in [
            "lsp",
            "background_start",
            "background_poll",
            "terminal_open",
            "terminal_write",
            "terminal_read",
        ] {
            assert!(names.contains(expected), "missing {expected}: {names:?}");
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_shell_rejects_posix_only_syntax_before_execution() {
        let ws = tmp_ws("shell_windows_syntax");
        let ctx = ToolContext::new(
            ws,
            SandboxPolicy::new(ncx_sandbox::DANGER_FULL_ACCESS, Path::new(".")),
        );

        for command in ["python - <<'EOF'\nprint('x')\nEOF", "echo ok | tail -1"] {
            let out = ShellTool
                .execute(&ctx, &json!({ "command": command }))
                .await;
            assert!(out.starts_with("Error: Windows shell"), "{out}");
            assert!(out.contains("temporary script"), "{out}");
        }
    }

    #[test]
    fn essential_recursive_discovery_tools_are_always_visible() {
        let ws = tmp_ws("essential_discovery_visibility");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let reg = ToolRegistry::new(ctx);
        let names = reg
            .schemas_for_query("请帮我检查这个陌生项目")
            .into_iter()
            .filter_map(|schema| schema["function"]["name"].as_str().map(String::from))
            .collect::<HashSet<_>>();

        for expected in [
            "find_files",
            "grep",
            "glob",
            "list_directory",
            "path_info",
            "read_file",
        ] {
            assert!(names.contains(expected), "missing {expected}: {names:?}");
        }
    }

    fn schema_desc(schemas: &[Value], name: &str) -> Option<String> {
        schemas.iter().find_map(|s| {
            let f = &s["function"];
            if f["name"] == name {
                f["description"].as_str().map(String::from)
            } else {
                None
            }
        })
    }

    #[tokio::test]
    async fn empty_genome_leaves_schema_and_catalog_byte_identical() {
        let ws = tmp_ws("genome_noop");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ReadFileTool));
        // schema description == the tool's own default
        let schemas = reg.schemas_limited_for_query("", 9);
        assert_eq!(
            schema_desc(&schemas, "read_file").as_deref(),
            Some(ReadFileTool.description())
        );
        // catalog description == default too
        let cat = reg.ctx.tool_catalog.borrow();
        assert_eq!(cat[0].description, ReadFileTool.description());
    }

    #[tokio::test]
    async fn genome_override_reaches_schema_and_catalog() {
        use crate::genome::Genome;
        let ws = tmp_ws("genome_override");
        let mut g = Genome::default();
        g.tool_desc
            .insert("read_file".into(), "OVERRIDDEN read desc".into());
        let ctx =
            ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws)).with_genome(g);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ReadFileTool));
        reg.register(Box::new(ShellTool));

        // The model-facing schema shows the override for read_file...
        let schemas = reg.schemas_limited_for_query("", 9);
        assert_eq!(
            schema_desc(&schemas, "read_file").as_deref(),
            Some("OVERRIDDEN read desc")
        );
        // ...and shell (no override) keeps its default.
        assert_eq!(
            schema_desc(&schemas, "shell").as_deref(),
            Some(ShellTool.description())
        );
        // tool_search's catalog sees the override too.
        let cat = reg.ctx.tool_catalog.borrow();
        let rf = cat.iter().find(|e| e.name == "read_file").unwrap();
        assert_eq!(rf.description, "OVERRIDDEN read desc");
    }

    #[tokio::test]
    async fn skill_tool_loads_body_and_reports_unknown() {
        use crate::skills::Skill;
        let ws = tmp_ws("skill_tool");
        let dir = ws.join("skills").join("greeter");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: greeter\ndescription: say hi\n---\n\nStep 1: greet warmly.",
        )
        .unwrap();
        let skill = Skill {
            name: "greeter".into(),
            description: "say hi".into(),
            capability: Default::default(),
            always_apply: false,
            path: dir.join("SKILL.md"),
            dir: dir.clone(),
            embedded: None,
        };
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_skills(vec![skill]);

        let out = SkillTool.execute(&ctx, &json!({"name": "greeter"})).await;
        assert!(out.contains("Step 1: greet warmly."), "{out}");
        assert!(out.contains("greeter"), "{out}");

        let miss = SkillTool.execute(&ctx, &json!({"name": "nope"})).await;
        assert!(miss.contains("no skill named 'nope'"), "{miss}");
        assert!(miss.contains("greeter"), "{miss}");
    }

    #[tokio::test]
async fn skill_tool_registered_only_when_skills_present() {
        use crate::skills::Skill;
        let ws = tmp_ws("skill_reg");
        let bare = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        assert!(ToolRegistry::new(bare).get("skill").is_none());

        let withskill = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_skills(vec![Skill {
                name: "x".into(),
                description: String::new(),
                capability: Default::default(),
                always_apply: false,
                path: ws.join("SKILL.md"),
                dir: ws.clone(),
                embedded: None,
            }]);
        assert!(ToolRegistry::new(withskill).get("skill").is_some());
    }

    #[tokio::test]
    async fn pre_tool_hook_can_block_execution() {
        let ws = tmp_ws("hook_pre_block");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_hooks(vec![HookConfig {
                event: "pre_tool".into(),
                matcher: "dummy".into(),
                command: "exit 1".into(),
                timeout_s: 3,
            }]);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(NamedTool("dummy", "test tool")));

        let out = reg.execute("dummy", &json!({})).await;

        assert!(out.contains("blocked by pre_tool hook"), "{out}");
        assert!(!out.ends_with("ok"), "{out}");
    }

    #[tokio::test]
    async fn post_tool_hook_output_is_returned() {
        let ws = tmp_ws("hook_post_note");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_hooks(vec![HookConfig {
                event: "post_tool".into(),
                matcher: "*".into(),
                command: "echo post-ok".into(),
                timeout_s: 3,
            }]);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(NamedTool("dummy", "test tool")));

        let out = reg.execute("dummy", &json!({})).await;

        assert!(out.contains("ok"), "{out}");
        assert!(out.contains("[hook output]"), "{out}");
        assert!(out.contains("post-ok"), "{out}");
    }

    #[tokio::test]
    async fn compaction_recovery_blocks_writes_but_keeps_reads_available() {
        let ws = tmp_ws("compaction_recovery");
        std::fs::write(ws.join("evidence.txt"), "workspace fact").unwrap();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        ctx.compaction_read_only_recovery.set(true);
        let registry = ToolRegistry::new(ctx);
        let blocked = registry.execute("apply_patch", &json!({"patch": "invalid"})).await;
        assert!(blocked.contains("context compaction consistency check"), "{blocked}");
        let read = registry.execute("read_file", &json!({"path": "evidence.txt"})).await;
        assert!(read.contains("workspace fact"), "{read}");
    }
}
