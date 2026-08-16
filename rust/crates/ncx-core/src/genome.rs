//! Training-time harness overrides — the "genome" the ncx-forge trainer evolves.
//!
//! At startup the agent reads `NCX_GENOME=<path.toml>`; when set, the named
//! fields override the agent's hardcoded scaffold (the base system prompt and
//! per-tool descriptions). When unset/empty/unreadable/malformed, [`Genome`] is
//! empty and the agent behaves byte-for-byte as if this module did not exist —
//! that no-op guarantee is what makes a training run trustworthy (a candidate
//! genome that fails to load must not silently change behavior).
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
//! ```
//!
//! Only DESCRIPTIONS and the base prompt are evolvable — never tool *behavior*
//! (the sandbox still governs execution). That keeps the genome a pure
//! text-substitution surface with no new capability for a teacher to inject.

use std::collections::HashMap;
use std::path::Path;

/// Harness text overrides loaded from `NCX_GENOME`. Empty = no overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Genome {
    /// Overrides the base system prompt (before project-instruction/memory/skill
    /// blocks are appended). `None` = keep the hardcoded default.
    pub system_prompt: Option<String>,
    /// Per-tool description overrides, keyed by tool name. Tools absent from the
    /// map keep their default description.
    pub tool_desc: HashMap<String, String>,
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
        Ok(g)
    }

    /// True when no field overrides anything (the no-op default).
    pub fn is_empty(&self) -> bool {
        self.system_prompt.is_none() && self.tool_desc.is_empty()
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
    fn blank_overrides_are_rejected() {
        // Empty/whitespace values must NOT override (would degrade the agent).
        let toml = "system_prompt = \"   \"\n[tool_desc]\napply_patch = \"\"\n";
        let g = Genome::parse(toml).unwrap();
        assert!(g.is_empty(), "blank overrides should be dropped");
        assert_eq!(g.describe("apply_patch", "KEEP"), "KEEP");
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
