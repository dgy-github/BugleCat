//! Genuine funded STDIN development only; independent frozen originals retained.
#[path = "verification_apps_stdin_v1_collect/engine.rs"]
mod engine;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
