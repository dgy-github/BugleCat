//! TOML writer for `~/.nanocodex/config.toml` — Rust port of the writer
//! functions at the bottom of `nanocodex/config.py`.
//!
//! `tomllib` (stdlib) only *reads* TOML; nanocodex avoids heavy deps so we ship
//! a tiny purpose-built writer for the flat scalar shape this file uses.

use std::collections::HashMap;
use std::path::Path;

// Keys the GUI Settings dialog may write, in on-disk order.
pub const WRITABLE_KEYS: &[&str] = &[
    "api_key",
    "base_url",
    "model",
    "sandbox_mode",
    "approval_policy",
    "permission_mode",
    "reasoning_effort",
    "vl_base_url",
    "vl_api_key",
    "vl_model",
    "ark_api_key",
    "max_iterations",
    "max_tool_calls",
    "context_edit_enabled",
    "context_edit_max_chars",
    "context_edit_keep_recent_messages",
    "context_edit_max_tool_result_chars",
    "price_in",
    "price_out",
];

/// Escape a string for a TOML basic (double-quoted) string value.
fn esc_toml(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Serialize known settings to `~/.nanocodex/config.toml` text (pure function).
///
/// Only keys in [`WRITABLE_KEYS`] are emitted, in a fixed order, so the output
/// round-trips through a standard TOML parser.  Empty / `None` values are
/// skipped; unknown keys are ignored.
pub fn dump_nanocodex_toml(values: &HashMap<&str, &str>) -> String {
    let header = concat!(
        "# nanocodex settings. Managed by the GUI Settings dialog, but also\n",
        "# safe to edit by hand. These values win over ~/.deepseek/config.toml\n",
        "# and ~/.codex/config.toml, but environment variables and CLI flags\n",
        "# still override them. The API key is never logged or printed.\n",
    );
    let mut lines: Vec<String> = Vec::new();
    for key in WRITABLE_KEYS {
        if let Some(val) = values.get(key) {
            if !val.is_empty() {
                lines.push(format!("{key} = \"{}\"", esc_toml(val)));
            }
        }
    }
    if lines.is_empty() {
        format!("{header}\n")
    } else {
        format!("{header}\n{}\n", lines.join("\n"))
    }
}

/// Merge *updates* into the file at *path* and write it back.
///
/// Existing values are preserved (merge, not replace), so setting just the API
/// key won't wipe a previously saved `base_url`.  Only [`WRITABLE_KEYS`] are
/// persisted; others are silently ignored.  Returns the path written.
pub fn write_nanocodex_config(
    updates: &HashMap<&str, &str>,
    path: &Path,
) -> Result<(), std::io::Error> {
    // Read current content (empty if file absent).
    let current_text = std::fs::read_to_string(path).unwrap_or_default();
    let current: toml::Table = match current_text.parse::<toml::Value>() {
        Ok(toml::Value::Table(t)) => t,
        _ => toml::Table::new(),
    };

    // Merge: start from existing writable keys, apply updates on top.
    let mut merged: HashMap<&str, String> = HashMap::new();
    for key in WRITABLE_KEYS {
        if let Some(v) = current
            .get(*key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            merged.insert(key, v.to_string());
        }
    }
    for (key, val) in updates {
        if WRITABLE_KEYS.contains(key) && !val.is_empty() {
            merged.insert(key, val.to_string());
        }
    }

    // Serialize via dump_nanocodex_toml (borrows &str).
    let str_map: HashMap<&str, &str> = merged.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let text = dump_nanocodex_toml(&str_map);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn map<'a>(pairs: &[(&'a str, &'a str)]) -> HashMap<&'a str, &'a str> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn dump_round_trips_quoted_value() {
        let text = dump_nanocodex_toml(&map(&[
            ("api_key", r#"sk-with"quote"#),
            ("base_url", "https://api.deepseek.com/beta"),
            ("model", "deepseek-v4-pro"),
        ]));
        let parsed = text.parse::<toml::Value>().unwrap();
        assert_eq!(parsed["api_key"].as_str().unwrap(), r#"sk-with"quote"#);
        assert_eq!(
            parsed["base_url"].as_str().unwrap(),
            "https://api.deepseek.com/beta"
        );
        assert_eq!(parsed["model"].as_str().unwrap(), "deepseek-v4-pro");
    }

    #[test]
    fn dump_skips_empty_and_unknown() {
        let text = dump_nanocodex_toml(&map(&[("api_key", ""), ("model", "m"), ("bogus", "x")]));
        let parsed = text.parse::<toml::Value>().unwrap();
        assert!(
            !parsed.as_table().unwrap().contains_key("api_key"),
            "empty should be skipped"
        );
        let t = parsed.as_table().unwrap();
        assert!(!t.contains_key("bogus"), "unknown key should be skipped");
        assert_eq!(t["model"].as_str().unwrap(), "m");
    }

    #[test]
    fn write_creates_and_merges() {
        let tmp = std::env::temp_dir().join("ncx_writer_test");
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("config.toml");
        let _ = std::fs::remove_file(&target);

        write_nanocodex_config(&map(&[("api_key", "sk-1")]), &target).unwrap();
        assert!(target.is_file());

        write_nanocodex_config(&map(&[("model", "deepseek-chat")]), &target).unwrap();

        let parsed = std::fs::read_to_string(&target)
            .unwrap()
            .parse::<toml::Value>()
            .unwrap();
        assert_eq!(parsed["api_key"].as_str().unwrap(), "sk-1");
        assert_eq!(parsed["model"].as_str().unwrap(), "deepseek-chat");
    }

    #[test]
    fn write_ignores_unknown_keys() {
        let tmp = std::env::temp_dir().join("ncx_writer_test_unknown");
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("config.toml");
        let _ = std::fs::remove_file(&target);

        write_nanocodex_config(&map(&[("api_key", "sk-1"), ("bogus", "nope")]), &target).unwrap();
        let parsed = std::fs::read_to_string(&target)
            .unwrap()
            .parse::<toml::Value>()
            .unwrap();
        assert!(!parsed.as_table().unwrap().contains_key("bogus"));
    }

    #[test]
    fn write_persists_runtime_control_keys() {
        let text = dump_nanocodex_toml(&map(&[
            ("max_iterations", "12"),
            ("max_tool_calls", "34"),
            ("context_edit_enabled", "false"),
            ("context_edit_max_chars", "9000"),
            ("context_edit_keep_recent_messages", "8"),
            ("context_edit_max_tool_result_chars", "600"),
        ]));
        let parsed = text.parse::<toml::Value>().unwrap();
        assert_eq!(parsed["max_iterations"].as_str().unwrap(), "12");
        assert_eq!(parsed["max_tool_calls"].as_str().unwrap(), "34");
        assert_eq!(parsed["context_edit_enabled"].as_str().unwrap(), "false");
        assert_eq!(parsed["context_edit_max_chars"].as_str().unwrap(), "9000");
        assert_eq!(
            parsed["context_edit_keep_recent_messages"]
                .as_str()
                .unwrap(),
            "8"
        );
        assert_eq!(
            parsed["context_edit_max_tool_result_chars"]
                .as_str()
                .unwrap(),
            "600"
        );
    }
}
