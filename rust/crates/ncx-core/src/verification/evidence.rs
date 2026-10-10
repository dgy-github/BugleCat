use super::{snapshot::snapshot_excluding, types::*};
use crate::session::Session;
use regex::Regex;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub type CheckSnapshots = BTreeMap<String, (FileSnapshot, FileSnapshot)>;

pub fn workspace_snapshot(session: &Session, root: &Path) -> FileSnapshot {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let mut excluded = vec![];
    if let Some(log) = &session.log_path {
        let log = log.canonicalize().unwrap_or_else(|_| {
            if log.is_absolute() {
                log.clone()
            } else {
                root.join(log)
            }
        });
        excluded.extend([
            log.clone(),
            log.with_extension("sources.jsonl"),
            log.with_extension("verification.jsonl"),
        ]);
        if let Some(parent) = log.parent() {
            excluded.extend(
                ["events.sqlite3", "events.sqlite3-wal", "events.sqlite3-shm"]
                    .map(|name| parent.join(name)),
            );
        }
    }
    snapshot_excluding(&root, &excluded)
}

pub fn redact(text: &str) -> String {
    let mut value = text.to_owned();
    if let Ok(key) = std::env::var("NCX_JEV_API_KEY") {
        if !key.is_empty() {
            value = value.replace(&key, "[redacted]");
        }
    }
    for (pattern, replacement) in [
        (
            r"(?i)(apikey_[a-z0-9_]+|sk-[a-z0-9_-]+|bearer\s+\S+)",
            "[redacted]",
        ),
        (
            r#"(?i)[\"']?(?:api[_-]?key|password|secret|access_token|refresh_token|token)[\"']?\s*[=:]\s*(?:\"[^\"]*\"|'[^']*'|[^\s,}]+)"#,
            "[redacted field]",
        ),
        (
            r"(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}",
            "[redacted email]",
        ),
        (r"(?i)[a-z]:\\users\\[^\\\s]+", "[user-home]"),
    ] {
        value = Regex::new(pattern)
            .unwrap()
            .replace_all(&value, replacement)
            .into_owned();
    }
    value
}

pub fn unsafe_to_send(text: &str) -> bool {
    [
        "-----BEGIN",
        "PRIVATE KEY-----",
        "data:image/",
        "data:application/",
        "Cookie:",
        "Set-Cookie:",
    ]
    .iter()
    .any(|s| text.contains(s))
}

pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                matches!(p["type"].as_str(), Some("text" | "input_text"))
                    .then(|| p["text"].as_str())
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub use super::extraction::claims;

/// Only actual shell argv text supplies scope. A command is the certified
/// scope, never "whole project" inferred from a passing excerpt.
pub fn test_scope(command: &str) -> Vec<String> {
    let lower = command.trim().to_lowercase();
    if lower.split_whitespace().any(|token| {
        matches!(
            token.split('=').next().unwrap_or_default(),
            "--help"
                | "-h"
                | "--version"
                | "--list"
                | "--listtests"
                | "--list-tests"
                | "--collect-only"
                | "--collectonly"
                | "--co"
                | "--dry-run"
                | "--no-run"
                | "--showconfig"
                | "--show-config"
                | "--fixtures"
                | "--fixtures-per-test"
        )
    }) {
        return vec![];
    }
    let direct =
        Regex::new(r"(?i)^python(?:3)? (?:[^\s;&|]+[/\\])?(?:check|test[^\s]*)\.py(?:\s|$)")
            .unwrap()
            .is_match(command.trim())
            || [
                "pytest",
                "python -m pytest",
                "python3 -m pytest",
                "cargo test",
                "npm test",
                "npm run test",
                "python -m unittest",
            ]
            .iter()
            .any(|s| {
                lower == *s
                    || lower
                        .strip_prefix(s)
                        .is_some_and(|tail| tail.starts_with(' '))
            });
    if direct
        && !command
            .chars()
            .any(|c| [';', '&', '|', '<', '>', '$', '`', '\n', '\r'].contains(&c))
    {
        vec![command.to_owned()]
    } else {
        vec![]
    }
}

pub fn packet(
    session: &Session,
    workspace: &Path,
    candidate: &str,
    checks: &CheckSnapshots,
) -> VerificationInput {
    super::assembly::packet(session, workspace, candidate, checks)
}
