//! nanocodex GUI (Tauri v2) — Rust backend.
//!
//! The agent loop runs on a dedicated `!Send` thread (see [`bridge`]); the
//! frontend talks to it through the `send_prompt` command and listens for
//! `ncx://event` window events. `get_status` is a cheap synchronous snapshot
//! for the header.

mod bridge;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use ncx_config::{
    load_config, write_nanocodex_config, ConfigPaths, Overrides, VALID_APPROVAL_POLICIES,
    VALID_SANDBOX_MODES,
};
use ncx_core::{CheckpointMeta, CheckpointStore, MemoryStore, RestoreReport, SessionIndex};
use serde::Serialize;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use bridge::{spawn_worker, Command, PendingMap};

#[derive(Serialize)]
pub struct Status {
    model: String,
    sandbox: String,
    approval: String,
    permission_mode: String,
    workspace: String,
    /// Masked (`****1234`) — never the real key.
    api_key: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    price_in: f64,
    price_out: f64,
}

/// Tauri managed state: the channel into the agent thread + pending approvals.
struct AppState {
    tx: UnboundedSender<Command>,
    pending: PendingMap,
}

#[derive(Serialize)]
pub struct CheckpointView {
    id: String,
    label: String,
    created_at: String,
    files: usize,
    skipped: usize,
    total_bytes: u64,
}

#[derive(Serialize)]
pub struct ConfigLocation {
    config_path: String,
    config_dir: String,
}

#[derive(Serialize)]
pub struct RestoreView {
    checkpoint_id: String,
    safety_checkpoint_id: Option<String>,
    restored_files: usize,
    deleted_files: usize,
}

/// Load the resolved config and return a display-safe snapshot.
#[tauri::command]
fn get_status() -> Result<Status, String> {
    let workspace = std::env::current_dir().ok();
    let overrides = Overrides {
        workspace,
        ..Default::default()
    };
    let cfg = load_config(overrides).map_err(|e| e.to_string())?;
    let red = cfg.redacted();
    Ok(Status {
        model: cfg.model.clone(),
        sandbox: cfg.sandbox_mode.clone(),
        approval: cfg.approval_policy.clone(),
        permission_mode: cfg.permission_mode.clone(),
        workspace: cfg.workspace.display().to_string(),
        api_key: red.get("api_key").cloned().unwrap_or_default(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        price_in: cfg.price_in,
        price_out: cfg.price_out,
    })
}

/// Queue a user prompt for the agent thread. `images` are absolute paths from
/// the file picker (attached as base64 vision blocks); non-image files are
/// passed by the UI as `@path` tokens inside `text`. Replies arrive as
/// `ncx://event`s.
#[tauri::command]
fn send_prompt(
    text: String,
    images: Option<Vec<String>>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .tx
        .send(Command::Prompt {
            text,
            images: images.unwrap_or_default(),
        })
        .map_err(|_| "agent thread is not running".to_string())
}

#[tauri::command]
fn get_config_location() -> Result<ConfigLocation, String> {
    config_location()
}

/// Switch the agent's workspace (the directory it operates on). Sets the process
/// working directory — which every command resolves against — then reloads the
/// agent so the new root, its project instructions, memory, and git all apply.
#[tauri::command]
fn set_workspace(path: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let p = PathBuf::from(path.trim());
    if !p.is_dir() {
        return Err(format!("not a directory: {}", p.display()));
    }
    std::env::set_current_dir(&p).map_err(|e| format!("cannot enter {}: {e}", p.display()))?;
    let _ = state.tx.send(Command::Reload);
    Ok(p.display().to_string())
}

/// Change the approval policy live (no session reset) + persist it.
#[tauri::command]
fn set_approval(policy: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetApproval(policy))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Change the sandbox mode live (auto-execute = danger-full-access) + persist.
#[tauri::command]
fn set_sandbox(mode: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetSandbox(mode))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Switch the active model (persists + rebuilds keeping the current transcript).
#[tauri::command]
fn set_model(model: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetModel(model))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Switch the CC permission mode (plan / default / accept-edits / bypass).
#[tauri::command]
fn set_permission_mode(mode: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetPermissionMode(mode))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Ask the agent thread to re-emit its `ready` snapshot (called by the UI once
/// its event listener is up, so the initial emit isn't missed).
#[tauri::command]
fn request_ready(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::RequestReady)
        .map_err(|_| "agent thread is not running".to_string())
}

/// Archive (or unarchive) a saved session; persisted in the session index.
#[tauri::command]
fn archive_session(
    session_id: String,
    archived: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .tx
        .send(Command::ArchiveSession(session_id, archived))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Start a fresh session (rebuild the agent from config — new empty context).
#[tauri::command]
fn new_session(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::Reload)
        .map_err(|_| "agent thread is not running".to_string())
}

/// Continue a saved session (reseed the agent from its snapshot, same id).
#[tauri::command]
fn resume_session(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::Resume(session_id))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Fork a saved session (reseed a NEW session from its snapshot; source kept).
#[tauri::command]
fn fork_session(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::Fork(session_id))
        .map_err(|_| "agent thread is not running".to_string())
}

