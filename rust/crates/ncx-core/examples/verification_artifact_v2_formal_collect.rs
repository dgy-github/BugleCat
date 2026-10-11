//! New mixed-source formal calibration collection; no implicit live mode.
#[path = "verification_artifact_v2_formal_collect/engine.rs"]
mod engine;
#[tokio::main]
async fn main() {
    engine::run().await;
}
