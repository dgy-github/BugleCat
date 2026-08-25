use super::*;
use crate::session_recorder::SessionRecorder;

pub(crate) fn compact_session_text(
    agent: &mut AgentLoop,
    recorder: &mut SessionRecorder,
) -> String {
    let stats = agent.session.compact(&agent.context_edit);
    if let Err(error) = recorder.replace_model_context(&agent.session) {
        return format!("Compaction succeeded but persistence failed: {error}");
    }
    format!(
        "Compacted session: chars {} -> {}; compressed_tool_results={} dropped_messages={}",
        stats.original_chars,
        stats.edited_chars,
        stats.compressed_tool_results,
        stats.dropped_messages
    )
}

pub(crate) fn checkpoint_before_turn(workspace: &Path, prompt: &str) {
    let label = format!("auto: {}", clipped_label(prompt, 80));
    match CheckpointStore::new(workspace).create(&label) {
        Ok(meta) => eprintln!(
            "checkpoint {} saved ({} file(s), {} skipped).",
            meta.id,
            meta.files.len(),
            meta.skipped_paths.len()
        ),
        Err(e) => eprintln!("checkpoint warning: {e}"),
    }
}

pub(crate) fn create_checkpoint_text(workspace: &Path, label: &str) -> String {
    let label = if label.trim().is_empty() {
        "manual checkpoint"
    } else {
        label.trim()
    };
    match CheckpointStore::new(workspace).create(label) {
        Ok(meta) => format_checkpoint_saved(&meta),
        Err(e) => format!("checkpoint failed: {e}"),
    }
}

pub(crate) fn restore_checkpoint_text(workspace: &Path, id: &str) -> String {
    if id.trim().is_empty() {
        return "usage: /restore <checkpoint-id>".into();
    }
    match CheckpointStore::new(workspace).restore(id) {
        Ok(report) => {
            let safety = report
                .safety_checkpoint_id
                .map(|id| format!("\nsafety checkpoint: {id}"))
                .unwrap_or_else(|| "\nsafety checkpoint: failed".into());
            format!(
                "restored checkpoint {}\nrestored_files: {}\ndeleted_files: {}{}",
                report.checkpoint_id, report.restored_files, report.deleted_files, safety
            )
        }
        Err(e) => format!("restore failed: {e}"),
    }
}

pub(crate) fn format_checkpoint_saved(meta: &CheckpointMeta) -> String {
    format!(
        "checkpoint: {}\nlabel: {}\nfiles: {}  skipped: {}  bytes: {}",
        meta.id,
        meta.label,
        meta.files.len(),
        meta.skipped_paths.len(),
        meta.total_bytes
    )
}

pub(crate) fn render_checkpoints(entries: &[CheckpointMeta], limit: usize) -> String {
    if entries.is_empty() {
        return "No checkpoints.".into();
    }
    let mut out = String::from("Checkpoints:");
    for meta in entries.iter().take(limit) {
        out.push_str(&format!(
            "\n  {}  {}  {}  files={} skipped={}",
            meta.created_at,
            meta.id,
            if meta.label.is_empty() {
                "(unlabeled)"
            } else {
                meta.label.as_str()
            },
            meta.files.len(),
            meta.skipped_paths.len()
        ));
    }
    out
}

pub(crate) fn clipped_label(text: &str, limit: usize) -> String {
    let s = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= limit {
        s
    } else {
        format!(
            "{}...",
            s.chars().take(limit.saturating_sub(3)).collect::<String>()
        )
    }
}

pub(crate) fn runtime_profile_for_args(cfg: &Config, args: &Args) -> AgentRuntimeProfile {
    match args.permission_mode.as_deref() {
        Some(mode) => AgentRuntimeProfile::from_permission_mode(cfg, mode),
        None if args.sandbox.is_some() || args.approval.is_some() => {
            AgentRuntimeProfile::from_legacy_permissions(cfg)
        }
        None => AgentRuntimeProfile::from_config(cfg),
    }
}

/// Build the one-shot user input. With no images it is just the prompt text;
/// with `--image` paths it becomes an OpenAI-style multimodal `content` array
/// (`text` block + one `image_url` block per file, each a base64 `data:` URL),
/// which trips [`AgentLoop`]'s image detection and routes to the vision model.
pub(crate) fn build_image_user_input(
    text: &str,
    images: &[PathBuf],
) -> Result<serde_json::Value, String> {
    if images.is_empty() {
        return Ok(json!(text));
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    for path in images {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("cannot read image {}: {e}", path.display()))?;
        let url = format!("data:{};base64,{}", image_mime(path), base64_encode(&bytes));
        content.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Ok(serde_json::Value::Array(content))
}

pub(crate) fn validate_attachments(
    tools: &ncx_core::ToolRegistry,
    images: &[PathBuf],
) -> Result<(), String> {
    if images.is_empty() {
        return Ok(());
    }
    let service = tools
        .service::<ncx_core::AttachmentServiceDescriptor>("attachment")
        .ok_or_else(|| "当前 Harness Profile 未启用附件插件".to_string())?;
    for path in images {
        let extension = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !service
            .extensions
            .iter()
            .any(|allowed| allowed == &extension)
        {
            return Err(format!("附件格式 .{extension} 未被当前插件允许"));
        }
        let size = std::fs::metadata(path)
            .map_err(|e| format!("cannot read image {}: {e}", path.display()))?
            .len();
        if size > service.max_bytes {
            return Err(format!(
                "附件 {} 超过 {} 字节限制",
                path.display(),
                service.max_bytes
            ));
        }
    }
    Ok(())
}

/// Guess an image MIME type from the file extension (defaults to PNG).
pub(crate) fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// Standard base64 encoding (RFC 4648, with `=` padding). Hand-rolled to avoid a
/// new crate dependency for the single image-attachment use site.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
