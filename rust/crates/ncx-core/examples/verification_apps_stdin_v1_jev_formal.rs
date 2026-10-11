//! Bounded, one-use formal STDIN calibration Jev transport; no threshold certificate.
#[path = "verification_apps_stdin_v1_jev_formal/engine.rs"]
mod engine;
#[path = "verification_apps_stdin_v1_jev_formal/fixture.rs"]
mod fixture;
#[path = "verification_apps_stdin_v1_jev_formal/provenance.rs"]
mod provenance;
#[path = "verification_apps_stdin_v1_jev_formal/support.rs"]
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
