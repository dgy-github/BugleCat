use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_core::vision_provider_from_config;

    #[test]
    fn help_lists_all_commands() {
        let help = render_help();
        for (cmd, _) in SLASH_HELP {
            assert!(help.contains(cmd), "{cmd}");
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 §10 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn image_input_builds_multimodal_content() {
        let dir = std::env::temp_dir().join(format!("ncx_img_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.jpg");
        std::fs::write(&img, b"foobar").unwrap();

        // No images -> plain text string.
        assert_eq!(build_image_user_input("hi", &[]).unwrap(), json!("hi"));

        // With an image -> [text, image_url(data: URL)].
        let v = build_image_user_input("describe", &[img]).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0], json!({"type": "text", "text": "describe"}));
        assert_eq!(arr[1]["type"], "image_url");
        assert_eq!(
            arr[1]["image_url"]["url"].as_str().unwrap(),
            "data:image/jpeg;base64,Zm9vYmFy"
        );

        // A missing file is a clean error, not a panic.
        assert!(build_image_user_input("x", &[dir.join("nope.png")]).is_err());
    }

    #[test]
    fn inline_images_split_from_prompt() {
        // No flag -> all text, no images.
        let (t, imgs) = split_inline_images("what is this");
        assert_eq!(t, "what is this");
        assert!(imgs.is_empty());

        // Flags anywhere are pulled out; remaining words form the prompt.
        let (t, imgs) = split_inline_images("--image a.png compare these --image b.jpg now");
        assert_eq!(t, "compare these now");
        assert_eq!(imgs, vec![PathBuf::from("a.png"), PathBuf::from("b.jpg")]);
    }

    #[test]
    fn vision_provider_only_built_when_vl_model_set() {
        let mut cfg = ncx_config::Config::default();
        // No vl_model -> image turns stay on the main provider.
        assert!(vision_provider_from_config(&cfg).is_none());
        // vl_model set -> a dedicated vision provider is constructed.
        cfg.vl_model = "qwen-vl-max".into();
        assert!(vision_provider_from_config(&cfg).is_some());
    }

    #[test]
    fn cli_and_gui_use_equivalent_runtime_profiles_for_same_config() {
        let cfg = Config {
            permission_mode: "default".into(),
            max_iterations: 9,
            max_tool_calls: 21,
            max_parallel_tool_calls: 4,
            context_edit_max_chars: 42_000,
            context_edit_keep_recent_messages: 12,
            context_edit_max_tool_result_chars: 888,
            ..Default::default()
        };

        let cli_profile = runtime_profile_for_args(&cfg, &Args::default());
        let gui_profile = AgentRuntimeProfile::from_config(&cfg);

        assert_eq!(cli_profile, gui_profile);
    }

    #[test]
    fn help_lists_custom_project_commands() {
        let ws = std::env::temp_dir().join(format!("ncx_custom_help_{}", new_session_id()));
        let dir = ws.join(".nanocodex").join("commands");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ship.md"), "Prepare release notes.").unwrap();

        let help = render_help_for_workspace(&ws);

        assert!(help.contains("Custom commands"));
        assert!(help.contains("/project:ship"));
    }

    #[test]
    fn parse_config_assignment_accepts_trimmed_key_value() {
        assert_eq!(
            parse_config_assignment(" model = deepseek-chat ").unwrap(),
            ("model".into(), "deepseek-chat".into())
        );
        assert!(parse_config_assignment("model").is_err());
        assert!(parse_config_assignment("bad key=value").is_err());
        assert!(parse_config_assignment("model=").is_err());
    }

    #[test]
    fn usage_tracker_renders_last_and_total_usage() {
        let mut tracker = UsageTracker::default();
        assert_eq!(tracker.render(), "No token usage recorded yet.");

        let mut first_usage = BTreeMap::new();
        first_usage.insert("prompt_tokens".into(), 100);
        first_usage.insert("completion_tokens".into(), 20);
        first_usage.insert("prompt_cache_hit_tokens".into(), 80);
        first_usage.insert("prompt_cache_miss_tokens".into(), 20);
        tracker.record(&TurnResult {
            final_text: "ok".into(),
            iterations: 2,
            stop_reason: "completed".into(),
            tools_used: vec!["read_file".into()],
            usage: first_usage,
        });

        let mut second_usage = BTreeMap::new();
        second_usage.insert("prompt_tokens".into(), 10);
        second_usage.insert("completion_tokens".into(), 5);
        tracker.record(&TurnResult {
            final_text: "ok".into(),
            iterations: 1,
            stop_reason: "completed".into(),
            tools_used: vec![],
            usage: second_usage,
        });

        let rendered = tracker.render();
        assert!(rendered.contains("Last turn"));
        assert!(rendered.contains("Session total"));
        assert!(rendered.contains("model_calls: 3"));
        assert!(rendered.contains("tool_calls:  1"));
        assert!(rendered.contains("prompt_tokens:     110"));
        assert!(rendered.contains("completion_tokens: 25"));
        assert!(rendered.contains("prompt_cache_hit_tokens:  80"));
        assert!(rendered.contains("raw token usage only"));
    }

    #[test]
    fn config_text_writes_known_key_to_path() {
        let dir = std::env::temp_dir().join(format!("ncx_config_slash_{}", new_session_id()));
        let path = dir.join("config.toml");
        let cfg = ncx_config::Config::default();
        let out = config_text_at(&cfg, "model=deepseek-chat", &path);

        assert!(out.contains("Saved config"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("model = \"deepseek-chat\""), "{text}");
    }

    #[test]
    fn config_text_rejects_unknown_key() {
        let dir = std::env::temp_dir().join(format!("ncx_config_slash_bad_{}", new_session_id()));
        let path = dir.join("config.toml");
        let cfg = ncx_config::Config::default();
        let out = config_text_at(&cfg, "bogus=value", &path);

        assert!(out.contains("Unknown writable config key"));
        assert!(!path.exists());
    }

    #[test]
    fn status_masks_api_key() {
        let cfg = ncx_config::Config {
            api_key: "sk-secret1234".into(),
            ..Default::default()
        };
        let status = render_status(&cfg);
        assert!(status.contains("****1234"));
        assert!(!status.contains("secret"));
    }

    #[test]
    fn history_renders_saved_sessions() {
        let rows = vec![ThreadMetadata {
            id: ThreadId::new("sid").unwrap(),
            workspace: "/p".into(),
            title: "fix bug".into(),
            created_at: 1,
            updated_at: 2,
            archived: false,
        }];
        let out = render_history(&rows, 10);
        assert!(out.contains("sid"));
        assert!(out.contains("fix bug"));
        assert!(out.contains("  2  "));
    }

    #[test]
    fn cli_recorder_uses_protocol_store_for_turn_ownership_and_resume() {
        let root = std::env::temp_dir().join(format!("ncx_cli_thread_{}", new_session_id()));
        let workspace = root.join("workspace");
        let store_path = root.join("threads-v2.json");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut recorder =
            SessionRecorder::open_at(workspace.clone(), false, store_path.clone()).unwrap();
        let original_id = recorder.thread_id.clone();
        let turn_id = recorder.start_turn("修复历史恢复").unwrap();
        let thread = match recorder
            .server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: original_id.clone(),
            })
            .unwrap()
            .response
            .payload
        {
            ResponsePayload::Thread(thread) => thread,
            _ => panic!("expected thread"),
        };
        assert_eq!(thread.metadata.title, "修复历史恢复");
        assert_eq!(thread.turns[0].status, TurnStatus::Running);
        assert!(matches!(
            thread.turns[0].items.first(),
            Some(ThreadItem::UserMessage { text, .. }) if text == "修复历史恢复"
        ));

        let messages = vec![
            json!({"role": "user", "content": "修复历史恢复"}),
            json!({"role": "assistant", "content": "已完成"}),
        ];
        recorder
            .server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: original_id.clone(),
                messages: messages.clone(),
            })
            .unwrap();
        recorder
            .server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: original_id.clone(),
                turn_id,
                status: TurnStatus::Completed,
                error: None,
                usage: ProtocolTurnUsage::default(),
            })
            .unwrap();
        drop(recorder);

        let resumed = SessionRecorder::open_at(workspace, true, store_path).unwrap();
        assert_eq!(resumed.thread_id, original_id);
        assert_eq!(resumed.model_context(), Some(messages));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checkpoints_render_saved_entries() {
        let rows = vec![CheckpointMeta {
            id: "cp1".into(),
            label: "before edit".into(),
            created_at: "2026-06-01T10:00:00".into(),
            files: vec!["a.txt".into()],
            skipped_paths: vec!["target/big".into()],
            total_bytes: 12,
        }];
        let out = render_checkpoints(&rows, 10);
        assert!(out.contains("cp1"));
        assert!(out.contains("before edit"));
        assert!(out.contains("skipped=1"));
    }

    #[test]
    fn export_renders_user_assistant_tool_markdown() {
        let mut s = Session::new("system instructions");
        s.add_user_text("fix the bug");
        s.add_assistant(
            "looking into it",
            Some(vec![json!({
                "id": "c1",
                "type": "function",
                "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"}
            })]),
            "thinking step by step",
        );
        s.add_tool_result("c1", "shell", "file.rs");

        let md = render_session_markdown(&s.system, &s.messages, "deepseek-chat", Path::new("/ws"));

        assert!(md.starts_with("# nanocodex session export"));
        assert!(md.contains("model: `deepseek-chat`"));
        assert!(md.contains("messages: 3 (user 1, assistant 1, tool 1)"));
        assert!(md.contains("## System prompt"));
        assert!(md.contains("## User"));
        assert!(md.contains("fix the bug"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("<details><summary>reasoning</summary>"));
        assert!(md.contains("thinking step by step"));
        assert!(md.contains("tool call: `shell`"));
        assert!(md.contains("### Tool result: `shell`"));
        assert!(md.contains("file.rs"));
    }

    #[test]
    fn export_flattens_multimodal_and_hides_image_data() {
        let mut s = Session::new("");
        s.add_user(json!([
            {"type": "text", "text": "what is this"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ]));

        let md = render_session_markdown(&s.system, &s.messages, "m", Path::new("/ws"));

        assert!(md.contains("what is this"));
        assert!(md.contains("[image]"));
        assert!(!md.contains("AAAA"));
        // An empty system prompt is omitted entirely.
        assert!(!md.contains("## System prompt"));
    }

    #[test]
    fn export_writes_markdown_file_to_explicit_path() {
        let dir = std::env::temp_dir().join(format!("ncx_export_{}", new_session_id()));
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let mut s = Session::new("sys");
        s.add_user_text("hello world");
        let target = dir.join("out.md");

        let status = export_session_text(&s, &cfg, "sid", target.to_str().unwrap());

        assert!(status.contains("Exported 1 message(s)"));
        assert!(status.contains("out.md"));
        let written = std::fs::read_to_string(&target).unwrap();
        assert!(written.contains("hello world"));
    }

    #[test]
    fn export_refuses_to_overwrite_existing_explicit_file() {
        let dir = std::env::temp_dir().join(format!("ncx_export_clob_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let existing = dir.join("keep.md");
        std::fs::write(&existing, "IMPORTANT").unwrap();
        let mut s = Session::new("sys");
        s.add_user_text("hi");

        let status = export_session_text(&s, &cfg, "sid", existing.to_str().unwrap());

        assert!(status.contains("already exists"), "{status}");
        // The original file is untouched.
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "IMPORTANT");
    }

    #[test]
    fn export_refuses_directory_arg_with_clear_message() {
        let dir = std::env::temp_dir().join(format!("ncx_export_dir_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let mut s = Session::new("sys");
        s.add_user_text("hi");

        let status = export_session_text(&s, &cfg, "sid", dir.to_str().unwrap());

        assert!(status.contains("is a directory"), "{status}");
    }

    #[test]
    fn export_default_path_uses_session_id_under_exports() {
        let p = export_target_path(Path::new("/ws"), "abc123", "");
        let s = p.to_string_lossy();
        assert!(s.contains("exports"), "{s}");
        assert!(s.ends_with("abc123.md"), "{s}");

        // A relative arg resolves against the workspace; absolute is taken as-is.
        let rel = export_target_path(Path::new("/ws"), "abc123", "notes/out.md");
        assert!(rel.ends_with(Path::new("notes/out.md")));
    }

    #[test]
    fn export_uses_longer_fence_when_content_has_backticks() {
        assert_eq!(code_fence("no backticks"), "```");
        assert_eq!(code_fence("inline `code`"), "```");
        assert_eq!(code_fence("a ``` b"), "````");
        assert_eq!(code_fence("````x"), "`````");

        let mut s = Session::new("");
        s.add_tool_result("c1", "read_file", "here:\n```rust\nfn main() {}\n```\n");
        let md = render_session_markdown(&s.system, &s.messages, "m", Path::new("/ws"));
        // The wrapping fence is longer than the inner triple backticks, and the
        // inner content survives verbatim.
        assert!(md.contains("````\n"), "{md}");
        assert!(md.contains("```rust"));
        assert!(md.contains("fn main() {}"));
    }

    #[test]
    fn review_verify_prompts_reference_diff_and_scope() {
        let review = review_prompt("src/main.rs");
        assert!(review.contains("git --no-pager diff"));
        assert!(review.contains("Scope/focus for this pass: src/main.rs"));

        let sec = security_review_prompt("");
        assert!(sec.to_lowercase().contains("injection"));
        assert!(!sec.contains("Scope/focus"));

        let verify = verify_prompt("the parser");
        assert!(verify.contains("VERIFIED"));
        assert!(verify.contains("the parser"));
    }

    #[test]
    fn doc_prompts_name_format_file_and_backend() {
        let d = doc_prompt("docx", "report.docx");
        assert!(d.contains(".docx"));
        assert!(d.contains("Target file: report.docx"));
        assert!(d.to_lowercase().contains("python-docx"));

        // No file arg -> the agent is told to ask which file.
        let x = doc_prompt("xlsx", "");
        assert!(x.contains(".xlsx"));
        assert!(x.to_lowercase().contains("openpyxl"));
        assert!(x.contains("names next"));

        let p = doc_prompt("pdf", "a.pdf");
        let pl = p.to_lowercase();
        assert!(pl.contains("pdfplumber") || pl.contains("pypdf"));

        let pptx = doc_prompt("pptx", "deck.pptx");
        assert!(pptx.to_lowercase().contains("python-pptx"));
    }
}
