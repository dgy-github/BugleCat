use super::*;

#[derive(Debug, Default, Clone)]
pub(crate) struct UsageTracker {
    last: Option<TurnUsage>,
    total: BTreeMap<String, i64>,
    total_model_calls: usize,
    total_tool_calls: usize,
}

#[derive(Debug, Clone)]
struct TurnUsage {
    usage: BTreeMap<String, i64>,
    model_calls: usize,
    tool_calls: usize,
    stop_reason: String,
}

impl UsageTracker {
    pub(crate) fn record(&mut self, result: &TurnResult) {
        self.total_model_calls += result.iterations;
        self.total_tool_calls += result.tools_used.len();
        add_usage(&mut self.total, &result.usage);
        self.last = Some(TurnUsage {
            usage: result.usage.clone(),
            model_calls: result.iterations,
            tool_calls: result.tools_used.len(),
            stop_reason: result.stop_reason.clone(),
        });
    }

    pub(crate) fn render(&self) -> String {
        let Some(last) = &self.last else {
            return "No token usage recorded yet.".into();
        };
        format!(
            "Last turn:\n{}\n\nSession total:\n{}\n\nCost: raw token usage only; no Rust price table is configured.",
            format_usage_block(
                last.model_calls,
                last.tool_calls,
                Some(&last.stop_reason),
                &last.usage
            ),
            format_usage_block(
                self.total_model_calls,
                self.total_tool_calls,
                None,
                &self.total
            )
        )
    }
}

fn add_usage(total: &mut BTreeMap<String, i64>, usage: &BTreeMap<String, i64>) {
    for (key, value) in usage {
        *total.entry(key.clone()).or_insert(0) += *value;
    }
}

fn format_usage_block(
    model_calls: usize,
    tool_calls: usize,
    stop_reason: Option<&str>,
    usage: &BTreeMap<String, i64>,
) -> String {
    let prompt = usage_value(usage, "prompt_tokens");
    let completion = usage_value(usage, "completion_tokens");
    let hit = usage_value(usage, "prompt_cache_hit_tokens");
    let miss = usage_value(usage, "prompt_cache_miss_tokens");
    let total = prompt + completion;
    let mut lines = vec![
        format!("model_calls: {model_calls}"),
        format!("tool_calls:  {tool_calls}"),
    ];
    if let Some(reason) = stop_reason {
        lines.push(format!("stop_reason: {reason}"));
    }
    lines.push(format!("prompt_tokens:     {prompt}"));
    lines.push(format!("completion_tokens: {completion}"));
    lines.push(format!("total_tokens:      {total}"));
    if hit > 0 || miss > 0 {
        lines.push(format!("prompt_cache_hit_tokens:  {hit}"));
        lines.push(format!("prompt_cache_miss_tokens: {miss}"));
    }
    lines.join("\n")
}

fn usage_value(usage: &BTreeMap<String, i64>, key: &str) -> i64 {
    usage.get(key).copied().unwrap_or(0)
}
