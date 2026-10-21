//! Run one command under the workspace-write executor to check what works
//! with OS write isolation.
//!
//! `cargo run -p ncx-tools --example sandbox_probe -- <workspace> <command>`
//!
//! Set `NCX_PROBE_UNCONFINED=1` for an unconfined baseline run, and
//! `NCX_PROBE_PASS_ENV=NAME,NAME` to forward variables the minimal sandbox
//! environment drops (e.g. `ProgramFiles(x86)` for MSVC discovery).

use std::collections::HashMap;
use std::time::Instant;

use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
use ncx_tools::PolicyExecutor;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(workspace), Some(command)) = (args.next(), args.next()) else {
        eprintln!("usage: sandbox_probe <workspace> <command>");
        std::process::exit(2);
    };
    let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
    let executor = if std::env::var_os("NCX_PROBE_UNCONFINED").is_some() {
        PolicyExecutor::new()
    } else {
        PolicyExecutor::for_policy(&policy)
    };
    let pass: HashMap<String, String> = std::env::var("NCX_PROBE_PASS_ENV")
        .unwrap_or_default()
        .split(',')
        .filter_map(|name| Some((name.to_string(), std::env::var(name).ok()?)))
        .collect();
    let started = Instant::now();
    let result = executor
        .run_with_env(&command, policy.workspace.as_path(), 600, &pass)
        .await;
    println!("{}", result.render());
    println!("(elapsed {:.1}s)", started.elapsed().as_secs_f64());
    std::process::exit(result.exit_code);
}
