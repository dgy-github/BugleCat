//! Code search tools — `grep` (regex over file contents) and `glob` (filename
//! patterns). These let the model locate code WITHOUT shelling out, which is
//! both faster and works under `read-only` sandbox without an approval prompt.
//!
//! Dependency-light: a small recursive walker (skips VCS/build/dep dirs) plus
//! the `regex` crate. `glob` patterns are translated to a regex over the
//! forward-slash-normalized path, so `**/*.rs`, `src/*.toml`, `?` all work
//! cross-platform.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext};

/// Directories never walked (noise / huge / generated).
const IGNORE_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".ncx",
    "dist",
    ".venv",
    "__pycache__",
];
/// Safety caps so a search can't run away on a giant tree.
const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: usize = 2_000_000;
const DEFAULT_MAX_RESULTS: usize = 200;

/// Recursively collect files under `root`, skipping [`IGNORE_DIRS`]. Bounded by
/// [`MAX_FILES`]. Returns absolute paths.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                let name = entry.file_name();
                if IGNORE_DIRS.contains(&name.to_string_lossy().as_ref()) {
                    continue;
                }
                stack.push(p);
            } else if ft.is_file() {
                out.push(p);
                if out.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    out
}

/// Forward-slash relative path of `p` under `root` (for display + glob match).
fn rel_slash(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Translate a glob pattern into an anchored regex over a `/`-separated path.
/// `**` → any (incl. `/`); `*` → any non-`/`; `?` → one non-`/`; others literal.
pub fn glob_to_regex(pattern: &str) -> Regex {
    let mut re = String::from("^");
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '*' => {
                if i + 1 < bytes.len() && bytes[i + 1] == '*' {
                    re.push_str(".*");
                    i += 2;
                    // consume a following '/' so `**/x` matches `x` at root too
                    if i < bytes.len() && bytes[i] == '/' {
                        i += 1;
                    }
                    continue;
                }
                re.push_str("[^/]*");
            }
            '?' => re.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            other => re.push(other),
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).unwrap_or_else(|_| Regex::new("$^").unwrap())
}

/// grep: regex over file contents → `rel/path:line: text` lines (capped).
pub fn grep(
    root: &Path,
    pattern: &str,
    path_glob: Option<&str>,
    max_results: usize,
) -> Result<String, String> {
    let re = Regex::new(pattern).map_err(|e| format!("invalid regex: {e}"))?;
    let path_re = path_glob.map(glob_to_regex);
    let mut hits: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for f in walk_files(root) {
        if hits.len() >= max_results {
            break;
        }
        let rel = rel_slash(root, &f);
        if let Some(pr) = &path_re {
            if !pr.is_match(&rel) {
                continue;
            }
        }
        let Ok(meta) = std::fs::metadata(&f) else {
            continue;
        };
        if meta.len() as usize > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        }; // skip binary
        scanned += 1;
        for (n, line) in text.lines().enumerate() {
            if re.is_match(line) {
                let shown = if line.len() > 300 { &line[..300] } else { line };
                hits.push(format!("{rel}:{}: {}", n + 1, shown.trim_end()));
                if hits.len() >= max_results {
                    break;
                }
            }
        }
    }

    if hits.is_empty() {
        return Ok(format!(
            "No matches for /{pattern}/ (scanned {scanned} files)."
        ));
    }
    let capped = hits.len() >= max_results;
    let mut out = hits.join("\n");
    if capped {
        out.push_str(&format!("\n... (capped at {max_results} matches)"));
    }
    Ok(out)
}

/// glob: filenames matching a pattern → newline list of `rel/path` (capped).
pub fn glob(root: &Path, pattern: &str, max_results: usize) -> String {
    let re = glob_to_regex(pattern);
    let mut hits: Vec<String> = Vec::new();
    for f in walk_files(root) {
        let rel = rel_slash(root, &f);
        if re.is_match(&rel) {
            hits.push(rel);
            if hits.len() >= max_results {
                break;
            }
        }
    }
    if hits.is_empty() {
        format!("No files match {pattern}.")
    } else {
        hits.sort();
        hits.join("\n")
    }
}

