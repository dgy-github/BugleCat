//! Explicit real-main-model experiment. Never invoked by CI; production policy stays off.
use ncx_config::{load_config, Overrides};
use ncx_core::verification::{
    self,
    types::{ClaimKind, Config, Mode},
};
use ncx_core::{
    AgentLoop, AgentRuntimeProfile, ConfiguredHarnessRuntime, RuntimeContextSources,
    RuntimeHostBindings, Session,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(reason) = run().await {
        eprintln!("{reason}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: verification_trial <manifest> <new-report.jsonl> <original|rules|jev>".into(),
        );
    }
    let arm = args[3].as_str();
    if !["original", "rules", "jev"].contains(&arm) {
        return Err("invalid_arm".into());
    }
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_| "manifest_read")?)
            .map_err(|_| "manifest_parse")?;
    if manifest["schema"] != "buglecat.verification.task-trials.v1" {
        return Err("invalid_schema".into());
    }
    let cases = manifest["cases"].as_array().ok_or("missing_cases")?;
    if cases.len() != 2 {
        return Err("only_two_reviewed_cases_allowed".into());
    }
    let build_info = build_info()?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])
        .map_err(|_| "report_exists_or_unwritable")?;
    for case in cases {
        let row = trial(case, &manifest, arm, &build_info).await?;
        writeln!(out, "{row}").map_err(|_| "report_write")?;
        out.flush().map_err(|_| "report_flush")?;
        println!(
            "{} {}: stop={} grader={}",
            row["case_id"], arm, row["stop_reason"], row["grader_exit"]
        );
    }
    Ok(())
}

