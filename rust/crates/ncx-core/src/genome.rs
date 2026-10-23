//! Training-time harness overrides — the "genome" the ncx-forge trainer evolves.
//!
//! At startup the agent reads `NCX_GENOME=<path.toml>`; when set, the named
//! fields override the agent's hardcoded scaffold (the base system prompt,
//! per-tool descriptions, orchestrator node system prompts, and memory recall
//! caps). When unset/empty/unreadable/malformed, [`Genome`] is empty and the
//! agent behaves byte-for-byte as if this module did not exist — that no-op
//! guarantee is what makes a training run trustworthy (a candidate genome that
//! fails to load must not silently change behavior).
//!
//! TOML format:
//! ```toml
//! system_prompt = """
//! You are nanocodex, ...
//! """
//!
//! [tool_desc]
//! apply_patch = """Create, update, ..."""
//! read_file   = "Read a UTF-8 text file ..."
//!
//! [orchestrator_prompts]
//! classify_sys = "..."
//! plan_sys = "..."
//!
//! [memory_config]
//! max_entries = 6
//! max_chars = 2400
//! ```
//!
//! Only DESCRIPTIONS, prompts, and bounded recall caps are evolvable — never
//! tool *behavior* or sandbox/approval policy. That keeps the genome a pure
//! text-and-cap surface with no new capability for a teacher to inject.

use std::collections::HashMap;
use std::path::Path;

/// Harness text and recall-cap overrides loaded from `NCX_GENOME`. Empty = no overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Genome {
    /// Overrides the base system prompt (before project-instruction/memory/skill
    /// blocks are appended). `None` = keep the hardcoded default.
    pub system_prompt: Option<String>,
    /// Per-tool description overrides, keyed by tool name. Tools absent from the
    /// map keep their default description.
    pub tool_desc: HashMap<String, String>,
    /// Optional overrides for orchestrator node system prompts (`classify_sys`,
    /// `plan_sys`, `decompose_sys`, `worker_sys`, `verify_sys`).
    pub orchestrator_prompts: HashMap<String, String>,
    /// Optional override for the maximum number of recalled memory entries (`1..=32`).
    pub memory_max_entries: Option<usize>,
    /// Optional override for the maximum recalled memory block size in chars (`200..=16000`).
    pub memory_max_chars: Option<usize>,
}

impl Genome {
    /// Load from the `NCX_GENOME` env var path. Returns an empty (no-op) genome
    /// when the var is unset/empty, or the file is missing/unreadable/malformed
    /// — failure to load must never change agent behavior.
    pub fn from_env() -> Self {
        match std::env::var_os("NCX_GENOME") {
            Some(p) if !p.is_empty() => Genome::load(Path::new(&p)).unwrap_or_default(),
            _ => Genome::default(),
        }
    }

