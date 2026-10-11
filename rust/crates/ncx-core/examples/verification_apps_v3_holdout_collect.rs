//! Independent APPS holdout main collector; genuine shared once gate required.
#[path = "verification_apps_v3_holdout_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
