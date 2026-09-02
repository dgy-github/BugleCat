//! Test-only filesystem fixture helpers.
//!
//! A test path must be unique both across independently started test binaries
//! and across concurrently running tests in one binary. `new_session_id`
//! includes the process ID and an in-process sequence in addition to time.

use std::path::PathBuf;

pub(crate) fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}",
        crate::session_index::new_session_id()
    ))
}
