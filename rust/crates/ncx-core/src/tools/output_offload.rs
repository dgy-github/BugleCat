//! Codex-rs / Goose style large tool output disk offloading.
//!
//! When a tool execution (`shell`, `grep`, `glob`, `process_poll`, `raw_terminal`,
//! MCP tools, etc.) produces a response larger than [`TOOL_OUTPUT_OFFLOAD_THRESHOLD`]
//! characters, truncating the middle inline would permanently discard compiler errors
//! or test tracebacks that occurred in the middle of the log. Conversely, keeping the
//! entire raw payload inline would rapidly exhaust the LLM context window.
//!
//! This module automatically persists the complete, untruncated tool output to an
//! external artifact file under `<temp_dir>/nanocodex-tool-outputs/` (outside the git
//! workspace so it never pollutes `git status` or workspace freshness verification)
//! and returns a compact **Head + Handle + Tail** preview. The model can then inspect
//! any omitted middle lines on demand via `read_file(path, offset, limit)`.

use std::path::PathBuf;

/// Maximum number of characters kept inline (split evenly between head and tail)
/// before a tool output is offloaded to disk.
pub const TOOL_OUTPUT_OFFLOAD_THRESHOLD: usize = 16_000;

/// Marker tag embedded in offloaded tool outputs to prevent double-offloading and
/// expose a machine/model-readable artifact handle.
pub const OFFLOAD_MARKER_PREFIX: &str = "[tool_output_offloaded:";

/// Directory under the system temporary folder where full tool outputs are stored.
fn offload_dir() -> PathBuf {
    std::env::temp_dir().join("nanocodex-tool-outputs")
}

/// If `output` exceeds [`TOOL_OUTPUT_OFFLOAD_THRESHOLD`], persist the complete
/// untruncated text to disk and return a head + tail preview with a `read_file`
/// handle pointing to the saved file.
///
/// `read_file` itself is excluded because it is the windowed file reader used to
/// inspect offloaded artifacts.
pub(crate) fn maybe_offload(tool_name: &str, output: String) -> String {
    if tool_name == "read_file" || output.contains(OFFLOAD_MARKER_PREFIX) {
        return output;
    }
    let total_chars = output.chars().count();
    if total_chars <= TOOL_OUTPUT_OFFLOAD_THRESHOLD {
        return output;
    }

    let chars: Vec<char> = output.chars().collect();
    let half = TOOL_OUTPUT_OFFLOAD_THRESHOLD / 2;
    let truncated = total_chars.saturating_sub(TOOL_OUTPUT_OFFLOAD_THRESHOLD);
    let head: String = chars[..half].iter().collect();
    let tail: String = chars[total_chars - half..].iter().collect();
    let total_lines = output.lines().count().max(1);
    let head_lines = head.lines().count().max(1);

    match persist_offload_artifact(tool_name, &output) {
        Some(path) => {
            let display_path = path.to_string_lossy().replace('\\', "/");
            format!(
                "{head}\n\n... ({truncated} chars truncated) ...\n\
                 {OFFLOAD_MARKER_PREFIX} path=\"{display_path}\" total_chars={total_chars} total_lines={total_lines} — \
                 use `read_file` with `path=\"{display_path}\"`, `offset={head_lines}`, and `limit` to inspect the omitted middle section]\n\n\
                 {tail}"
            )
        }
        None => format!("{head}\n\n... ({truncated} chars truncated) ...\n\n{tail}"),
    }
}

/// Write the full untruncated tool output to a content-addressed `.log` file under
/// the system temp directory.
fn persist_offload_artifact(tool_name: &str, content: &str) -> Option<PathBuf> {
    let dir = offload_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let safe_tool: String = tool_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    let hash = crate::context_trace::content_hash(content);
    let short_hash: String = hash.chars().take(16).collect();
    let file_path = dir.join(format!("{safe_tool}-{short_hash}.log"));
    std::fs::write(&file_path, content.as_bytes()).ok()?;
    Some(file_path)
}
