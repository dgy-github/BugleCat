//! Independent formal calibration sidecar for the frozen high resource profile.
#[path = "verification_artifact_profile_formal/engine.rs"]
mod engine;
#[path = "verification_artifact_profile_formal/formal.rs"]
mod formal;
#[path = "verification_artifact_profile_formal/support.rs"]
mod support;
const RUNNER_SOURCE: &[u8] = include_bytes!("verification_artifact_profile_formal.rs");
const SUPPORT_SOURCE: &[u8] = include_bytes!("verification_artifact_profile_formal/support.rs");
const FORMAL_SOURCE: &[u8] = include_bytes!("verification_artifact_profile_formal/formal.rs");
#[tokio::main(flavor = "current_thread")]
async fn main() {
    engine::run().await;
}
