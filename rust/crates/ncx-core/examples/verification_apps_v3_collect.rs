//! Independent APPS development collector, no formal or holdout authority.
#[path = "verification_apps_v3_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
