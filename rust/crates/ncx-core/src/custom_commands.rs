//! Prompt-backed custom slash commands shared by CLI and GUI.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommandSummary {
    pub scope: &'static str,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommandQuery {
    pub scope: Option<&'static str>,
    pub name: String,
}

pub fn custom_command_prompt(
    workspace: &Path,
    slash_cmd: &str,
    arg: &str,
) -> Result<Option<String>, String> {
    let Some(query) = parse_custom_command_query(slash_cmd) else {
        return Ok(None);
    };
    let Some(cmd) = resolve_custom_command(workspace, &query) else {
        return Ok(None);
    };
    let template = std::fs::read_to_string(&cmd.path).map_err(|e| {
        format!(
            "could not read custom command {} from {}: {e}",
            slash_cmd,
            cmd.path.display()
        )
    })?;
    Ok(Some(expand_custom_command_template(
        strip_frontmatter(&template),
        arg,
    )))
}

pub fn parse_custom_command_query(slash_cmd: &str) -> Option<CustomCommandQuery> {
    let body = slash_cmd.strip_prefix('/')?;
    if body.is_empty() {
        return None;
    }
    let (scope, name) = if let Some((scope, name)) = body.split_once(':') {
        let scope = match scope {
            "project" => "project",
            "user" => "user",
            _ => return None,
        };
        (Some(scope), name)
    } else {
        (None, body)
    };
    if !valid_custom_command_name(name) {
        return None;
    }
    Some(CustomCommandQuery {
        scope,
        name: name.to_string(),
    })
}

pub fn resolve_custom_command(
    workspace: &Path,
    query: &CustomCommandQuery,
) -> Option<CustomCommandSummary> {
    custom_command_roots(workspace)
        .into_iter()
        .filter(|root| query.scope.is_none_or(|s| s == root.scope))
        .find_map(|root| {
            let path = root.dir.join(format!("{}.md", query.name));
            if path.is_file() {
                Some(CustomCommandSummary {
                    scope: root.scope,
                    name: query.name.clone(),
                    path,
                })
            } else {
                None
            }
        })
}

pub fn list_custom_commands(workspace: &Path) -> Vec<CustomCommandSummary> {
    let mut out: Vec<CustomCommandSummary> = Vec::new();
    let mut seen: Vec<(&'static str, String)> = Vec::new();
    for root in custom_command_roots(workspace) {
        let Ok(entries) = std::fs::read_dir(&root.dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !valid_custom_command_name(name) {
                continue;
            }
            let name = name.to_string();
            if seen
                .iter()
                .any(|(scope, n)| *scope == root.scope && n == &name)
            {
                continue;
            }
            seen.push((root.scope, name.clone()));
            out.push(CustomCommandSummary {
                scope: root.scope,
                name,
                path,
            });
        }
    }
    out.sort_by(|a, b| (a.scope, &a.name).cmp(&(b.scope, &b.name)));
    out
}

struct CustomCommandRoot {
    scope: &'static str,
    dir: PathBuf,
}

fn custom_command_roots(workspace: &Path) -> Vec<CustomCommandRoot> {
    let mut roots = vec![
        CustomCommandRoot {
            scope: "project",
            dir: workspace.join(".nanocodex").join("commands"),
        },
        CustomCommandRoot {
            scope: "project",
            dir: workspace.join(".claude").join("commands"),
        },
    ];
    if let Some(home) = home_dir() {
        roots.push(CustomCommandRoot {
            scope: "user",
            dir: home.join(".nanocodex").join("commands"),
        });
        roots.push(CustomCommandRoot {
            scope: "user",
            dir: home.join(".claude").join("commands"),
        });
    }
    roots
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn valid_custom_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn strip_frontmatter(template: &str) -> &str {
    let Some(rest) = template
        .strip_prefix("---\n")
        .or_else(|| template.strip_prefix("---\r\n"))
    else {
        return template.trim();
    };
    let mut offset = template.len() - rest.len();
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return template[offset + line.len()..].trim();
        }
        offset += line.len();
    }
    template.trim()
}

pub fn expand_custom_command_template(template: &str, arg: &str) -> String {
    let args = split_custom_args(arg);
    let mut out = template.to_string();
    for i in 0..10 {
        let value = args.get(i).map(String::as_str).unwrap_or("");
        out = out.replace(&format!("$ARGUMENTS[{i}]"), value);
        out = out.replace(&format!("${i}"), value);
    }
    out = out.replace("$ARGUMENTS", arg.trim());
    if !arg.trim().is_empty()
        && !template.contains("$ARGUMENTS")
        && !(0..10).any(|i| template.contains(&format!("${i}")))
    {
        out.push_str("\n\nArguments: ");
        out.push_str(arg.trim());
    }
    out.trim().to_string()
}

fn split_custom_args(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in arg.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_session_id;

    #[test]
    fn parses_scoped_custom_command_queries() {
        assert!(parse_custom_command_query("/project:review").is_some());
        assert!(parse_custom_command_query("/user:review").is_some());
        assert!(parse_custom_command_query("/team:review").is_none());
        assert!(parse_custom_command_query("/bad/name").is_none());
        assert!(parse_custom_command_query("/bad name").is_none());
    }

    #[test]
    fn expands_custom_command_arguments() {
        let prompt = expand_custom_command_template(
            "Review $ARGUMENTS[0] against $1. Full: $ARGUMENTS",
            "\"src/main.rs\" tests --fast",
        );
        assert_eq!(
            prompt,
            "Review src/main.rs against tests. Full: \"src/main.rs\" tests --fast"
        );

        let appended = expand_custom_command_template("Ship it.", "release notes");
        assert_eq!(appended, "Ship it.\n\nArguments: release notes");
    }

    #[test]
    fn custom_command_prompt_strips_frontmatter_and_prefers_project() {
        let ws = std::env::temp_dir().join(format!("ncx_custom_core_{}", new_session_id()));
        let dir = ws.join(".nanocodex").join("commands");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ship.md"),
            "---\ndescription: Ship\n---\nShip $ARGUMENTS",
        )
        .unwrap();

        let prompt = custom_command_prompt(&ws, "/ship", "v1").unwrap().unwrap();
        assert_eq!(prompt, "Ship v1");
        assert_eq!(list_custom_commands(&ws).len(), 1);

        let _ = std::fs::remove_dir_all(ws);
    }
}
