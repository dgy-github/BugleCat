//! Test-only filesystem fixture helpers.
//!
//! Directory names include a process ID, nanosecond timestamp, and an
//! in-process counter. Creating the leaf atomically means concurrent test
//! binaries cannot reuse or delete one another's fixtures.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn unique_temp_dir(prefix: &str) -> PathBuf {
    for _ in 0..16 {
        let sequence = TEST_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create unique test directory {}: {error}", path.display()),
        }
    }

    panic!("could not allocate a unique test directory for {prefix}");
}