/// The current workspace (process working directory).
#[tauri::command]
fn get_workspace() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_config_file() -> Result<(), String> {
    let path = ensure_config_file()?;
    open_file(&path)
}

#[tauri::command]
fn open_config_dir() -> Result<(), String> {
    let dir = ensure_config_dir()?;
    open_dir(&dir)
}

/// The editable settings shown in the Settings panel. The API key is never
/// returned in the clear — only whether one is set, plus a masked tail.
#[derive(Serialize)]
pub struct Settings {
    model: String,
    base_url: String,
    sandbox_mode: String,
    approval_policy: String,
    reasoning_effort: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    context_edit_keep_recent_messages: i64,
    context_edit_max_tool_result_chars: i64,
    price_in: f64,
    price_out: f64,
    api_key_masked: String,
    has_api_key: bool,
    available_models: Vec<String>,
    sandbox_modes: Vec<String>,
    approval_policies: Vec<String>,
}

/// Read the current settings for the panel (with dropdown option lists).
#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    let workspace = std::env::current_dir().ok();
    let cfg = load_config(Overrides {
        workspace,
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let masked = cfg.redacted().get("api_key").cloned().unwrap_or_default();
    Ok(Settings {
        model: cfg.model.clone(),
        base_url: cfg.base_url.clone(),
        sandbox_mode: cfg.sandbox_mode.clone(),
        approval_policy: cfg.approval_policy.clone(),
        reasoning_effort: cfg.reasoning_effort.clone(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        context_edit_keep_recent_messages: cfg.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: cfg.context_edit_max_tool_result_chars,
        price_in: cfg.price_in,
        price_out: cfg.price_out,
        api_key_masked: masked,
        has_api_key: !cfg.api_key.is_empty(),
        available_models: cfg.available_models.clone(),
        sandbox_modes: VALID_SANDBOX_MODES.iter().map(|s| s.to_string()).collect(),
        approval_policies: VALID_APPROVAL_POLICIES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Persist settings to `~/.nanocodex/config.toml`, then rebuild the agent so the
/// change applies live. Empty values are skipped (so a blank API key keeps the
/// existing one). Only known keys are written.
#[tauri::command]
fn save_settings(
    updates: std::collections::HashMap<String, String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let borrowed: HashMap<&str, &str> = updates
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let path = ConfigPaths::default().nanocodex;
    write_nanocodex_config(&borrowed, &path).map_err(|e| e.to_string())?;
    // Apply live (fresh session with the new config).
    let _ = state.tx.send(Command::Reload);
    Ok(())
}

fn config_location() -> Result<ConfigLocation, String> {
    let path = ConfigPaths::default().nanocodex;
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent directory".to_string())?
        .to_path_buf();
    Ok(ConfigLocation {
        config_path: path.display().to_string(),
        config_dir: dir.display().to_string(),
    })
}

fn ensure_config_dir() -> Result<PathBuf, String> {
    let path = ConfigPaths::default().nanocodex;
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent directory".to_string())?
        .to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn ensure_config_file() -> Result<PathBuf, String> {
    let path = ConfigPaths::default().nanocodex;
    if !path.exists() {
        let empty: HashMap<&str, &str> = HashMap::new();
        write_nanocodex_config(&empty, &path).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn open_file(path: &Path) -> Result<(), String> {
    ProcessCommand::new("notepad.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open config file: {e}"))
}

#[cfg(target_os = "windows")]
fn open_dir(path: &Path) -> Result<(), String> {
    ProcessCommand::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open config directory: {e}"))
}

#[cfg(target_os = "macos")]
fn open_file(path: &Path) -> Result<(), String> {
    open_with("open", path, "config file")
}

#[cfg(target_os = "macos")]
fn open_dir(path: &Path) -> Result<(), String> {
    open_with("open", path, "config directory")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_file(path: &Path) -> Result<(), String> {
    open_with("xdg-open", path, "config file")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_dir(path: &Path) -> Result<(), String> {
    open_with("xdg-open", path, "config directory")
}

#[cfg(not(target_os = "windows"))]
fn open_with(program: &str, path: &Path, label: &str) -> Result<(), String> {
    ProcessCommand::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open {label}: {e}"))
}

/// Answer a pending approval request (raised by an `approval` event).
/// `decision` is "deny" | "once" | "always" (always = remember this session).
#[tauri::command]
fn approve(id: u64, decision: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let dec = match decision.as_str() {
        "always" => ncx_core::ApprovalDecision::Always,
        "once" | "approve" | "yes" | "true" => ncx_core::ApprovalDecision::Once,
        _ => ncx_core::ApprovalDecision::Deny,
    };
    let sender = state.pending.lock().unwrap().remove(&id);
    match sender {
        Some(tx) => tx
            .send(dec)
            .map_err(|_| "approval already resolved".to_string()),
        None => Err(format!("no pending approval with id {id}")),
    }
}

#[tauri::command]
fn get_checkpoints() -> Result<Vec<CheckpointView>, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    Ok(CheckpointStore::new(&cfg.workspace)
        .list()
        .into_iter()
        .map(checkpoint_view)
        .collect())
}

/// The files captured by a checkpoint (for the checkpoint detail expander).
#[tauri::command]
fn checkpoint_files(id: String) -> Result<Vec<String>, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    CheckpointStore::new(&cfg.workspace)
        .get(&id)
        .map(|m| m.files)
        .ok_or_else(|| format!("no checkpoint with id {id}"))
}

#[tauri::command]
fn create_checkpoint(label: String) -> Result<CheckpointView, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let label = if label.trim().is_empty() {
        "manual checkpoint"
    } else {
        label.trim()
    };
    CheckpointStore::new(&cfg.workspace)
        .create(label)
        .map(checkpoint_view)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_checkpoint(id: String) -> Result<RestoreView, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    CheckpointStore::new(&cfg.workspace)
        .restore(&id)
        .map(restore_view)
        .map_err(|e| e.to_string())
}

fn checkpoint_view(meta: CheckpointMeta) -> CheckpointView {
    CheckpointView {
        id: meta.id,
        label: meta.label,
        created_at: meta.created_at,
        files: meta.files.len(),
        skipped: meta.skipped_paths.len(),
        total_bytes: meta.total_bytes,
    }
}

fn restore_view(report: RestoreReport) -> RestoreView {
    RestoreView {
        checkpoint_id: report.checkpoint_id,
        safety_checkpoint_id: report.safety_checkpoint_id,
        restored_files: report.restored_files,
        deleted_files: report.deleted_files,
    }
}

// ── Phase 1: git branches + diff + session history (no agent-thread bridge) ────

#[derive(Serialize)]
pub struct BranchInfo {
    name: String,
    current: bool,
}

#[derive(Serialize)]
pub struct SessionRow {
    session_id: String,
    title: String,
    snippet: String,
    user_messages: usize,
    assistant_messages: usize,
    tool_calls: usize,
    updated_at: String,
    has_snapshot: bool,
    archived: bool,
}

/// Run a git command in the workspace; Ok(stdout) or Err(stderr).
fn run_git(args: &[&str]) -> Result<String, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let out = ProcessCommand::new("git")
        .args(args)
        .current_dir(&ws)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("git {args:?} failed")
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
fn git_branches() -> Result<Vec<BranchInfo>, String> {
    let current = run_git(&["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    let listing = run_git(&["branch", "--format=%(refname:short)"])?;
    Ok(listing
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|name| BranchInfo {
            current: name == current,
            name: name.to_string(),
        })
        .collect())
}

#[tauri::command]
fn git_create_branch(name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    run_git(&["checkout", "-b", name]).map(|_| ())
}

#[tauri::command]
fn git_switch_branch(name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    run_git(&["checkout", name]).map(|_| ())
}

#[derive(Serialize)]
pub struct CommitInfo {
    hash: String,
    subject: String,
    when: String,
}

/// Recent commits on a branch (for the branch detail expander).
#[tauri::command]
fn git_log(name: String, limit: u32) -> Result<Vec<CommitInfo>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    let n = format!("-{}", limit.clamp(1, 50));
    // %h <US> subject <US> relative-date  (0x1f field separator).
    let out = run_git(&["log", &n, "--pretty=format:%h\u{1f}%s\u{1f}%cr", name])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let p: Vec<&str> = line.split('\u{1f}').collect();
            (p.len() == 3).then(|| CommitInfo {
                hash: p[0].to_string(),
                subject: p[1].to_string(),
                when: p[2].to_string(),
            })
        })
        .collect())
}

/// The working-tree diff vs HEAD (staged + unstaged) for the diff panel.
#[tauri::command]
fn git_diff() -> Result<String, String> {
    let out = run_git(&["diff", "HEAD"])?;
    Ok(if out.trim().is_empty() {
        "(no changes in the working tree)".into()
    } else {
        out
    })
}

#[derive(Serialize)]
pub struct FileChange {
    path: String,
    added: i64,   // -1 = unknown (binary/untracked)
    removed: i64,
    kind: String, // modified | added | deleted | renamed | untracked
}

/// The working-tree change set vs HEAD: one entry per changed file with +/-
/// line counts (like the reference's working-tree panel).
#[tauri::command]
fn git_changes() -> Result<Vec<FileChange>, String> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, FileChange> = BTreeMap::new();
    // Tracked changes vs HEAD: added \t removed \t path.
    if let Ok(numstat) = run_git(&["diff", "HEAD", "--numstat"]) {
        for line in numstat.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 3 {
                let path = parts[2].trim().to_string();
                map.insert(
                    path.clone(),
                    FileChange {
                        added: parts[0].parse().unwrap_or(-1),
                        removed: parts[1].parse().unwrap_or(-1),
                        kind: "modified".into(),
                        path,
                    },
                );
            }
        }
    }
    // Status pass: refine kind + add untracked files.
    if let Ok(st) = run_git(&["status", "--porcelain"]) {
        for line in st.lines() {
            if line.len() < 4 {
                continue;
            }
            let code = &line[..2];
            let path = line[3..].trim().trim_matches('"').to_string();
            let kind = if code.contains('?') {
                "untracked"
            } else if code.contains('A') {
                "added"
            } else if code.contains('D') {
                "deleted"
            } else if code.contains('R') {
                "renamed"
            } else {
                "modified"
            };
            map.entry(path.clone())
                .and_modify(|f| f.kind = kind.to_string())
                .or_insert(FileChange {
                    path,
                    added: -1,
                    removed: -1,
                    kind: kind.to_string(),
                });
        }
    }
    Ok(map.into_values().collect())
}

/// The diff for a single file (vs HEAD). Untracked files show their content as
/// added lines.
#[tauri::command]
fn git_file_diff(path: String) -> Result<String, String> {
    let out = run_git(&["diff", "HEAD", "--", &path]).unwrap_or_default();
    if !out.trim().is_empty() {
        return Ok(out);
    }
    // Untracked / no tracked diff: show the file content as added lines.
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    match std::fs::read_to_string(ws.join(&path)) {
        Ok(c) => Ok(c
            .lines()
            .take(500)
            .map(|l| format!("+{l}"))
            .collect::<Vec<_>>()
            .join("\n")),
        Err(_) => Ok("(no textual diff — binary or unreadable)".into()),
    }
}

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String, // workspace-relative, forward slashes
    is_dir: bool,
}

