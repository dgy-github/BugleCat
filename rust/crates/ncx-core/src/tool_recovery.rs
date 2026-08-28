//! Capability routing and conservative recovery for model-facing tools.

use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Stable capability groups used for discovery and compatible fallback routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCapability {
    FileRead,
    FileEdit,
    FileSearch,
    PathDiscovery,
    WebSearch,
    WebFetch,
    VersionControl,
    Shell,
    Planning,
    Memory,
    Skill,
    External,
}

impl fmt::Display for ToolCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::FileRead => "file-read",
            Self::FileEdit => "file-edit",
            Self::FileSearch => "file-search",
            Self::PathDiscovery => "path-discovery",
            Self::WebSearch => "web-search",
            Self::WebFetch => "web-fetch",
            Self::VersionControl => "version-control",
            Self::Shell => "shell",
            Self::Planning => "planning",
            Self::Memory => "memory",
            Self::Skill => "skill",
            Self::External => "external",
        };
        f.write_str(name)
    }
}

/// Normalized failure classes. Only transient failures are retried unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFailureClass {
    InvalidInput,
    PermissionDenied,
    NotFound,
    WrongTarget,
    Timeout,
    Cancelled,
    Transient,
    UnknownTool,
    Execution,
}

impl fmt::Display for ToolFailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format!("{self:?}").to_ascii_lowercase())
    }
}

impl ToolFailureClass {
    pub fn retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Transient)
    }
}

/// Classify the existing string result contract without changing public tool APIs.
pub fn classify_tool_result(result: &str) -> Option<ToolFailureClass> {
    let text = result.trim().to_ascii_lowercase();
    let failed_exit = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("exit code:"))
        .filter_map(|code| code.trim().parse::<i32>().ok())
        .any(|code| code != 0);
    let exhausted_not_found_recovery = text.starts_with("[recovery:")
        && text.contains("after notfound")
        && (text.contains("\"count\":0") || text.contains("\"matches\":[]"));
    if !(text.starts_with("error:")
        || text.starts_with("[interrupted:")
        || failed_exit
        || exhausted_not_found_recovery)
    {
        return None;
    }
    let class = if exhausted_not_found_recovery {
        ToolFailureClass::NotFound
    } else if text.contains("unknown tool") {
        ToolFailureClass::UnknownTool
    } else if text.contains("stopped by user")
        || text.contains("cancelled")
        || text.contains("canceled")
        || text.contains("interrupted")
    {
        ToolFailureClass::Cancelled
    } else if text.contains("timed out") || text.contains("timeout") {
        ToolFailureClass::Timeout
    } else if text.contains("not allowed")
        || text.contains("disabled in")
        || text.contains("permission denied")
        || text.contains("blocked by")
        || text.contains("approval")
    {
        ToolFailureClass::PermissionDenied
    } else if text.contains("is a directory")
        || text.contains("not a regular file")
        || text.contains("not a file")
    {
        ToolFailureClass::WrongTarget
    } else if text.contains("not found") || text.contains("no such file") {
        ToolFailureClass::NotFound
    } else if text.contains("required")
        || text.contains("invalid regex")
        || text.contains("invalid argument")
        || text.contains("must be")
    {
        ToolFailureClass::InvalidInput
    } else if text.contains("temporar")
        || text.contains("connection reset")
        || text.contains("connection refused")
        || text.contains("service unavailable")
        || text.contains("status 429")
        || text.contains("status 502")
        || text.contains("status 503")
        || text.contains("status 504")
    {
        ToolFailureClass::Transient
    } else {
        ToolFailureClass::Execution
    };
    Some(class)
}

/// Infer a compact capability catalog for built-ins and dynamically loaded tools.
pub fn infer_capabilities(name: &str, description: &str) -> Vec<ToolCapability> {
    let lowered = format!("{} {}", name, description).to_ascii_lowercase();
    let mut out = Vec::new();
    let candidates = [
        (
            ToolCapability::FileRead,
            ["read_file", "file read", "read file"].as_slice(),
        ),
        (
            ToolCapability::FileEdit,
            ["apply_patch", "write file", "edit file", "str_replace"].as_slice(),
        ),
        (
            ToolCapability::FileSearch,
            ["grep", "file contents", "file-search"].as_slice(),
        ),
        (
            ToolCapability::PathDiscovery,
            ["glob", "list_directory", "path_info", "find files"].as_slice(),
        ),
        (
            ToolCapability::WebSearch,
            ["web_search", "web search"].as_slice(),
        ),
        (
            ToolCapability::WebFetch,
            ["web_fetch", "fetch a web"].as_slice(),
        ),
        (
            ToolCapability::VersionControl,
            ["git_", "git status", "git diff"].as_slice(),
        ),
        (
            ToolCapability::Shell,
            ["shell", "bash", "pwsh", "terminal"].as_slice(),
        ),
        (
            ToolCapability::Planning,
            ["update_plan", "todo", "goal"].as_slice(),
        ),
        (ToolCapability::Memory, ["remember", "memory"].as_slice()),
        (ToolCapability::Skill, ["skill"].as_slice()),
    ];
    for (capability, needles) in candidates {
        if needles.iter().any(|needle| lowered.contains(needle)) {
            out.push(capability);
        }
    }
    if out.is_empty() {
        out.push(ToolCapability::External);
    }
    out
}

