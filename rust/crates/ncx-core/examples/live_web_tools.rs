//! Live network acceptance for the production web tool implementations.
//!
//! Run explicitly; this is intentionally not part of the offline test suite:
//! `cargo run -p ncx-core --example live_web_tools`

use std::path::PathBuf;

use ncx_core::{ToolContext, ToolRegistry};
use ncx_sandbox::{SandboxPolicy, DANGER_FULL_ACCESS};
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let policy = SandboxPolicy::new(DANGER_FULL_ACCESS, &workspace);
    let tools = ToolRegistry::new(ToolContext::new(workspace, policy));

    let search = tools
        .execute("web_search", &json!({"query": "Rust programming language"}))
        .await;
    let search_ok = report("web_search", &search);

    let fetch = tools
        .execute("web_fetch", &json!({"url": "https://www.rust-lang.org/"}))
        .await;
    let fetch_ok = report("web_fetch", &fetch);
    assert!(search_ok && fetch_ok, "one or more live web tools failed");
}

fn report(name: &str, output: &str) -> bool {
    let ok = !output.trim().is_empty() && !output.trim_start().starts_with("Error:");
    if ok {
        println!("{name}: ok ({} chars)", output.chars().count());
    } else {
        eprintln!("{name}: failed: {output}");
    }
    ok
}