/// List a directory under the workspace (`rel` = "" for the root). Skips heavy
/// noise dirs; dirs first, then files, alphabetical.
#[tauri::command]
fn list_dir(rel: String) -> Result<Vec<DirEntry>, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let wsc = ws.canonicalize().unwrap_or(ws.clone());
    let target = if rel.trim().is_empty() {
        wsc.clone()
    } else {
        wsc.join(&rel)
    };
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&wsc) {
        return Err("path is outside the workspace".into());
    }
    const SKIP: &[&str] = &[".git", "node_modules", "target", ".nanocodex"];
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&target).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let p = entry.path();
        let is_dir = p.is_dir();
        let path = p
            .strip_prefix(&wsc)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(DirEntry { name, path, is_dir });
    }
    out.sort_by(|a, b| {
        (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
    });
    Ok(out)
}

/// Read a workspace file's text for the file-preview panel. Mirrors `list_dir`'s
/// containment; capped; refuses non-UTF-8 (binary) files.
#[tauri::command]
fn read_workspace_file(rel: String) -> Result<String, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let wsc = ws.canonicalize().unwrap_or(ws);
    let target = wsc.join(&rel).canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&wsc) {
        return Err("path is outside the workspace".into());
    }
    let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if meta.len() > 400_000 {
        return Err(format!("文件太大，无法预览（{} KB）", meta.len() / 1024));
    }
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "二进制文件，无法预览".to_string())
}

