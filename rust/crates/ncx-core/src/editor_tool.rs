//! Harness-compatible literal editor layered on the existing patch boundary.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{ApplyPatchTool, ReadFileTool, Tool, ToolContext};

/// View, create, replace, or insert text without requiring the model to author V4A syntax.
pub struct StrReplaceEditorTool;

#[async_trait(?Send)]
impl Tool for StrReplaceEditorTool {
    fn name(&self) -> &str {
        "str_replace_editor"
    }

    fn description(&self) -> &str {
        "View, create, uniquely replace, or insert UTF-8 text in one file. Use this when a small \
         literal edit is less error-prone than authoring a patch. Mutations delegate to \
         apply_patch and retain its sandbox and approval behavior."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "enum": ["view", "create", "str_replace", "insert"]},
                "path": {"type": "string", "description": "Absolute or workspace-relative file path."},
                "old_str": {"type": "string", "description": "Unique existing text for str_replace."},
                "new_str": {"type": "string", "description": "Replacement, created content, or inserted text."},
                "insert_line": {"type": "integer", "minimum": 0, "description": "Insert after this one-based line; 0 inserts before the first line."}
            },
            "required": ["command", "path"]
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return "Error: 'command' is required and must be a string.".into();
        };
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return "Error: 'path' is required and must be a string.".into();
        };
        match command {
            "view" => ReadFileTool.execute(ctx, &json!({"path": path})).await,
            "create" => create(ctx, path, args).await,
            "str_replace" => replace(ctx, path, args).await,
            "insert" => insert(ctx, path, args).await,
            _ => format!("Error: unsupported editor command '{command}'."),
        }
    }
}

async fn create(ctx: &ToolContext, path: &str, args: &Value) -> String {
    let Some(content) = args.get("new_str").and_then(Value::as_str) else {
        return "Error: 'new_str' is required for create.".into();
    };
    let resolved = resolve(ctx, path);
    if resolved.exists() {
        return format!("Error: file already exists: {path}");
    }
    apply(ctx, add_patch(path, content)).await
}

async fn replace(ctx: &ToolContext, path: &str, args: &Value) -> String {
    let Some(old) = args.get("old_str").and_then(Value::as_str) else {
        return "Error: 'old_str' is required for str_replace.".into();
    };
    let Some(new) = args.get("new_str").and_then(Value::as_str) else {
        return "Error: 'new_str' is required for str_replace.".into();
    };
    if old.is_empty() {
        return "Error: 'old_str' must not be empty.".into();
    }
    let current = match read_utf8(ctx, path) {
        Ok(text) => text,
        Err(error) => return error,
    };
    let matches = current.match_indices(old).count();
    if matches != 1 {
        return format!("Error: old_str must match exactly once; found {matches} matches.");
    }
    let updated = current.replacen(old, new, 1);
    apply(ctx, update_patch(path, &current, &updated)).await
}

async fn insert(ctx: &ToolContext, path: &str, args: &Value) -> String {
    let Some(new) = args.get("new_str").and_then(Value::as_str) else {
        return "Error: 'new_str' is required for insert.".into();
    };
    let Some(line) = args.get("insert_line").and_then(Value::as_u64) else {
        return "Error: 'insert_line' is required for insert.".into();
    };
    let current = match read_utf8(ctx, path) {
        Ok(text) => text,
        Err(error) => return error,
    };
    let mut lines = current.lines().map(str::to_string).collect::<Vec<_>>();
    if line as usize > lines.len() {
        return format!(
            "Error: insert_line {line} exceeds file length {}.",
            lines.len()
        );
    }
    let inserted = new.lines().map(str::to_string).collect::<Vec<_>>();
    lines.splice(line as usize..line as usize, inserted);
    let mut updated = lines.join("\n");
    if current.ends_with('\n') || new.ends_with('\n') {
        updated.push('\n');
    }
    apply(ctx, update_patch(path, &current, &updated)).await
}

