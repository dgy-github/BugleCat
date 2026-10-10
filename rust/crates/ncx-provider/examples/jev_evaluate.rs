//! Explicit billed fixture replay, capped at 30 calls; never part of normal CI.
use ncx_provider::jev;
use serde_json::{json, Value};
use std::{io::Write, path::PathBuf, time::Instant};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(reason) = run().await {
        eprintln!("{reason}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: jev_evaluate <synthetic-cases.json> <new-report.jsonl>".into());
    }
    let path = PathBuf::from(&args[2]);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "report_exists_or_unwritable")?;
    let bytes = std::fs::read(&args[1]).map_err(|_| "cases_unreadable")?;
    let cases: Vec<Value> = serde_json::from_slice(&bytes).map_err(|_| "cases_invalid")?;
    if cases.is_empty()
        || cases.len() > 30
        || cases.iter().any(|c| {
            !c["provenance"]
                .as_str()
                .is_some_and(|s| s.starts_with("synthetic;"))
        })
    {
        return Err("requires_1_to_30_explicit_synthetic_cases".into());
    }
    let key = std::env::var("NCX_JEV_API_KEY").map_err(|_| "missing_key")?;
    let mut calls = 0;
    for case in cases {
        let input = &case["input"];
        let mut state = input.clone();
        state["claim"] = input["claims"][0]["text"].clone();
        // Synthetic fixtures contain no real user data. Do not accept arbitrary
        // trace files through this billed replay entrypoint.
        let started = Instant::now();
        calls += 1;
        let response = jev::check(&key, state).await;
        let row = match response {
            Ok(response) => {
                json!({"id":case["id"],"split":case["split"],"expected":case["expected"],
                "call_status":"ok","elapsed_ms":started.elapsed().as_millis(),"response":response})
            }
            Err(reason) => {
                json!({"id":case["id"],"split":case["split"],"expected":case["expected"],
                "call_status":reason,"elapsed_ms":started.elapsed().as_millis()})
            }
        };
        writeln!(file, "{row}").map_err(|_| "report_write_failed")?;
        file.flush().map_err(|_| "report_write_failed")?;
        println!("{}: {}", row["id"], row["call_status"]);
        if matches!(
            row["call_status"].as_str(),
            Some("http_401" | "http_403" | "http_429")
        ) {
            break;
        }
    }
    println!("billed attempts: {calls}; maximum 30; no retries");
    Ok(())
}
