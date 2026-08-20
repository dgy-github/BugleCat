//! Cross-platform workspace inspection tools that avoid shell-specific syntax.
//!
//! Directory and path inspection use filesystem APIs directly. Git tools run
//! fixed read-only commands through [`PolicyExecutor`]; model-provided shell
//! fragments never cross the process boundary.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use ncx_tools::PolicyExecutor;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext};

const DEFAULT_LIST_LIMIT: usize = 200;
const MAX_LIST_LIMIT: usize = 1_000;
const DEFAULT_LIST_DEPTH: usize = 1;
const MAX_LIST_DEPTH: usize = 25;

/// List a directory tree through native filesystem APIs.
pub struct ListDirectoryTool;

#[async_trait(?Send)]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List files and folders with structured JSON without using shell commands. Use this \
         instead of ls, dir, find, Get-ChildItem, or shell when inspecting a project tree. \
         Supports bounded recursion and deterministic sorting on Windows, macOS, and Linux."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path, absolute or workspace-relative. Defaults to '.'."
                },
                "depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_LIST_DEPTH,
                    "description": "Nested directory levels to include; 0 lists direct children only."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIST_LIMIT,
                    "description": "Maximum number of entries returned."
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include dot-prefixed files and directories."
                }
            }
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let depth = bounded_usize(args, "depth", DEFAULT_LIST_DEPTH, 0, MAX_LIST_DEPTH);
        let limit = bounded_usize(args, "limit", DEFAULT_LIST_LIMIT, 1, MAX_LIST_LIMIT);
        let include_hidden = args
            .get("include_hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resolved = resolve_path(ctx, path);

        if !ctx.policy.can_read(&resolved) {
            return format!("Error: reading {path} is not allowed under the sandbox policy.");
        }
        if !resolved.exists() {
            return format!("Error: directory not found: {path}");
        }
        if !resolved.is_dir() {
            return format!("Error: not a directory: {path}");
        }

        match collect_entries(&resolved, depth, limit, include_hidden) {
            Ok((entries, truncated)) => json!({
                "path": display_path(ctx, &resolved),
                "entries": entries,
                "truncated": truncated
            })
            .to_string(),
            Err(error) => format!("Error: cannot list directory {path}: {error}"),
        }
    }
}

/// Return structured metadata for one path, including a non-error missing state.
pub struct PathInfoTool;

#[async_trait(?Send)]
impl Tool for PathInfoTool {
    fn name(&self) -> &str {
        "path_info"
    }

    fn description(&self) -> &str {
        "Check whether a file or directory exists and return structured type, size, and modified \
         time metadata. Use this instead of shell test, stat, dir, or Test-Path. A missing path is \
         reported as exists=false rather than a failed tool call."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to inspect, absolute or workspace-relative."
                }
            },
            "required": ["path"]
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return "Error: 'path' is required and must be a string.".into();
        };
        let resolved = resolve_path(ctx, path);
        if !ctx.policy.can_read(&resolved) {
            return format!("Error: reading {path} is not allowed under the sandbox policy.");
        }

        match std::fs::symlink_metadata(&resolved) {
            Ok(metadata) => path_metadata(ctx, &resolved, &metadata).to_string(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({
                "path": display_path(ctx, &resolved),
                "exists": false,
                "type": "missing"
            })
            .to_string(),
            Err(error) => format!("Error: cannot inspect path {path}: {error}"),
        }
    }
}

/// Show repository state with a stable, read-only Git command.
pub struct GitStatusTool;

#[async_trait(?Send)]
impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Show the current Git branch and changed/untracked files using porcelain output. This is \
         a fixed read-only command; use it instead of composing git status through shell."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_untracked": {
                    "type": "boolean",
                    "description": "Include untracked files (default true)."
                }
            }
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let include_untracked = args
            .get("include_untracked")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let command = git_status_command(include_untracked);
        run_git(ctx, self.name(), command).await
    }
}

/// Show tracked workspace changes with fixed Git diff options.
pub struct GitDiffTool;

#[async_trait(?Send)]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show tracked Git changes for the workspace. Choose unstaged or staged changes and an \
         optional summary; arbitrary command text is not accepted. Use this instead of shelling \
         out to git diff."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Show staged changes instead of unstaged changes."
                },
                "stat": {
                    "type": "boolean",
                    "description": "Return a diff summary instead of the full patch."
                }
            }
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let staged = args.get("staged").and_then(Value::as_bool).unwrap_or(false);
        let stat = args.get("stat").and_then(Value::as_bool).unwrap_or(false);
        let result = run_git(ctx, self.name(), git_diff_command(staged, stat)).await;
        if result.trim().is_empty() {
            "No changes.".to_string()
        } else {
            result
        }
    }
}

fn bounded_usize(args: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| (value as usize).clamp(min, max))
        .unwrap_or(default)
}

fn resolve_path(ctx: &ToolContext, path: &str) -> PathBuf {
    let input = PathBuf::from(path);
    let joined = if input.is_absolute() {
        input
    } else {
        ctx.workspace.join(input)
    };
    joined.canonicalize().unwrap_or(joined)
}

fn display_path(ctx: &ToolContext, path: &Path) -> String {
    let workspace = ctx
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| ctx.workspace.clone());
    let shown = path.strip_prefix(&workspace).unwrap_or(path);
    let text = shown.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        ".".to_string()
    } else {
        text
    }
}

