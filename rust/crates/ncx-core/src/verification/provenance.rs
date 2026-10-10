//! Invocation identity comes from host events, never from candidate prose.
use crate::context_trace::content_hash;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn safe_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if let Ok(meta) = std::fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return None;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return None;
                }
            }
        }
    }
    path.canonicalize().ok()
}

pub fn invocation(workspace: Option<&Path>, tool: &str, arguments: &Value) -> Value {
    let args = arguments
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let Some((root, args)) = workspace.zip(args) else {
        return json!({"complete":false});
    };
    let cwd = if tool == "shell" {
        args["workdir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .map(|p| if p.is_absolute() { p } else { root.join(p) })
            .unwrap_or_else(|| root.into())
    } else {
        root.into()
    };
    let Some(cwd) = safe_path(&cwd) else {
        return json!({"complete":false,"reason":"unsafe_workdir"});
    };
    let Some(root) = safe_path(root) else {
        return json!({"complete":false,"reason":"unsafe_workspace"});
    };
    let command = args["command"].as_str();
    let mut complete = cwd.starts_with(&root);
    let paths: Vec<_> = crate::tool_recovery::implicated_paths(&args)
        .iter()
        .map(|p| {
            let path = PathBuf::from(p);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            let resolved = safe_path(&path).or_else(|| {
                (!path.exists())
                    .then(|| {
                        path.parent()
                            .and_then(safe_path)
                            .zip(path.file_name())
                            .map(|(parent, name)| parent.join(name))
                    })
                    .flatten()
            });
            match resolved {
                Some(p) => {
                    if !p.starts_with(&root) {
                        complete = false;
                    }
                    p.to_string_lossy().into_owned()
                }
                None => {
                    complete = false;
                    path.to_string_lossy().into_owned()
                }
            }
        })
        .collect();
    let identity =
        json!({"tool":tool,"workdir":cwd,"command":command,"paths":paths,"arguments":args});
    let text = identity.to_string();
    if super::evidence::redact(&text) != text || text.len() > 16_000 {
        complete = false;
    }
    json!({"complete":complete,"tool":tool,"workdir":cwd,"command":command.map(super::evidence::redact),
        "target_paths":paths,"target_id":content_hash(&text)})
}

pub fn success(e: &super::types::Evidence) -> bool {
    e.authoritative
        && !e.truncated
        && ["ok", "passed"].contains(&e.status.as_str())
        && (e.tool.as_deref() != Some("shell") || e.exit_code == Some(0))
}

pub fn checked_files_current(
    root: &Path,
    runtime: &crate::context_trace::RuntimeProvenance,
) -> bool {
    use sha2::{Digest, Sha256};
    !runtime.content_hashes.is_empty()
        && runtime.target_paths.len() == runtime.content_hashes.len()
        && runtime.target_paths.iter().all(|target| {
            let path = Path::new(target);
            if safe_path(path).as_deref() != Some(path) || !path.starts_with(root) {
                return false;
            }
            let Ok(meta) = std::fs::metadata(path) else {
                return false;
            };
            if meta.len() > 16 * 1024 * 1024 {
                return false;
            }
            let Ok(bytes) = std::fs::read(path) else {
                return false;
            };
            let Ok(after) = std::fs::metadata(path) else {
                return false;
            };
            let hash: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            meta.len() == after.len()
                && meta.modified().ok() == after.modified().ok()
                && safe_path(path).as_deref() == Some(path)
                && runtime.content_hashes.get(target) == Some(&hash)
        })
}