/// Return an argument-compatible, read-only fallback call for known tool pairs.
pub fn fallback_call(
    name: &str,
    args: &Value,
    failure: ToolFailureClass,
) -> Option<(&'static str, Value)> {
    match (name, failure) {
        ("grep", ToolFailureClass::InvalidInput) => {
            let pattern = args.get("pattern")?.as_str()?;
            Some((
                "grep_literal",
                json!({
                    "pattern": pattern,
                    "path_glob": args.get("path_glob").cloned().unwrap_or(Value::Null),
                    "max_results": args.get("max_results").cloned().unwrap_or(json!(200))
                }),
            ))
        }
        ("read_file", ToolFailureClass::WrongTarget) => {
            let path = args.get("path")?.as_str()?;
            Some(("list_directory", json!({"path": path, "depth": 1})))
        }
        ("read_file", ToolFailureClass::NotFound) => {
            let path = args.get("path")?.as_str()?;
            let basename = Path::new(path).file_name()?.to_string_lossy();
            Some((
                "find_files",
                json!({"query": basename, "exact": true, "max_results": 20}),
            ))
        }
        _ => None,
    }
}

/// Resolve a missing `read_file` request only when recursive basename search
/// produces exactly one candidate. Ambiguous matches are deliberately left to
/// `find_files` so recovery never guesses which source file the model meant.
pub fn resolve_unique_missing_read(workspace: &Path, args: &Value) -> Option<(String, Value)> {
    let requested = args.get("path")?.as_str()?;
    let requested_path = PathBuf::from(requested);
    if requested_path.is_absolute() {
        return None;
    }
    let basename = requested_path.file_name()?.to_string_lossy().to_string();
    let candidates = crate::search::find_files_by_name(workspace, &basename, true, 2);
    if candidates.len() != 1 {
        return None;
    }
    let resolved = candidates.into_iter().next()?;
    let mut recovered_args = args.clone();
    recovered_args
        .as_object_mut()?
        .insert("path".to_string(), Value::String(resolved.clone()));
    Some((resolved, recovered_args))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use async_trait::async_trait;
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};

    use super::*;
    use crate::tools::{Tool, ToolContext, ToolRegistry};

    #[test]
    fn classifies_failures_without_marking_success_text() {
        assert_eq!(classify_tool_result("done"), None);
        assert_eq!(
            classify_tool_result("Error: invalid regex: unclosed group"),
            Some(ToolFailureClass::InvalidInput)
        );
        assert_eq!(
            classify_tool_result("Error: service unavailable (status 503)"),
            Some(ToolFailureClass::Transient)
        );
        assert_eq!(
            classify_tool_result("[interrupted: stopped by user mid-command]"),
            Some(ToolFailureClass::Cancelled)
        );
        assert_eq!(
            classify_tool_result("STDERR: bad command\n\nExit code: 1"),
            Some(ToolFailureClass::Execution)
        );
        assert_eq!(
            classify_tool_result(
                "[recovery: read_file -> find_files after notfound]\n{\"count\":0,\"matches\":[],\"query\":\"missing.txt\"}"
            ),
            Some(ToolFailureClass::NotFound)
        );
        assert_eq!(
            classify_tool_result("warning on stderr\n\nExit code: 0"),
            None
        );
    }

    #[test]
    fn fallback_routes_only_known_compatible_calls() {
        let args = json!({"pattern": "[", "max_results": 4});
        let (name, mapped) = fallback_call("grep", &args, ToolFailureClass::InvalidInput).unwrap();
        assert_eq!(name, "grep_literal");
        assert_eq!(mapped["pattern"], "[");
        assert!(fallback_call("apply_patch", &args, ToolFailureClass::Transient).is_none());
    }

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ncx_recovery_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/value.txt"), "literal [value\n").unwrap();
        root
    }

    #[tokio::test]
    async fn registry_falls_back_from_invalid_regex_to_literal_search() {
        let root = fixture("literal");
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let registry = ToolRegistry::new(ctx);

        let result = registry
            .execute_with_recovery("grep", &json!({"pattern": "[value"}))
            .await;

        assert!(result.contains("grep -> grep_literal"), "{result}");
        assert!(result.contains("nested/value.txt:1"), "{result}");
    }

    #[tokio::test]
    async fn registry_treats_directory_read_as_directory_listing() {
        let root = fixture("directory");
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let registry = ToolRegistry::new(ctx);

        let result = registry
            .execute_with_recovery("read_file", &json!({"path": "nested"}))
            .await;

        assert!(result.contains("read_file -> list_directory"), "{result}");
        assert!(result.contains("value.txt"), "{result}");
    }

    #[tokio::test]
    async fn registry_reads_utf16_and_gb18030_files_without_mojibake() {
        let root = fixture("legacy_encoding");
        let utf16 = "UTF16 中文\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut utf16_with_bom = vec![0xFF, 0xFE];
        utf16_with_bom.extend(utf16);
        std::fs::write(root.join("nested/utf16.txt"), utf16_with_bom).unwrap();
        std::fs::write(
            root.join("nested/gb18030.txt"),
            [0xD6, 0xD0, 0xCE, 0xC4, 0xC4, 0xDA, 0xC8, 0xDD, b'\n'],
        )
        .unwrap();
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let registry = ToolRegistry::new(ctx);

        let utf16_result = registry
            .execute_with_recovery("read_file", &json!({"path": "nested/utf16.txt"}))
            .await;
        let gb18030_result = registry
            .execute_with_recovery("read_file", &json!({"path": "nested/gb18030.txt"}))
            .await;

        assert!(utf16_result.contains("UTF16 中文"), "{utf16_result}");
        assert!(gb18030_result.contains("中文内容"), "{gb18030_result}");
    }

    #[tokio::test]
    async fn registry_backtracks_unique_missing_read_by_basename() {
        let root = fixture("recursive_read");
        let deep = root.join("中文目录/第二层/第三层");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(root.join(".nanocodex/checkpoints/snapshot")).unwrap();
        std::fs::write(deep.join("唯一配置.toml"), "answer = 42\n").unwrap();
        std::fs::write(
            root.join(".nanocodex/checkpoints/snapshot/唯一配置.toml"),
            "stale = true\n",
        )
        .unwrap();
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let registry = ToolRegistry::new(ctx);

        let result = registry
            .execute_with_recovery("read_file", &json!({"path": "唯一配置.toml"}))
            .await;

        assert!(result.contains("answer = 42"), "{result}");
        assert!(
            result.contains("中文目录/第二层/第三层/唯一配置.toml"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn registry_does_not_guess_when_recursive_read_is_ambiguous() {
        let root = fixture("ambiguous_read");
        std::fs::create_dir_all(root.join("first")).unwrap();
        std::fs::create_dir_all(root.join("second/deep")).unwrap();
        std::fs::write(root.join("first/config.toml"), "owner = 'first'\n").unwrap();
        std::fs::write(root.join("second/deep/config.toml"), "owner = 'second'\n").unwrap();
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let registry = ToolRegistry::new(ctx);

        let result = registry
            .execute_with_recovery("read_file", &json!({"path": "config.toml"}))
            .await;

        assert!(result.contains("read_file -> find_files"), "{result}");
        assert!(result.contains("first/config.toml"), "{result}");
        assert!(result.contains("second/deep/config.toml"), "{result}");
        assert!(!result.contains("owner ="), "{result}");
    }

    struct FlakyReadTool {
        calls: Rc<Cell<usize>>,
    }

    struct FailingWriteTool {
        calls: Rc<Cell<usize>>,
    }

    #[async_trait(?Send)]
    impl Tool for FailingWriteTool {
        fn name(&self) -> &str {
            "failing_write"
        }

        fn description(&self) -> &str {
            "Mutating test tool that must never be retried automatically."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            self.calls.set(self.calls.get() + 1);
            "Error: service unavailable (status 503)".into()
        }
    }

    #[async_trait(?Send)]
    impl Tool for FlakyReadTool {
        fn name(&self) -> &str {
            "flaky_read"
        }

        fn description(&self) -> &str {
            "Read a remote value with a deterministic transient test failure."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn read_only(&self) -> bool {
            true
        }

        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            let call = self.calls.get() + 1;
            self.calls.set(call);
            if call == 1 {
                "Error: service unavailable (status 503)".into()
            } else {
                "recovered".into()
            }
        }
    }

    #[tokio::test]
    async fn registry_retries_transient_read_once() {
        let root = fixture("retry");
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let calls = Rc::new(Cell::new(0));
        let mut registry = ToolRegistry::empty(ctx);
        registry.register(Box::new(FlakyReadTool {
            calls: calls.clone(),
        }));

        let result = registry
            .execute_with_recovery("flaky_read", &json!({}))
            .await;

        assert!(result.contains("retried flaky_read"), "{result}");
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn registry_never_retries_mutating_tools() {
        let root = fixture("no_write_retry");
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let calls = Rc::new(Cell::new(0));
        let mut registry = ToolRegistry::empty(ctx);
        registry.register(Box::new(FailingWriteTool {
            calls: calls.clone(),
        }));

        let result = registry
            .execute_with_recovery("failing_write", &json!({}))
            .await;

        assert!(result.starts_with("Error:"), "{result}");
        assert_eq!(calls.get(), 1);
    }
}
