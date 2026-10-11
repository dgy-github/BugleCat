//! Independent resource-profile collector. Default paths are zero-network.
#[path = "verification_artifact_profile_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
