//! ncx-tools — the agent's file/shell capabilities and the sandboxed executor.
//!
//! Rust port of `nanocodex/tools/` (the core tools) plus
//! `nanocodex/sandbox/executor.py`:
//!
//! * [`patch`] — Codex V4A patch parse + atomic apply (`apply_patch` tool).
//! * [`detect`] — the read-only command heuristic the `shell` tool uses to pick
//!   the no-prompt fast-path under `read-only` sandbox mode.
//! * [`read_file`] — line-numbered file rendering (`read_file` tool).
//! * [`executor`] — subprocess execution with timeout and (on Windows) Job-Object
//!   process containment.

pub mod detect;
pub mod executor;
pub mod managed;
pub mod patch;
pub mod pty;
pub mod read_file;
pub mod text_encoding;

pub use detect::looks_read_only;
pub use executor::{ExecResult, PolicyExecutor};
pub use managed::{ManagedProcess, ProcessOutputChunk, ProcessSnapshot};
pub use patch::{apply_patch, parse_patch, ActionType, ApplyOutcome, FileAction, PatchError};
pub use pty::{PtyOutputChunk, PtyProcess, PtySnapshot};
pub use text_encoding::{
    decode_text, decode_text_lossy, DecodedText, TextEncoding, Utf8StreamDecoder,
};
