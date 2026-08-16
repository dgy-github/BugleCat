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
const WIKIPEDIA_ENDPOINT: &str = "https://en.wikipedia.org/w/api.php";
const BING_ENDPOINT: &str = "https://www.bing.com/search";

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

/// Keyless fallback search using Wikipedia's OpenSearch API.
pub async fn wikipedia_search(query: &str, limit: u32) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("client error: {e}"))?;
    let resp = client
        .get(WIKIPEDIA_ENDPOINT)
        .query(&[
            ("action", "opensearch"),
            ("search", query),
            ("limit", &limit.min(10).to_string()),
            ("namespace", "0"),
            ("format", "json"),
        ])
        .header("User-Agent", "nanocodex/0.1 (+https://localhost)")
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let value: Value = resp
        .json()
        .await
        .map_err(|e| format!("decode error: {e}"))?;
    format_wikipedia(&value, query)
}

fn format_wikipedia(value: &Value, query: &str) -> Result<String, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "response was not an array".to_string())?;
    let titles = rows.get(1).and_then(Value::as_array);
    let descriptions = rows.get(2).and_then(Value::as_array);
    let urls = rows.get(3).and_then(Value::as_array);
    let Some(titles) = titles else {
        return Err("response omitted result titles".to_string());
    };
    let lines = titles
        .iter()
        .enumerate()
        .filter_map(|(index, title)| {
            let title = title.as_str()?;
            let description = descriptions
                .and_then(|items| items.get(index))
                .and_then(Value::as_str)
                .unwrap_or("");
            let url = urls
                .and_then(|items| items.get(index))
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(format!("- {title} ({url})\n  {description}"))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        Ok(format!("No Wikipedia results for {query:?}."))
    } else {
        Ok(lines.join("\n"))
    }
}

/// Keyless broad-search fallback using Bing's RSS response.
pub async fn bing_rss_search(query: &str, limit: usize) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("client error: {e}"))?;
    let response = client
        .get(BING_ENDPOINT)
        .query(&[("format", "rss"), ("q", query)])
        .header("User-Agent", "nanocodex/0.1 (+https://localhost)")
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let xml = response
        .text()
        .await
        .map_err(|e| format!("read error: {e}"))?;
    format_bing_rss(&xml, query, limit)
}

fn format_bing_rss(xml: &str, query: &str, limit: usize) -> Result<String, String> {
    let document = roxmltree::Document::parse(xml).map_err(|e| format!("XML error: {e}"))?;
    let lines = document
        .descendants()
        .filter(|node| node.has_tag_name("item"))
        .take(limit.min(10))
        .filter_map(|item| {
            let field = |name| {
                item.children()
                    .find(|child| child.has_tag_name(name))
                    .and_then(|child| child.text())
                    .unwrap_or("")
            };
            let title = field("title");
            if title.is_empty() {
                return None;
            }
            Some(format!(
                "- {title} ({})\n  {}",
                field("link"),
                html_to_text(field("description"))
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        Ok(format!("No Bing results for {query:?}."))
    } else {
        Ok(lines.join("\n"))
    }
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

const MAX_FETCH_BYTES: usize = 2_000_000;
const MAX_TEXT_CHARS: usize = 12_000;

/// Fetch a URL and return readable text (HTML stripped). Complements the keyless
/// `web_search` with "read this page". Caps size; errors on non-2xx / empty text.
pub async fn fetch_url(url: &str) -> Result<String, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("url must start with http:// or https://".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("client error: {e}"))?;
    let resp = client
        .get(url)
        .header("User-Agent", "nanocodex/0.1 (+https://localhost)")
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let bytes = resp.bytes().await.map_err(|e| format!("read error: {e}"))?;
    let slice = if bytes.len() > MAX_FETCH_BYTES {
        &bytes[..MAX_FETCH_BYTES]
    } else {
        &bytes[..]
    };
    let body = String::from_utf8_lossy(slice);
    let mut text = if ctype.contains("html") || body.trim_start().starts_with('<') {
        html_to_text(&body)
    } else {
        body.to_string()
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        text = text.chars().take(MAX_TEXT_CHARS).collect::<String>();
        text.push_str("\n\u{2026} (truncated)");
    }
    if text.trim().is_empty() {
        Err("fetched page had no extractable text".to_string())
    } else {
        Ok(text)
    }
}

/// Minimal HTML→text: drop <script>/<style> blocks, strip tags, decode a few
/// entities, collapse whitespace. Dependency-light (no scraper crate).
pub fn html_to_text(html: &str) -> String {
    let chars: Vec<char> = html.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            // tag name (skip a leading '/')
            let mut j = i + 1;
            while j < chars.len() && chars[j] == '/' {
                j += 1;
            }
            let name: String = chars[j..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            let tag_end = chars[i..]
                .iter()
                .position(|&c| c == '>')
                .map(|p| i + p)
                .unwrap_or(chars.len() - 1);
            if name == "script" || name == "style" {
                let close = format!("</{name}>");
                match find_ci(&chars, tag_end, &close) {
                    Some(end) => i = end + close.chars().count(),
                    None => i = chars.len(),
                }
                continue;
            }
            out.push(' '); // tag boundary becomes whitespace
            i = tag_end + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    // collapse runs of whitespace to single spaces
    let mut result = String::with_capacity(decoded.len());
    let mut last_ws = false;
    for c in decoded.chars() {
        if c.is_whitespace() {
            if !last_ws {
                result.push(' ');
                last_ws = true;
            }
        } else {
            result.push(c);
            last_ws = false;
        }
    }
    result.trim().to_string()
}

/// Case-insensitive search for `needle` in `chars` starting at `from`.
fn find_ci(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let nl: Vec<char> = needle.to_ascii_lowercase().chars().collect();
    let n = nl.len();
    if n == 0 || chars.len() < n {
        return None;
    }
    for start in from..=chars.len() - n {
        if (0..n).all(|k| chars[start + k].to_ascii_lowercase() == nl[k]) {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn html_to_text_strips_tags_and_scripts() {
        let html = "<html><head><style>.x{color:red}</style></head><body><h1>Title</h1>                    <script>var a=1;</script><p>Hello&nbsp;&amp; welcome</p></body></html>";
        let t = html_to_text(html);
        assert!(t.contains("Title"));
        assert!(t.contains("Hello & welcome"));
        assert!(!t.contains("color:red"), "style content leaked: {t}");
        assert!(!t.contains("var a"), "script content leaked: {t}");
    }

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

    #[test]
    fn wikipedia_formats_parallel_result_arrays() {
        let value = json!([
            "rust",
            ["Rust (programming language)"],
            ["A systems programming language."],
            ["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
        ]);
        let output = format_wikipedia(&value, "rust").unwrap();
        assert!(output.contains("Rust (programming language)"));
        assert!(output.contains("systems programming"));
    }

    #[test]
    fn bing_formats_rss_items() {
        let xml = r#"<rss><channel><item><title>Rust</title><link>https://rust-lang.org/</link><description>A &lt;b&gt;systems&lt;/b&gt; language.</description></item></channel></rss>"#;
        let output = format_bing_rss(xml, "rust", 5).unwrap();
        assert!(output.contains("Rust (https://rust-lang.org/)"));
        assert!(output.contains("A systems language."));
    }

    #[tokio::test]
    async fn tavily_without_key_errors() {
        assert!(tavily_search("q", "", 5).await.is_err());
    }
}
