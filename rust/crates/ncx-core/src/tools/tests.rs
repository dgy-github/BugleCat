#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

    fn tmp_ws(name: &str) -> PathBuf {
        let d = crate::test_support::unique_temp_dir(&format!("ncx_approve_{name}"));
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
    async fn self_repair_unicode_path_reads_utf16_without_mojibake() {
        let ws = tmp_ws("unicode_encoding");
        std::fs::create_dir(ws.join("资料")).unwrap();
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend("猫与 BOM\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(ws.join("资料/含 空格.txt"), bytes).unwrap();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let output = ReadFileTool
            .execute(&ctx, &json!({"path":"./资料/../资料/含 空格.txt"}))
            .await;
        assert!(output.contains("猫与 BOM"), "{output}");
        assert!(output.contains("UTF-16 LE"), "{output}");
    }

    #[tokio::test]
    async fn self_repair_unicode_path_still_rejects_binary() {
        let ws = tmp_ws("unicode_binary");
        std::fs::write(ws.join("二进制.dat"), [0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let output = ReadFileTool
            .execute(&ctx, &json!({"path":"二进制.dat"}))
            .await;
        assert!(output.starts_with("Error:"), "{output}");
    }

    #[tokio::test]
    async fn python_patch_reports_compile_failure_then_recovery() {
        let ws = tmp_ws("compile_gate");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let result = ApplyPatchTool.execute(&ctx, &json!({"patch": "*** Begin Patch\n*** Add File: bad.py\n+    value = 1\n*** End Patch"})).await;
        assert!(result.contains("compile_failed"), "{result}");
        let result = ApplyPatchTool.execute(&ctx, &json!({"patch": "*** Begin Patch\n*** Update File: bad.py\n-    value = 1\n+value = 1\n*** End Patch"})).await;
        assert!(result.contains("compile_passed"), "{result}");
    }

    #[tokio::test]
    async fn python_patch_registers_all_written_files_before_cancellation() {
        let ws = tmp_ws("compile_cancel");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let args = json!({"patch": "*** Begin Patch\n*** Add File: a.py\n+x = 1\n*** Add File: b.py\n+    x = 1\n*** End Patch"});
        let future = ApplyPatchTool.execute(&ctx, &args);
        tokio::pin!(future);
        // One poll commits the whole patch, then yields to the compiler I/O.
        let _ = futures_util::poll!(&mut future);
        assert!(ws.join("b.py").exists());
        assert_eq!(ctx.python_checks.borrow().len(), 2);
    }

    #[tokio::test]
    async fn python_delete_alias_clears_failed_validation() {
        let ws = tmp_ws("compile_alias");
        std::fs::create_dir(ws.join("sub")).unwrap();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let added = ApplyPatchTool
            .execute(
                &ctx,
                &json!({"patch":"*** Begin Patch\n*** Add File: bad.py\n+    x=1\n*** End Patch"}),
            )
            .await;
        assert!(added.contains("compile_failed"), "{added}");
        assert_eq!(ctx.python_checks.borrow().len(), 1);
        let deleted = ApplyPatchTool
            .execute(
                &ctx,
                &json!({"patch":"*** Begin Patch\n*** Delete File: sub/../bad.py\n*** End Patch"}),
            )
            .await;
        assert!(deleted.contains("successfully"), "{deleted}");
        assert!(!ws.join("bad.py").exists());
        assert!(ctx.python_checks.borrow().is_empty());
    }

    #[tokio::test]
    async fn denied_escaping_patch_is_blocked() {
        let ws = tmp_ws("deny");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)));
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": ESCAPING }))
            .await;
        assert!(out.contains("not approved"), "{out}");
    }

    #[tokio::test]
    async fn no_approver_escaping_patch_errors_out_of_sandbox() {
        let ws = tmp_ws("noapprover");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
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
                timeout_s: 20,
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
                timeout_s: 20,
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
        let blocked = registry
            .execute("apply_patch", &json!({"patch": "invalid"}))
            .await;
        assert!(
            blocked.contains("context compaction consistency check"),
            "{blocked}"
        );
        let read = registry
            .execute("read_file", &json!({"path": "evidence.txt"}))
            .await;
        assert!(read.contains("workspace fact"), "{read}");
    }

    struct StubDiagnosticLspProvider;
    #[async_trait(?Send)]
    impl crate::lsp_tool::LspProvider for StubDiagnosticLspProvider {
        async fn request(&self, request: crate::lsp_tool::LspRequest) -> Result<Value, String> {
            assert_eq!(request.operation, "diagnostics");
            Ok(json!({
                "kind": "full",
                "items": [
                    {
                        "range": {"start": {"line": 2, "character": 8}},
                        "severity": 1,
                        "message": "mismatched types: expected `u32`, found `&str`"
                    },
                    {
                        "range": {"start": {"line": 0, "character": 0}},
                        "severity": 3,
                        "message": "informational note that should be filtered out"
                    }
                ]
            }))
        }
    }

    #[tokio::test]
    async fn post_edit_auto_diagnostics_surfaces_lsp_errors_on_patch() {
        let ws = tmp_ws("post_edit_lsp");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_lsp_provider(Rc::new(StubDiagnosticLspProvider));
        let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn demo() -> u32 {\n+    let x = \"bad\";\n+    x\n+}\n*** End Patch";
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("Patch applied successfully"), "{out}");
        assert!(out.contains("[post_edit_diagnostics:"), "{out}");
        assert!(
            out.contains("src/lib.rs:3:9 [error] mismatched types: expected `u32`, found `&str`"),
            "{out}"
        );
        assert!(!out.contains("informational note"), "{out}");
    }

    #[tokio::test]
    async fn post_edit_auto_diagnostics_catches_invalid_json_toml_and_conflict_markers() {
        let ws = tmp_ws("post_edit_syntax");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));

        let bad_json = "*** Begin Patch\n*** Add File: config.json\n+{\"key\": }\n*** End Patch";
        let out_json = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": bad_json }))
            .await;
        assert!(out_json.contains("[post_edit_diagnostics:"), "{out_json}");
        assert!(out_json.contains("JSON syntax error"), "{out_json}");

        let bad_toml =
            "*** Begin Patch\n*** Add File: Cargo.toml\n+[package\n+name = \"demo\"\n*** End Patch";
        let out_toml = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": bad_toml }))
            .await;
        assert!(out_toml.contains("[post_edit_diagnostics:"), "{out_toml}");
        assert!(out_toml.contains("TOML syntax error"), "{out_toml}");

        let conflict_rs = "*** Begin Patch\n*** Add File: main.rs\n+<<<<<<< HEAD\n+fn a() {}\n+=======\n+fn b() {}\n+>>>>>>> branch\n*** End Patch";
        let out_conflict = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": conflict_rs }))
            .await;
        assert!(
            out_conflict.contains("unresolved git merge conflict marker"),
            "{out_conflict}"
        );

        let clean_json = "*** Begin Patch\n*** Update File: config.json\n-{\"key\": }\n+{\"key\": 42}\n*** End Patch";
        let out_clean = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": clean_json }))
            .await;
        assert!(
            !out_clean.contains("[post_edit_diagnostics:"),
            "{out_clean}"
        );
    }

    struct VerboseLogTool;
    #[async_trait(?Send)]
    impl Tool for VerboseLogTool {
        fn name(&self) -> &str {
            "verbose_log"
        }
        fn description(&self) -> &str {
            "Emits >16k chars with a middle marker."
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        fn read_only(&self) -> bool {
            true
        }
        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            let mut lines = Vec::new();
            for i in 1..=250 {
                lines.push(format!("HEAD_LINE_{i:04}: {}", "a".repeat(40)));
            }
            lines.push("MIDDLE_CRITICAL_TRACEBACK_MARKER_LINE_251".to_string());
            for i in 252..=500 {
                lines.push(format!("TAIL_LINE_{i:04}: {}", "b".repeat(40)));
            }
            lines.push("Exit code: 0".to_string());
            lines.join("\n")
        }
    }

    #[tokio::test]
    async fn large_tool_output_offloads_to_disk_and_is_readable_via_read_file() {
        let ws = tmp_ws("output_offload");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx.clone());
        reg.register(Box::new(VerboseLogTool));

        let out = reg.execute("verbose_log", &json!({})).await;
        assert!(out.contains("HEAD_LINE_0001"), "{out}");
        assert!(out.contains("Exit code: 0"), "{out}");
        assert!(out.contains("chars truncated"), "{out}");
        assert!(out.contains("[tool_output_offloaded: path=\""), "{out}");
        // Middle line was omitted from the inline preview to save context window tokens.
        assert!(
            !out.contains("MIDDLE_CRITICAL_TRACEBACK_MARKER_LINE_251"),
            "middle marker should be offloaded, not inline"
        );

        // Extract the offloaded path from `[tool_output_offloaded: path="<PATH>" ...]`
        let path_start = out.find("[tool_output_offloaded: path=\"").unwrap()
            + "[tool_output_offloaded: path=\"".len();
        let path_end = out[path_start..].find('"').unwrap() + path_start;
        let offload_path = &out[path_start..path_end];

        // Read the omitted middle section via ReadFileTool using offset & limit!
        let middle = ReadFileTool
            .execute(
                &ctx,
                &json!({"path": offload_path, "offset": 245, "limit": 15}),
            )
            .await;
        assert!(
            middle.contains("251| MIDDLE_CRITICAL_TRACEBACK_MARKER_LINE_251"),
            "{middle}"
        );
    }

    #[tokio::test]
    async fn workspace_write_shell_escalates_for_network_and_escaping_paths() {
        let ws = tmp_ws("ws_write_esc");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)));

        // Network command when network_access == false must prompt and be denied when Answer(false):
        let net_out = ShellTool
            .execute(&ctx, &json!({ "command": "curl https://example.com" }))
            .await;
        assert!(net_out.contains("not approved"), "{net_out}");

        // Redirecting outside workspace must prompt and be denied:
        let esc_out = ShellTool
            .execute(&ctx, &json!({ "command": "echo pwn > ../escape.txt" }))
            .await;
        assert!(esc_out.contains("not approved"), "{esc_out}");

        // Writing to .git/hooks inside workspace must prompt and be denied:
        let hook_out = ShellTool
            .execute(
                &ctx,
                &json!({ "command": "echo pwn > .git/hooks/pre-commit" }),
            )
            .await;
        assert!(hook_out.contains("not approved"), "{hook_out}");
    }

    #[tokio::test]
    async fn read_file_and_apply_patch_protect_sensitive_and_hook_paths() {
        let ws = tmp_ws("sensitive_protect");
        std::fs::write(ws.join(".env"), "API_KEY=secret123\n").unwrap();
        std::fs::write(ws.join(".env.example"), "API_KEY=placeholder\n").unwrap();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)));

        // Reading .env is blocked outside danger-full-access:
        let env_read = ReadFileTool.execute(&ctx, &json!({ "path": ".env" })).await;
        assert!(env_read.contains("not allowed"), "{env_read}");

        // Reading .env.example is allowed:
        let example_read = ReadFileTool
            .execute(&ctx, &json!({ "path": ".env.example" }))
            .await;
        assert!(example_read.contains("placeholder"), "{example_read}");

        // Patching .git/hooks/pre-commit inside workspace requires escalation approval:
        let hook_patch = "*** Begin Patch\n*** Add File: .git/hooks/pre-commit\n+#!/bin/sh\n*** End Patch";
        let hook_res = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": hook_patch }))
            .await;
        assert!(hook_res.contains("not approved"), "{hook_res}");
    }

    #[tokio::test]
    async fn on_failure_policy_denies_escalating_command_before_running_then_retries_on_approval() {
        let ws = tmp_ws("on_failure_sandbox");
        // With a denying approver under ON_FAILURE, an escalating command (mkdir in READ_ONLY)
        // must be blocked BEFORE touching the filesystem!
        let ctx_deny = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approval_policy(ON_FAILURE)
            .with_approver(Rc::new(Answer(false)));
        let denied_out = ShellTool
            .execute(&ctx_deny, &json!({ "command": "mkdir should_not_exist" }))
            .await;
        assert!(denied_out.contains("Sandbox denied"), "{denied_out}");
        assert!(
            !ws.join("should_not_exist").exists(),
            "escalating command must not run before approval under ON_FAILURE"
        );

        // With an approving handler under ON_FAILURE, the retry runs and succeeds:
        let ctx_ok = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approval_policy(ON_FAILURE)
            .with_approver(Rc::new(Answer(true)));
        let ok_out = ShellTool
            .execute(&ctx_ok, &json!({ "command": "mkdir approved_on_retry" }))
            .await;
        assert!(ok_out.contains("Exit code: 0"), "{ok_out}");
        assert!(ws.join("approved_on_retry").is_dir());
    }

    #[tokio::test]
    async fn on_failure_policy_does_not_offer_a_retry_for_ordinary_failures() {
        struct Counting(std::cell::Cell<usize>);
        #[async_trait(?Send)]
        impl ApprovalHandler for Counting {
            async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
                self.0.set(self.0.get() + 1);
                ApprovalDecision::Once
            }
        }
        let ws = tmp_ws("on_failure_ordinary");
        let approver = Rc::new(Counting(std::cell::Cell::new(0)));
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::WORKSPACE_WRITE, &ws))
            .with_approval_policy(ON_FAILURE)
            .with_approver(approver.clone());
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "echo once>> runs.txt && exit 3" }))
            .await;
        assert!(out.contains("Exit code: 3"), "{out}");
        assert_eq!(approver.0.get(), 0, "no retry prompt for a non-sandbox failure");
        let runs = std::fs::read_to_string(ws.join("runs.txt")).unwrap();
        assert_eq!(runs.matches("once").count(), 1, "the command ran exactly once");
    }
}
