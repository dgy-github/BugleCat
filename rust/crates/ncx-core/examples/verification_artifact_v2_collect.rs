//! Independent fresh development collector; no formal or holdout authority.
#[path = "verification_artifact_v2_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
