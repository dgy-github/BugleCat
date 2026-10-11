//! Bounded, one-use STDIN development Jev transport; never certifies a threshold.
#[path = "verification_apps_stdin_v1_jev_development/engine.rs"]
mod engine;
#[path = "verification_apps_stdin_v1_jev_development/fixture.rs"]
mod fixture;
#[path = "verification_apps_stdin_v1_jev_development/provenance.rs"]
mod provenance;
#[path = "verification_apps_stdin_v1_jev_development/support.rs"]
mod support;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = engine::run(std::env::args().skip(1).collect()).await;
    match result {
        Ok(value) => println!("{value}"),
        Err(kind) => {
            eprintln!(
                "{}",
                serde_json::json!({"status":"not_run","kind":kind,"new_paid_calls":0})
            );
            std::process::exit(1);
        }
    }
}
