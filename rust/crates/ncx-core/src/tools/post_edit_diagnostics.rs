//! OpenCode-style automatic post-edit diagnostics feedback.
//!
//! After `apply_patch` or `str_replace_editor` mutates workspace files, the harness
//! immediately inspects the touched files and appends any static syntax errors or
//! Language Server (`LspProvider`) diagnostics directly to the tool output. This
//! saves an entire model turn that would otherwise be spent discovering syntax or
//! type errors via a separate tool call.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::lsp_tool::LspRequest;
use crate::tools::ToolContext;

/// Maximum number of diagnostics reported per edited file to keep tool output compact.
const MAX_DIAGNOSTICS_PER_FILE: usize = 10;

/// Timeout for automatic post-edit LSP diagnostic queries so a busy language server
/// never stalls file editing.
const POST_EDIT_LSP_TIMEOUT: Duration = Duration::from_secs(5);

/// Collect post-edit diagnostics for all non-deleted files touched by a patch.
///
/// Returns an empty `Vec` when all modified files are clean, ensuring zero token
/// overhead on healthy edits.
pub(crate) async fn collect(ctx: &ToolContext, relative_paths: &[String]) -> Vec<String> {
    let mut findings = Vec::new();
    for rel_path in relative_paths {
        let abs_path = ctx.workspace.join(rel_path);
        let resolved = abs_path.canonicalize().unwrap_or(abs_path);
        if !ctx.policy.can_read(&resolved) || !resolved.is_file() {
            continue;
        }
        // 1. Fast in-process syntax & conflict-marker checks for structured files and source code.
        if let Ok(text) = std::fs::read_to_string(&resolved) {
            check_conflict_markers(rel_path, &text, &mut findings);
            check_structured_syntax(rel_path, &resolved, &text, &mut findings);
        }

        // 2. Live LSP diagnostics when an `LspProvider` (e.g. `rust-analyzer`) is attached.
        if let Some(provider) = &ctx.lsp_provider {
            let request = LspRequest {
                operation: "diagnostics".to_string(),
                path: Some(rel_path.clone()),
                line: None,
                column: None,
                query: None,
            };
            if let Ok(Ok(payload)) =
                tokio::time::timeout(POST_EDIT_LSP_TIMEOUT, provider.request(request)).await
            {
                extract_lsp_diagnostics(rel_path, &payload, &mut findings);
            }
        }
    }
    findings
}

/// Format collected diagnostics as a compact block appended to `apply_patch` output.
pub(crate) fn format_block(findings: &[String]) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let body = findings
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n[post_edit_diagnostics: {} issue(s) detected — review and repair before finishing]\n{body}",
        findings.len()
    )
}

/// Detect unresolved Git merge conflict markers left inside source/config files.
fn check_conflict_markers(rel_path: &str, text: &str, findings: &mut Vec<String>) {
    let ext = Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "rs" | "py"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "go"
            | "c"
            | "cpp"
            | "h"
            | "java"
    ) {
        return;
    }
    for (idx, line) in text.lines().enumerate() {
        if line.starts_with("<<<<<<< ") || line == "=======" || line.starts_with(">>>>>>> ") {
            findings.push(format!(
                "{rel_path}:{}:1 [error] unresolved git merge conflict marker `{}`",
                idx + 1,
                line.chars().take(15).collect::<String>()
            ));
            break;
        }
    }
}

/// Perform zero-subprocess syntax validation for `.json` and `.toml` files.
fn check_structured_syntax(
    rel_path: &str,
    resolved: &Path,
    text: &str,
    findings: &mut Vec<String>,
) {
    let Some(ext) = resolved.extension().and_then(|e| e.to_str()) else {
        return;
    };
    if ext.eq_ignore_ascii_case("json") {
        if let Err(err) = serde_json::from_str::<Value>(text) {
            findings.push(format!(
                "{rel_path}:{}:{} [error] JSON syntax error: {err}",
                err.line(),
                err.column()
            ));
        }
    } else if ext.eq_ignore_ascii_case("toml") {
        if let Err(err) = toml::from_str::<toml::Value>(text) {
            let msg = err
                .message()
                .lines()
                .next()
                .unwrap_or("invalid TOML syntax");
            findings.push(format!("{rel_path} [error] TOML syntax error: {msg}"));
        }
    }
}

/// Extract human-readable error/warning items from an LSP `textDocument/diagnostic` response.
///
/// Supports both raw `Vec<Diagnostic>` arrays and LSP 3.17 `FullDocumentDiagnosticReport`
/// objects (`{"kind": "full", "items": [...]}` or `{"diagnostics": [...]}`).
fn extract_lsp_diagnostics(rel_path: &str, payload: &Value, findings: &mut Vec<String>) {
    let items = payload
        .as_array()
        .or_else(|| payload.get("items").and_then(Value::as_array))
        .or_else(|| payload.get("diagnostics").and_then(Value::as_array));
    let Some(items) = items else {
        return;
    };

    let mut count = 0usize;
    for diag in items {
        if count >= MAX_DIAGNOSTICS_PER_FILE {
            break;
        }
        // LSP DiagnosticSeverity: 1 = Error, 2 = Warning, 3 = Information, 4 = Hint.
        // Only surface Errors and Warnings automatically after edits.
        let severity_num = diag.get("severity").and_then(Value::as_u64).unwrap_or(1);
        if severity_num > 2 {
            continue;
        }
        let severity_label = match severity_num {
            1 => "error",
            2 => "warning",
            _ => "diagnostic",
        };
        let Some(message) = diag
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|m| !m.is_empty())
        else {
            continue;
        };
        let first_line = message.lines().next().unwrap_or(message);
        let start = diag.get("range").and_then(|r| r.get("start"));
        let line = start
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        let col = start
            .and_then(|s| s.get("character"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;

        findings.push(format!(
            "{rel_path}:{line}:{col} [{severity_label}] {first_line}"
        ));
        count += 1;
    }
}