/// Open an http(s) URL in the default browser (e.g. the /feedback command).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    #[cfg(target_os = "windows")]
    {
        ProcessCommand::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ProcessCommand::new("xdg-open").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}

#[derive(Serialize)]
pub struct McpRow {
    name: String,
    command: String,
}

/// Configured MCP servers (from ~/.nanocodex/mcp.toml) for the /mcp command.
#[tauri::command]
fn list_mcp() -> Result<Vec<McpRow>, String> {
    Ok(ncx_config::load_mcp_servers()
        .into_iter()
        .map(|s| {
            let command = if s.args.is_empty() {
                s.command
            } else {
                format!("{} {}", s.command, s.args.join(" "))
            };
            McpRow { name: s.name, command }
        })
        .collect())
}

/// Write pasted/clipboard image bytes to a temp file and return its path, so it
/// can be attached through the normal image pipeline.
#[tauri::command]
fn save_temp_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let dir = std::env::temp_dir().join("ncx_gui_paste");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ext = if ext.trim().is_empty() { "png".into() } else { ext };
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("paste_{n}.{ext}"));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn list_sessions() -> Result<Vec<SessionRow>, String> {
    let mut entries = SessionIndex::default().entries();
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)); // newest first
    Ok(entries
        .into_iter()
        .take(50)
        .map(|s| SessionRow {
            session_id: s.session_id,
            title: s.title,
            snippet: s.snippet,
            user_messages: s.user_messages,
            assistant_messages: s.assistant_messages,
            tool_calls: s.tool_calls,
            updated_at: s.updated_at,
            has_snapshot: s.has_snapshot,
            archived: s.archived,
        })
        .collect())
}