fn collect_entries(
    root: &Path,
    max_depth: usize,
    limit: usize,
    include_hidden: bool,
) -> Result<(Vec<Value>, bool), std::io::Error> {
    let mut entries = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, level)) = pending.pop() {
        let mut children = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
        for entry in children {
            let name = entry.file_name().to_string_lossy().to_string();
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            if entries.len() >= limit {
                return Ok((entries, true));
            }
            let path = entry.path();
            let file_type = entry.file_type()?;
            let kind = entry_kind(&file_type);
            let size = if file_type.is_file() {
                entry.metadata().ok().map(|metadata| metadata.len())
            } else {
                None
            };
            entries.push(json!({
                "path": path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/"),
                "type": kind,
                "size_bytes": size
            }));
            if file_type.is_dir() && level < max_depth {
                pending.push((path, level + 1));
            }
        }
    }
    entries.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    Ok((entries, false))
}

fn entry_kind(file_type: &std::fs::FileType) -> &'static str {
    if file_type.is_symlink() {
        "symlink"
    } else if file_type.is_dir() {
        "directory"
    } else if file_type.is_file() {
        "file"
    } else {
        "other"
    }
}

fn path_metadata(ctx: &ToolContext, path: &Path, metadata: &std::fs::Metadata) -> Value {
    let modified_unix_ms = metadata.modified().ok().and_then(|modified| {
        modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_millis() as u64)
    });
    let file_type = metadata.file_type();
    json!({
        "path": display_path(ctx, path),
        "exists": true,
        "type": entry_kind(&file_type),
        "size_bytes": if file_type.is_file() { Some(metadata.len()) } else { None },
        "modified_unix_ms": modified_unix_ms
    })
}

fn git_status_command(include_untracked: bool) -> &'static str {
    if include_untracked {
        "git status --porcelain=v1 --branch --untracked-files=all"
    } else {
        "git status --porcelain=v1 --branch --untracked-files=no"
    }
}

fn git_diff_command(staged: bool, stat: bool) -> &'static str {
    match (staged, stat) {
        (false, false) => "git diff --no-ext-diff --no-color",
        (false, true) => "git diff --no-ext-diff --no-color --stat",
        (true, false) => "git diff --no-ext-diff --no-color --cached",
        (true, true) => "git diff --no-ext-diff --no-color --cached --stat",
    }
}

async fn run_git(ctx: &ToolContext, tool_name: &str, command: &str) -> String {
    let result = PolicyExecutor::new()
        .run(command, &ctx.workspace, ctx.timeout_s)
        .await;
    if result.ok() {
        result.stdout.trim_end().to_string()
    } else {
        format!("Error: {tool_name} failed.\n{}", result.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ncx_workspace_tools_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("README.md"), "hello").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        std::fs::write(root.join("src/nested/deep.rs"), "deep\n").unwrap();
        root
    }

    fn context(root: &Path) -> ToolContext {
        ToolContext::new(root.to_path_buf(), SandboxPolicy::new(READ_ONLY, root))
    }

    #[tokio::test]
    async fn list_directory_is_sorted_bounded_and_cross_platform() {
        let root = fixture("list");
        let result = ListDirectoryTool
            .execute(&context(&root), &json!({"path": ".", "depth": 0}))
            .await;
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["path"], ".");
        assert_eq!(parsed["truncated"], false);
        assert_eq!(parsed["entries"][0]["path"], "README.md");
        assert_eq!(parsed["entries"][1]["path"], "src");
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 2);

        let limited = ListDirectoryTool
            .execute(&context(&root), &json!({"limit": 1}))
            .await;
        let parsed: Value = serde_json::from_str(&limited).unwrap();
        assert_eq!(parsed["truncated"], true);
        assert_eq!(parsed["entries"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn list_directory_depth_controls_recursion() {
        let root = fixture("depth");
        let result = ListDirectoryTool
            .execute(&context(&root), &json!({"path": "src", "depth": 1}))
            .await;
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let paths = parsed["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["lib.rs", "nested", "nested/deep.rs"]);
    }

    #[tokio::test]
    async fn path_info_reports_existing_and_missing_paths() {
        let root = fixture("info");
        let existing = PathInfoTool
            .execute(&context(&root), &json!({"path": "README.md"}))
            .await;
        let parsed: Value = serde_json::from_str(&existing).unwrap();
        assert_eq!(parsed["exists"], true);
        assert_eq!(parsed["type"], "file");
        assert_eq!(parsed["size_bytes"], 5);

        let missing = PathInfoTool
            .execute(&context(&root), &json!({"path": "missing.txt"}))
            .await;
        let parsed: Value = serde_json::from_str(&missing).unwrap();
        assert_eq!(parsed["exists"], false);
        assert_eq!(parsed["type"], "missing");
    }

    #[test]
    fn git_commands_only_vary_fixed_read_only_options() {
        assert_eq!(
            git_status_command(false),
            "git status --porcelain=v1 --branch --untracked-files=no"
        );
        assert_eq!(
            git_diff_command(true, true),
            "git diff --no-ext-diff --no-color --cached --stat"
        );
    }

    #[test]
    fn registry_keeps_workspace_tools_visible_and_git_tools_discoverable() {
        let root = fixture("registry");
        let registry = ToolRegistry::new(context(&root));
        for name in ["list_directory", "path_info", "git_status", "git_diff"] {
            assert!(
                registry.get(name).is_some(),
                "missing registered tool {name}"
            );
        }

        let default_names = schema_names(registry.schemas_for_query(""));
        assert!(default_names.iter().any(|name| name == "list_directory"));
        assert!(default_names.iter().any(|name| name == "path_info"));

        let git_names = schema_names(registry.schemas_for_query("git status diff"));
        assert!(git_names.iter().any(|name| name == "git_status"));
        assert!(git_names.iter().any(|name| name == "git_diff"));
    }

    fn schema_names(schemas: Vec<Value>) -> Vec<String> {
        schemas
            .iter()
            .filter_map(|schema| schema["function"]["name"].as_str().map(str::to_string))
            .collect()
    }
}
