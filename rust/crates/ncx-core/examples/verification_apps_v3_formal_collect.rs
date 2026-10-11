//! Independent APPS formal calibration main collector; holdout denied.
#[path = "verification_apps_v3_formal_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
