//! Independently qualified, once-gated holdout main collection; default zero HTTP.
#[path = "verification_artifact_holdout_collect/engine.rs"]
mod engine;
#[path = "verification_artifact_holdout/guard.rs"]
mod guard;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