// ── tools ─────────────────────────────────────────────────────────────────────

/// `grep` — regex search over workspace file contents.
pub struct GrepTool;

#[async_trait(?Send)]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn description(&self) -> &str {
        "Search file CONTENTS by regular expression across the workspace and \
         return 'path:line: text' matches. Faster than shelling out and works \
         read-only. Optional 'path_glob' filters which files are searched \
         (e.g. '**/*.rs'). Skips .git/target/node_modules/etc."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Rust regex to match against each line."},
                "path_glob": {"type": "string", "description": "Optional glob to limit files, e.g. '**/*.rs'."},
                "max_results": {"type": "integer", "minimum": 1, "description": "Max matches (default 200)."},
            },
            "required": ["pattern"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return "Error: 'pattern' is required and must be a string.".into();
        };
        let path_glob = args.get("path_glob").and_then(|v| v.as_str());
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize;
        match grep(&ctx.workspace, pattern, path_glob, max) {
            Ok(s) => s,
            Err(e) => format!("Error: {e}"),
        }
    }
}

/// `glob` — find files by name pattern.
pub struct GlobTool;

#[async_trait(?Send)]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }
    fn description(&self) -> &str {
        "Find files whose path matches a glob pattern (e.g. '**/*.rs', \
         'src/**/mod.rs', '*.toml') and return the matching paths. Use to locate \
         files by name. Skips .git/target/node_modules/etc."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob, e.g. '**/*.rs' or 'src/*.toml'."},
                "max_results": {"type": "integer", "minimum": 1, "description": "Max paths (default 200)."},
            },
            "required": ["pattern"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return "Error: 'pattern' is required and must be a string.".into();
        };
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize;
        glob(&ctx.workspace, pattern, max)
    }
}

/// `web_search` — keyless DuckDuckGo Instant Answer lookup (facts/definitions).
pub struct WebSearchTool;

#[async_trait(?Send)]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Look something up on the web via DuckDuckGo's Instant Answer API and \
         return a short summary with links. Best for factual / definitional \
         queries ('what is X', 'who is Y'); it is NOT a general web crawler. \
         Requires network access."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The search query (a factual question works best)."},
            },
            "required": ["query"],
        })
    }
    fn read_only(&self) -> bool {
        true // no local side effects; safe to run concurrently with other reads
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' is required and must be a string.".into();
        };
        if ctx.policy.mode == ncx_sandbox::READ_ONLY {
            return "Error: web access is disabled in plan / read-only mode. Switch to default, \
                    accept-edits, or bypass to use the network."
                .into();
        }
        // Keyed Tavily when configured; otherwise (or on failure) DuckDuckGo.
        if ctx.search_provider.eq_ignore_ascii_case("tavily") && !ctx.search_api_key.is_empty() {
            match ncx_provider::tavily_search(query, &ctx.search_api_key, 6).await {
                Ok(s) => return s,
                Err(e) => {
                    // Fall back to the keyless backend rather than hard-failing.
                    return match ncx_provider::ddg_instant_answer(query).await {
                        Ok(s) => format!("(tavily failed: {e}; fell back to DuckDuckGo)\n{s}"),
                        Err(e2) => format!("Error: web_search failed: tavily={e}, ddg={e2}"),
                    };
                }
            }
        }
        match ncx_provider::ddg_instant_answer(query).await {
            Ok(s) => s,
            Err(e) => format!("Error: web_search failed: {e}"),
        }
    }
}


/// `web_fetch` — fetch a URL and return its readable text (HTML stripped).
pub struct WebFetchTool;

