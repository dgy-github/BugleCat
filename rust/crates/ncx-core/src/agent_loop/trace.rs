//! Opt-in runtime tracing for model and tool execution.

use ncx_provider::ModelResponse;

use crate::session::ContextEditStats;

pub(super) fn enabled() -> bool {
    std::env::var("NCX_TRACE")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

pub(super) fn model_response(iteration: usize, response: &ModelResponse, stats: &ContextEditStats) {
    if !enabled() {
        return;
    }
    eprintln!(
        "[ncx-trace] iter={} finish={} n_tools={} ctx={}/{} compressed={} dropped={} content={:?}",
        iteration,
        response.finish_reason,
        response.tool_calls.len(),
        stats.edited_chars,
        stats.original_chars,
        stats.compressed_tool_results,
        stats.dropped_messages,
        truncate(&response.content, 120)
    );
    for call in &response.tool_calls {
        eprintln!(
            "[ncx-trace]   call {} args={}",
            call.name,
            truncate(&call.arguments.to_string(), 200)
        );
    }
}

pub(super) fn tool_result(tool_name: &str, result: &str) {
    if enabled() {
        eprintln!(
            "[ncx-trace]   result {} -> {:?}",
            tool_name,
            truncate(result, 200)
        );
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!(
            "{}...",
            text.chars().take(max.saturating_sub(3)).collect::<String>()
        )
    }
}