// ── Hermes: project-memory self-evolution panel ───────────────────────────────

#[derive(Serialize)]
pub struct MemoryNote {
    ts: u64,
    tags: Vec<String>,
    text: String,
}

/// The project memory store for the current workspace.
fn memory_store() -> MemoryStore {
    let ws = std::env::current_dir().unwrap_or_default();
    MemoryStore::new(ws.join(".ncx").join("memory"))
}

/// List accumulated learnings (newest first).
#[tauri::command]
fn memory_list() -> Result<Vec<MemoryNote>, String> {
    let mut entries = memory_store().entries();
    entries.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(entries
        .into_iter()
        .map(|e| MemoryNote {
            ts: e.ts,
            tags: e.tags,
            text: e.text,
        })
        .collect())
}

/// Trigger self-evolution maintenance: fold near-duplicate notes (heuristic,
/// local — no model). Returns how many entries were removed.
#[tauri::command]
fn memory_consolidate() -> Result<usize, String> {
    memory_store().consolidate(0.85).map_err(|e| e.to_string())
}

/// Manually record a verified learning into project memory.
#[tauri::command]
fn memory_add(note: String, tags: Vec<String>) -> Result<bool, String> {
    let note = note.trim();
    if note.is_empty() {
        return Err("note is required".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    memory_store()
        .remember(note, &tags, now)
        .map_err(|e| e.to_string())
}

pub fn run() {
    let (tx, rx) = unbounded_channel::<Command>();
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let pending_for_worker = pending.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState { tx, pending })
        .setup(move |app| {
            // Hand the agent thread an AppHandle (to emit events), the receiver
            // (to take prompts), and the shared pending-approvals map.
            spawn_worker(app.handle().clone(), rx, pending_for_worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            send_prompt,
            approve,
            get_settings,
            save_settings,
            get_config_location,
            open_config_file,
            open_config_dir,
            get_checkpoints,
            checkpoint_files,
            create_checkpoint,
            restore_checkpoint,
            git_branches,
            git_log,
            git_create_branch,
            git_switch_branch,
            git_diff,
            git_changes,
            git_file_diff,
            list_dir,
            read_workspace_file,
            open_url,
            list_mcp,
            save_temp_image,
            list_sessions,
            memory_list,
            memory_consolidate,
            memory_add,
            set_workspace,
            get_workspace,
            resume_session,
            fork_session,
            archive_session,
            new_session,
            set_approval,
            set_sandbox,
            set_model,
            set_permission_mode,
            request_ready
        ])
        .run(tauri::generate_context!())
        .expect("error while running the nanocodex GUI");
}