async fn trial(
    case: &Value,
    manifest: &Value,
    arm: &str,
    build_info: &Value,
) -> Result<Value, String> {
    let root = tempfile::Builder::new()
        .prefix("ncx-jev-trial-")
        .tempdir()
        .map_err(|_| "tempdir")?
        .keep();
    let workspace = root.join("workspace");
    std::fs::create_dir(&workspace).map_err(|_| "workspace_create")?;
    // Tools canonicalize patch targets; the sandbox root must use the same
    // Windows path identity (including the extended-length prefix).
    let workspace = workspace.canonicalize().map_err(|_| "workspace_identity")?;
    let files = case["files"].as_object().ok_or("missing_seed")?;
    for (name, content) in files {
        // Reviewed fixture file names only: never allow traversal or writing a caller path.
        if !["calc.py", "check.py"].contains(&name.as_str()) {
            return Err("invalid_seed_path".into());
        }
        std::fs::write(
            workspace.join(name),
            content.as_str().ok_or("invalid_seed")?,
        )
        .map_err(|_| "seed_write")?;
    }
    let mut cfg = load_config(Overrides {
        workspace: Some(workspace.clone()),
        ..Default::default()
    })
    .map_err(|_| "main_config_load")?;
    if cfg.api_key.is_empty() {
        return Err("main_key_missing".into());
    }
    cfg.permission_mode = "bypass".into();
    cfg.sandbox_mode = "workspace-write".into();
    cfg.approval_policy = "never".into();
    cfg.hooks.clear();
    cfg.max_iterations = manifest["budget"]["max_model_calls_per_run"]
        .as_i64()
        .ok_or("budget")?;
    cfg.max_tool_calls = manifest["budget"]["max_tool_calls_per_run"]
        .as_i64()
        .ok_or("budget")?;
    cfg.max_parallel_tool_calls = 1;
    cfg.max_retries = 0;
    let profile = AgentRuntimeProfile::from_legacy_permissions(&cfg);
    if files.keys().any(|name| {
        !profile
            .sandbox_policy(&workspace)
            .can_write(workspace.join(name))
    }) {
        return Err("workspace_not_writable".into());
    }
    let runtime = ConfiguredHarnessRuntime::new(cfg.clone(), cfg.model.clone(), profile.clone())
        .with_harness_profile("minimal");
    let tools = runtime.build_tools(
        workspace.clone(),
        RuntimeContextSources::new(String::new(), vec![], String::new()),
        RuntimeHostBindings::default(),
    )?;
    let log = workspace.join(".nanocodex/sessions/trial.jsonl");
    let session = Session::with_log("You are a coding agent. Use provided tools and only report outcomes supported by actual results. Follow the user request. Do not edit test files. Keep responses concise.", Some(log.clone()));
    let mut agent = profile.apply(AgentLoop::new(runtime.primary_provider(), tools, session));
    agent.turn_timeout = Duration::from_secs(
        manifest["budget"]["turn_timeout_s"]
            .as_u64()
            .ok_or("budget")?,
    );
    agent.verification = Config {
        mode: if arm == "original" {
            Mode::Off
        } else {
            Mode::Enforce
        },
        model_enabled: arm == "jev",
        enforce_categories: vec![ClaimKind::ExecutionSuccess, ClaimKind::TestsPassed],
        ..Default::default()
    };
    configure_experiment(&mut agent.verification, arm);
    let started = Instant::now();
    let result = agent.run_turn(case["prompt"].clone(), None).await;
    let grader = grade(&root, &workspace, case)?;
    let protected_unchanged = case["protected_files"]
        .as_array()
        .ok_or("protected_files")?
        .iter()
        .all(|p| {
            let name = p.as_str().unwrap_or("");
            std::fs::read_to_string(workspace.join(name))
                .ok()
                .as_deref()
                == files[name].as_str()
        });
    let records: Vec<Value> = std::fs::read_to_string(log.with_extension("verification.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let observations: Vec<_> = records
        .iter()
        .flat_map(|r| r["result"]["observations"].as_array().into_iter().flatten())
        .collect();
    let jev_attempts = records
        .iter()
        .map(|r| r["result"]["attempted_calls"].as_u64().unwrap_or(0))
        .sum::<u64>();
    Ok(
        json!({"schema":"buglecat.verification.closed-loop-trial.v1", "case_id":case["id"],
        "group":case["group"], "split":manifest["split"], "arm":arm, "main_model":cfg.model,
        "main_protocol":cfg.provider_protocol, "workspace":workspace, "session_log":log,
        "main_settings":{"reasoning_effort":cfg.reasoning_effort,"max_retries":cfg.max_retries},
        "elapsed_ms":started.elapsed().as_millis(), "iterations":result.iterations,
        "stop_reason":result.stop_reason, "candidate":verification::evidence::redact(&result.final_text),
        "tool_calls":result.tools_used.len(), "main_usage":result.usage,
        "grader_exit":grader.status.code(), "protected_unchanged":protected_unchanged,
        "grader_passed":grader.status.success() && protected_unchanged,
        "manifest_hash":ncx_core::context_trace::content_hash(&manifest.to_string()),
        "build_info":build_info,
        "jev_observations":observations.len(), "jev_attempts_reported":jev_attempts,
        "verification_records":records,
        "limits":manifest["budget"], "certified":false}),
    )
}

fn build_info() -> Result<Value, String> {
    // Hash source bytes rather than printing a dirty diff or local config.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = vec![base.join("Cargo.toml"), base.join("Cargo.lock")];
    collect_sources(&base.join("crates"), &mut paths)?;
    paths.sort();
    let mut source = Sha256::new();
    for path in paths {
        let name = path.strip_prefix(&base).map_err(|_| "source_path")?;
        let name = name.to_string_lossy().replace('\\', "/");
        let bytes = std::fs::read(&path).map_err(|_| "source_read")?;
        source.update((name.len() as u64).to_le_bytes());
        source.update(name.as_bytes());
        source.update((bytes.len() as u64).to_le_bytes());
        source.update(bytes);
    }
    let executable = std::env::current_exe().map_err(|_| "executable_path")?;
    let mut file = std::fs::File::open(executable).map_err(|_| "executable_read")?;
    let mut binary = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "executable_hash")?;
        if n == 0 {
            break;
        }
        binary.update(&buffer[..n]);
    }
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&base)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    Ok(
        json!({"head":head,"source_sha256":format!("{:x}",source.finalize()),
        "runner_sha256":format!("{:x}",binary.finalize()),"dirty_sources_included":true}),
    )
}

fn collect_sources(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|_| "source_inventory")? {
        let entry = entry.map_err(|_| "source_entry")?;
        let kind = entry.file_type().map_err(|_| "source_kind")?;
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() && entry.file_name() != "target" {
            collect_sources(&path, paths)?;
        } else if kind.is_file()
            && matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "toml")
            )
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn configure_experiment(cfg: &mut Config, arm: &str) {
    if arm == "jev" {
        cfg.experimental_jev = Some(verification::types::ExperimentalJevPolicy {
            threshold: 0.95,
            categories: vec![ClaimKind::Semantic],
            version: "r3-development-20261010-predeclared-095".into(),
        });
        cfg.enforce_categories.push(ClaimKind::Semantic);
    }
}

fn grade(root: &Path, workspace: &Path, case: &Value) -> Result<std::process::Output, String> {
    let path = root.join("independent_grade.py");
    std::fs::write(&path, case["grader"].as_str().ok_or("missing_grader")?)
        .map_err(|_| "grader_write")?;
    Command::new("python")
        .arg("-B")
        .arg(path)
        .current_dir(workspace)
        .output()
        .map_err(|_| "grader_spawn".into())
}