#[async_trait(?Send)]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch a web page (or text resource) by URL and return its readable text \
         with HTML/scripts stripped. Use after web_search to actually READ a page. \
         Requires network access; output is size-capped."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Absolute http(s) URL to fetch."},
            },
            "required": ["url"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
            return "Error: 'url' is required and must be a string.".into();
        };
        if ctx.policy.mode == ncx_sandbox::READ_ONLY {
            return "Error: web access is disabled in plan / read-only mode. Switch to default, \
                    accept-edits, or bypass to use the network."
                .into();
        }
        match ncx_provider::fetch_url(url).await {
            Ok(s) => s,
            Err(e) => format!("Error: web_fetch failed: {e}"),
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncx_search_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap(); // ignored
        std::fs::write(
            d.join("src/main.rs"),
            "fn main() {\n    let x = 42;\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        std::fs::write(d.join("src/util.rs"), "pub fn helper() -> i32 { 42 }\n").unwrap();
        std::fs::write(d.join("README.md"), "# Title\nsome TODO here\n").unwrap();
        std::fs::write(
            d.join("target/junk.rs"),
            "fn should_be_ignored() { let x = 42; }\n",
        )
        .unwrap();
        d
    }

    #[test]
    fn glob_to_regex_matches_expected() {
        assert!(glob_to_regex("**/*.rs").is_match("src/main.rs"));
        assert!(glob_to_regex("**/*.rs").is_match("a/b/c.rs"));
        assert!(glob_to_regex("*.toml").is_match("Cargo.toml"));
        assert!(!glob_to_regex("*.toml").is_match("src/Cargo.toml")); // * doesn't cross /
        assert!(glob_to_regex("src/*.rs").is_match("src/main.rs"));
        assert!(!glob_to_regex("src/*.rs").is_match("src/sub/main.rs"));
    }

    #[test]
    fn grep_finds_matches_and_skips_ignored() {
        let d = fixture("grep");
        let out = grep(&d, r"\b42\b", None, 200).unwrap();
        assert!(out.contains("src/main.rs:2"), "{out}");
        assert!(out.contains("src/util.rs:1"), "{out}");
        // target/ is ignored -> the junk file must not appear
        assert!(!out.contains("junk.rs"), "{out}");
    }

    #[test]
    fn grep_path_glob_filters() {
        let d = fixture("grepglob");
        let out = grep(&d, "TODO", Some("**/*.md"), 200).unwrap();
        assert!(out.contains("README.md"), "{out}");
        let none = grep(&d, "TODO", Some("**/*.rs"), 200).unwrap();
        assert!(none.contains("No matches"), "{none}");
    }

    #[test]
    fn grep_no_match_reports_count() {
        let d = fixture("nomatch");
        let out = grep(&d, "zzzznotfound", None, 200).unwrap();
        assert!(out.contains("No matches"));
    }

    #[test]
    fn grep_invalid_regex_errors() {
        let d = fixture("badre");
        assert!(grep(&d, "(unclosed", None, 200).is_err());
    }

    #[test]
    fn glob_lists_rs_files_skipping_ignored() {
        let d = fixture("glob");
        let out = glob(&d, "**/*.rs", 200);
        assert!(out.contains("src/main.rs"));
        assert!(out.contains("src/util.rs"));
        assert!(!out.contains("junk.rs")); // target/ ignored
    }

    #[tokio::test]
    async fn web_tools_blocked_in_read_only() {
        use crate::tools::ToolContext;
        use ncx_sandbox::{SandboxPolicy, READ_ONLY};
        let ws = std::env::temp_dir();
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(READ_ONLY, &ws));
        let s = WebSearchTool.execute(&ctx, &json!({ "query": "x" })).await;
        assert!(s.contains("disabled") && s.contains("read-only"), "{s}");
        let f = WebFetchTool
            .execute(&ctx, &json!({ "url": "http://example.com" }))
            .await;
        assert!(f.contains("disabled") && f.contains("read-only"), "{f}");
    }
}
