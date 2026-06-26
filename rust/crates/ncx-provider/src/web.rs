//! Keyless web lookup via DuckDuckGo's Instant Answer API.
//!
//! Lives here because this crate already has `reqwest` + rustls. The IA API is
//! free and needs no key, but only returns instant answers (facts, definitions,
//! disambiguation) — NOT general web-crawl results. Good enough for "what is X"
//! style lookups; a keyed backend (Tavily/Brave) would be the upgrade.

use std::time::Duration;

use serde_json::Value;

const ENDPOINT: &str = "https://api.duckduckgo.com/";
const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

/// Keyed general web search via Tavily (built for LLM agents — returns clean
/// title/url/content). Needs an API key. Falls back to nothing here; the caller
/// (WebSearchTool) decides whether to use this or [`ddg_instant_answer`].
pub async fn tavily_search(query: &str, api_key: &str, max_results: u32) -> Result<String, String> {
    if api_key.is_empty() {
        return Err("no Tavily API key configured".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("client error: {e}"))?;
    let body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "max_results": max_results,
        "search_depth": "basic",
    });
    let resp = client
        .post(TAVILY_ENDPOINT)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("decode error: {e}"))?;
    Ok(format_tavily(&v, query))
}

/// Build a compact block from a Tavily response (`answer` + `results[]`).
fn format_tavily(v: &Value, query: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(a) = v
        .get("answer")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("Answer: {a}"));
    }
    if let Some(results) = v.get("results").and_then(|x| x.as_array()) {
        for r in results.iter().take(6) {
            let title = r.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let url = r.get("url").and_then(|x| x.as_str()).unwrap_or("");
            let content = r.get("content").and_then(|x| x.as_str()).unwrap_or("");
            let snippet = if content.len() > 300 {
                &content[..300]
            } else {
                content
            };
            parts.push(format!(
                "- {title} ({url})\n  {}",
                snippet.replace('\n', " ")
            ));
        }
    }
    if parts.is_empty() {
        format!("No Tavily results for {query:?}.")
    } else {
        parts.join("\n")
    }
}

/// Query the DuckDuckGo Instant Answer API and return a compact text summary.
pub async fn ddg_instant_answer(query: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("client error: {e}"))?;
    let resp = client
        .get(ENDPOINT)
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
            ("t", "nanocodex"),
        ])
        .header("User-Agent", "nanocodex/0.1 (+https://localhost)")
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("decode error: {e}"))?;
    Ok(format_answer(&v, query))
}

/// Pull the useful fields out of an IA response into a short block.
fn format_answer(v: &Value, query: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(a) = v
        .get("Answer")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("Answer: {a}"));
    }
    if let Some(abs) = v
        .get("AbstractText")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        let url = v.get("AbstractURL").and_then(|x| x.as_str()).unwrap_or("");
        if url.is_empty() {
            parts.push(abs.to_string());
        } else {
            parts.push(format!("{abs}\n  {url}"));
        }
    }
    if let Some(def) = v
        .get("Definition")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        let url = v
            .get("DefinitionURL")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        parts.push(if url.is_empty() {
            format!("Definition: {def}")
        } else {
            format!("Definition: {def}\n  {url}")
        });
    }
    if let Some(rt) = v.get("RelatedTopics").and_then(|x| x.as_array()) {
        for t in rt.iter().take(5) {
            if let Some(text) = t
                .get("Text")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                let url = t.get("FirstURL").and_then(|x| x.as_str()).unwrap_or("");
                if url.is_empty() {
                    parts.push(format!("- {text}"));
                } else {
                    parts.push(format!("- {text} ({url})"));
                }
            }
        }
    }

    if parts.is_empty() {
        format!(
            "No instant answer for {query:?}. DuckDuckGo's Instant Answer API only covers \
             factual/definitional queries, not general web results — try a more specific factual \
             query, or wire a keyed search backend (Tavily/Brave) for broad search."
        )
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_abstract_and_related() {
        let v = json!({
            "Answer": "",
            "AbstractText": "Rust is a systems programming language.",
            "AbstractURL": "https://example.com/rust",
            "RelatedTopics": [{"Text": "Cargo - the build tool", "FirstURL": "https://example.com/cargo"}],
        });
        let out = format_answer(&v, "rust");
        assert!(out.contains("systems programming language"));
        assert!(out.contains("https://example.com/rust"));
        assert!(out.contains("Cargo - the build tool"));
    }

    #[test]
    fn empty_response_explains_limits() {
        let out = format_answer(&json!({}), "some obscure thing");
        assert!(out.contains("No instant answer"));
        assert!(out.contains("Tavily/Brave"));
    }

    #[test]
    fn tavily_formats_answer_and_results() {
        let v = json!({
            "answer": "Rust is a systems language.",
            "results": [
                {"title": "Rust", "url": "https://rust-lang.org", "content": "A language empowering everyone."},
            ],
        });
        let out = format_tavily(&v, "rust");
        assert!(out.contains("Answer: Rust is a systems language."));
        assert!(out.contains("https://rust-lang.org"));
        assert!(out.contains("empowering everyone"));
    }

    #[tokio::test]
    async fn tavily_without_key_errors() {
        assert!(tavily_search("q", "", 5).await.is_err());
    }
}
