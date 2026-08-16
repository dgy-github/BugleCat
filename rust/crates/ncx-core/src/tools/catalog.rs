/// large to expose every schema every turn. Read-only.
pub struct ToolSearchTool;

#[async_trait(?Send)]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }
    fn description(&self) -> &str {
        "Search available tools by keyword when you need a capability that is not currently visible. Returns matching tool names and makes them available next turn."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Capability or tool keywords to search for."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
            },
            "required": ["query"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' is required and must be a string.".into();
        };
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 20) as usize;
        let q = tool_words(query);
        let catalog = ctx.tool_catalog.borrow();
        let mut scored: Vec<(i64, &ToolCatalogEntry)> = catalog
            .iter()
            .map(|e| (catalog_score(e, &q), e))
            .filter(|(s, _)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let mut hints = ctx.tool_hints.borrow_mut();
        hints.clear();
        if scored.is_empty() {
            return format!("No tools matched '{query}'.");
        }
        let mut out = format!("Tools matching '{query}':");
        for (_, entry) in scored.into_iter().take(max) {
            hints.push(entry.name.clone());
            let capabilities = entry
                .capabilities
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "\n- {}{} [{}]: {}",
                entry.name,
                if entry.read_only { " (read-only)" } else { "" },
                capabilities,
                entry.description
            ));
        }
        out
    }
}

fn tool_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in s
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
    {
        let w = raw.trim_matches('_');
        if w.len() >= 2 && !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out
}

fn catalog_score(entry: &ToolCatalogEntry, query_words: &[String]) -> i64 {
    if query_words.is_empty() {
        return 0;
    }
    let capability_text = entry
        .capabilities
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let hay = format!(
        "{} {} {}",
        entry.name.to_lowercase(),
        entry.description.to_lowercase(),
        capability_text
    );
    let mut score = 0;
    for q in query_words {
        if entry.name.eq_ignore_ascii_case(q) {
            score += 100;
        } else if entry.name.to_lowercase().contains(q) {
            score += 50;
        } else if hay.contains(q) {
            score += 20;
        }
    }
    score
}