    /// Read and parse a genome TOML file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        Genome::parse(&text)
    }

    /// Parse a genome from TOML text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let val: toml::Value = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut g = Genome::default();
        if let Some(sp) = val.get("system_prompt").and_then(|v| v.as_str()) {
            let sp = sp.trim();
            if !sp.is_empty() {
                g.system_prompt = Some(sp.to_string());
            }
        }
        if let Some(table) = val.get("tool_desc").and_then(|v| v.as_table()) {
            for (k, v) in table {
                if let Some(s) = v.as_str() {
                    let s = s.trim();
                    // Reject blank overrides: an empty description (e.g. of the
                    // load-bearing apply_patch) would silently degrade the agent.
                    if !s.is_empty() {
                        g.tool_desc.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        if let Some(table) = val.get("orchestrator_prompts").and_then(|v| v.as_table()) {
            for (k, v) in table {
                if let Some(s) = v.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        g.orchestrator_prompts.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
        if let Some(table) = val.get("memory_config").and_then(|v| v.as_table()) {
            if let Some(n) = table.get("max_entries").and_then(|v| v.as_integer()) {
                if n > 0 {
                    g.memory_max_entries = Some((n as usize).clamp(1, 32));
                }
            }
            if let Some(c) = table.get("max_chars").and_then(|v| v.as_integer()) {
                if c > 0 {
                    g.memory_max_chars = Some((c as usize).clamp(200, 16_000));
                }
            }
        }
        Ok(g)
    }

    /// True when no field overrides anything (the no-op default).
    pub fn is_empty(&self) -> bool {
        self.system_prompt.is_none()
            && self.tool_desc.is_empty()
            && self.orchestrator_prompts.is_empty()
            && self.memory_max_entries.is_none()
            && self.memory_max_chars.is_none()
    }

    /// The effective base system prompt: the override if present, else `default`.
    pub fn base_system_prompt<'a>(&'a self, default: &'a str) -> &'a str {
        self.system_prompt.as_deref().unwrap_or(default)
    }

    /// The effective description for tool `name`: the override if present, else
    /// `default`. Borrow-safe: returned ref lives as long as both inputs.
    pub fn describe<'a>(&'a self, name: &str, default: &'a str) -> &'a str {
        self.tool_desc
            .get(name)
            .map(String::as_str)
            .unwrap_or(default)
    }

    /// The effective orchestrator prompt for node `key`: the override if present,
    /// else `default`.
    pub fn orchestrator_prompt<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.orchestrator_prompts
            .get(key)
            .map(String::as_str)
            .unwrap_or(default)
    }

    /// Effective memory recall entry cap (`default` when unset).
    pub fn effective_memory_max_entries(&self, default: usize) -> usize {
        self.memory_max_entries.unwrap_or(default)
    }

    /// Effective memory recall char cap (`default` when unset).
    pub fn effective_memory_max_chars(&self, default: usize) -> usize {
        self.memory_max_chars.unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_genome_is_a_noop() {
        let g = Genome::default();
        assert!(g.is_empty());
        assert_eq!(g.base_system_prompt("DEFAULT"), "DEFAULT");
        assert_eq!(g.describe("apply_patch", "DEFAULT_DESC"), "DEFAULT_DESC");
        assert_eq!(
            g.orchestrator_prompt("plan_sys", "DEFAULT_PLAN"),
            "DEFAULT_PLAN"
        );
        assert_eq!(g.effective_memory_max_entries(8), 8);
        assert_eq!(g.effective_memory_max_chars(2400), 2400);
    }

    #[test]
    fn parses_system_prompt_and_tool_desc() {
        let toml = r#"
system_prompt = "You are a focused agent."

[tool_desc]
apply_patch = "Edit files via V4A patches."
read_file = "Read a file."
"#;
        let g = Genome::parse(toml).unwrap();
        assert!(!g.is_empty());
        assert_eq!(g.base_system_prompt("DEFAULT"), "You are a focused agent.");
        assert_eq!(
            g.describe("apply_patch", "DEFAULT"),
            "Edit files via V4A patches."
        );
        assert_eq!(g.describe("read_file", "DEFAULT"), "Read a file.");
        // A tool not in the map keeps its default.
        assert_eq!(g.describe("shell", "SHELL_DEFAULT"), "SHELL_DEFAULT");
    }

    #[test]
    fn parses_orchestrator_prompts_and_memory_config() {
        let toml = r#"
[orchestrator_prompts]
plan_sys = "Produce a 3-step plan."
verify_sys = "Start with PASS or FAIL."

[memory_config]
max_entries = 5
max_chars = 1800
"#;
        let g = Genome::parse(toml).unwrap();
        assert!(!g.is_empty());
        assert_eq!(
            g.orchestrator_prompt("plan_sys", "DEFAULT"),
            "Produce a 3-step plan."
        );
        assert_eq!(
            g.orchestrator_prompt("verify_sys", "DEFAULT"),
            "Start with PASS or FAIL."
        );
        assert_eq!(
            g.orchestrator_prompt("classify_sys", "KEEP_DEFAULT"),
            "KEEP_DEFAULT"
        );
        assert_eq!(g.effective_memory_max_entries(8), 5);
        assert_eq!(g.effective_memory_max_chars(4000), 1800);
    }

    #[test]
    fn blank_overrides_are_rejected() {
        // Empty/whitespace values must NOT override (would degrade the agent).
        let toml = "system_prompt = \"   \"\n[tool_desc]\napply_patch = \"\"\n[orchestrator_prompts]\nplan_sys = \"  \"\n";
        let g = Genome::parse(toml).unwrap();
        assert!(g.is_empty(), "blank overrides should be dropped");
        assert_eq!(g.describe("apply_patch", "KEEP"), "KEEP");
        assert_eq!(g.orchestrator_prompt("plan_sys", "KEEP_PLAN"), "KEEP_PLAN");
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(Genome::parse("system_prompt = = =").is_err());
    }

    #[test]
    fn multiline_triple_quoted_prompt() {
        let toml = "system_prompt = \"\"\"\nline one\nline two\n\"\"\"\n";
        let g = Genome::parse(toml).unwrap();
        assert_eq!(g.base_system_prompt("D"), "line one\nline two");
    }
}
