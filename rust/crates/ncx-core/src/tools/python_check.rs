//! Syntax-only validation: never import or execute the edited program.
use super::ToolContext;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub(crate) async fn check(ctx: &ToolContext, path: &Path) -> String {
    // Cancellation must leave an unverified entry, never a stale success.
    ctx.python_checks.borrow_mut().insert(
        path.to_path_buf(),
        Some("verification_failed: check interrupted".into()),
    );
    let result = tokio::time::timeout(Duration::from_secs(15), compile(ctx, path)).await;
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("verification_failed: compiler timeout".into()),
    };
    let evidence = serde_json::json!({
        "event": "post_edit_compile", "path": path,
        "status": error.as_deref().map(|e| e.split(':').next().unwrap()).unwrap_or("compile_passed"),
        "detail": error
    })
    .to_string();
    ctx.python_checks
        .borrow_mut()
        .insert(path.to_path_buf(), error);
    evidence
}

async fn compile(ctx: &ToolContext, path: &Path) -> Result<(), String> {
    let resolved = path
        .canonicalize()
        .map_err(|_| "verification_failed: file missing")?;
    if !ctx.policy.can_read(&resolved) {
        return Err("verification_failed: read denied".into());
    }
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|_| "verification_failed: read error")?;
    // -I -S prevents workspace/PYTHONPATH/site startup execution. Only bytecode
    // compilation runs, no imports from the target and no __pycache__ writes.
    let script = "import sys\ntry:\n compile(sys.stdin.buffer.read(), '<edited>', 'exec')\nexcept SyntaxError as e:\n print(type(e).__name__ + ': line ' + str(e.lineno))\n sys.exit(1)\n";
    let mut child = Command::new(if cfg!(windows) { "python" } else { "python3" })
        .args(["-I", "-S", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "verification_failed: Python unavailable")?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&bytes)
        .await
        .map_err(|_| "verification_failed: compiler input")?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|_| "verification_failed: compiler execution")?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stdout);
    if output.status.code() == Some(1) && detail.contains("Error: line ") {
        Err(format!("compile_failed: {}", detail.trim()))
    } else {
        Err("verification_failed: compiler failed".into())
    }
}

pub(crate) fn failure(ctx: &ToolContext) -> Option<&'static str> {
    let checks = ctx.python_checks.borrow();
    if checks
        .values()
        .flatten()
        .any(|e| e.starts_with("compile_failed"))
    {
        Some("compile_failed")
    } else if checks.values().any(Option::is_some) {
        Some("verification_failed")
    } else {
        None
    }
}

pub(crate) async fn recheck(ctx: &ToolContext) -> Vec<String> {
    let paths: Vec<_> = ctx.python_checks.borrow().keys().cloned().collect();
    let mut evidence = Vec::new();
    for path in paths {
        evidence.push(check(ctx, &path).await);
    }
    evidence
}