fn read_utf8(ctx: &ToolContext, path: &str) -> Result<String, String> {
    let resolved = resolve(ctx, path);
    if !ctx.policy.can_read(&resolved) {
        return Err(format!(
            "Error: reading {path} is not allowed under the sandbox policy."
        ));
    }
    std::fs::read_to_string(&resolved).map_err(|error| format!("Error reading file: {error}"))
}

fn resolve(ctx: &ToolContext, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        ctx.workspace.join(candidate)
    };
    joined.canonicalize().unwrap_or(joined)
}

fn add_patch(path: &str, content: &str) -> String {
    let body = content
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("*** Begin Patch\n*** Add File: {path}\n{body}\n*** End Patch")
}

fn update_patch(path: &str, old: &str, new: &str) -> String {
    let removed = old
        .lines()
        .map(|line| format!("-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added = new
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("*** Begin Patch\n*** Update File: {path}\n@@\n{removed}\n{added}\n*** End Patch")
}

async fn apply(ctx: &ToolContext, patch: String) -> String {
    ApplyPatchTool.execute(ctx, &json!({"patch": patch})).await
}

#[cfg(test)]
mod tests {
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

    use super::*;

    fn fixture(name: &str) -> (PathBuf, ToolContext) {
        let root = std::env::temp_dir().join(format!("ncx_editor_{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("sample.txt"), "alpha\nbeta\n").unwrap();
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &root));
        (root, ctx)
    }

    #[tokio::test]
    async fn replaces_unique_text_through_patch_boundary() {
        let (root, ctx) = fixture("replace");
        let result = StrReplaceEditorTool.execute(
            &ctx,
            &json!({"command": "str_replace", "path": "sample.txt", "old_str": "beta", "new_str": "gamma"}),
        ).await;
        assert!(result.contains("Patch applied successfully"), "{result}");
        assert_eq!(
            std::fs::read_to_string(root.join("sample.txt")).unwrap(),
            "alpha\ngamma\n"
        );
    }

    #[tokio::test]
    async fn refuses_ambiguous_replacement_without_writing() {
        let (root, ctx) = fixture("ambiguous");
        std::fs::write(root.join("sample.txt"), "same\nsame\n").unwrap();
        let result = StrReplaceEditorTool.execute(
            &ctx,
            &json!({"command": "str_replace", "path": "sample.txt", "old_str": "same", "new_str": "changed"}),
        ).await;
        assert!(result.contains("found 2 matches"), "{result}");
        assert_eq!(
            std::fs::read_to_string(root.join("sample.txt")).unwrap(),
            "same\nsame\n"
        );
    }

    #[tokio::test]
    async fn creates_and_inserts_text_through_patch_boundary() {
        let (root, ctx) = fixture("create_insert");
        let created = StrReplaceEditorTool
            .execute(
                &ctx,
                &json!({"command": "create", "path": "created.txt", "new_str": "first\nthird\n"}),
            )
            .await;
        assert!(created.contains("Patch applied successfully"), "{created}");
        let inserted = StrReplaceEditorTool
            .execute(
                &ctx,
                &json!({"command": "insert", "path": "created.txt", "insert_line": 1, "new_str": "second"}),
            )
            .await;
        assert!(
            inserted.contains("Patch applied successfully"),
            "{inserted}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("created.txt")).unwrap(),
            "first\nsecond\nthird\n"
        );
    }

    #[tokio::test]
    async fn plan_mode_refuses_editor_mutations() {
        let (root, ctx) = fixture("plan");
        let ctx = ctx.with_plan_mode(true);
        let result = StrReplaceEditorTool
            .execute(
                &ctx,
                &json!({"command": "str_replace", "path": "sample.txt", "old_str": "beta", "new_str": "changed"}),
            )
            .await;
        assert!(result.contains("plan mode"), "{result}");
        assert_eq!(
            std::fs::read_to_string(root.join("sample.txt")).unwrap(),
            "alpha\nbeta\n"
        );
    }
}
