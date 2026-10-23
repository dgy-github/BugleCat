//! Managed subprocess lifecycle using the same command boundary as PolicyExecutor.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::task::JoinHandle;

use crate::executor::{command_with_env, spawn_contained, Containment, PolicyExecutor};
use crate::output_buffer::{OutputRingBuffer, SequencedChunk};
use crate::text_encoding::Utf8StreamDecoder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutputChunk {
    pub seq: u64,
    pub stream: &'static str,
    pub text: String,
}

impl SequencedChunk for ProcessOutputChunk {
    fn seq(&self) -> u64 {
        self.seq
    }
    fn text_len(&self) -> usize {
        self.text.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub running: bool,
    pub exit_code: Option<i32>,
    pub chunks: Vec<ProcessOutputChunk>,
    pub next_cursor: u64,
}

pub struct ManagedProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    output: Arc<Mutex<OutputRingBuffer<ProcessOutputChunk>>>,
    readers: Vec<JoinHandle<()>>,
    exit_code: Option<i32>,
    containment: Containment,
}

impl PolicyExecutor {
    /// Spawn a command that remains contained while callers poll its lifecycle.
    pub fn spawn_managed(&self, command: &str, cwd: &Path) -> Result<ManagedProcess, String> {
        let mut process = command_with_env(command, cwd, &HashMap::new());
        process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (mut child, containment) = spawn_contained(&mut process, self.active_process_limit)
            .map_err(|error| format!("spawn failed: {error}"))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "managed process stdout was not piped".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "managed process stderr was not piped".to_string())?;
        let output = Arc::new(Mutex::new(OutputRingBuffer::default()));
        let sequence = Arc::new(AtomicU64::new(1));
        let readers = vec![
            spawn_reader(stdout, "stdout", output.clone(), sequence.clone()),
            spawn_reader(stderr, "stderr", output.clone(), sequence),
        ];
        Ok(ManagedProcess {
            child,
            stdin,
            output,
            readers,
            exit_code: None,
            containment,
        })
    }
}

impl ManagedProcess {
    pub async fn poll(&mut self, after: u64) -> Result<ProcessSnapshot, String> {
        if self.exit_code.is_none() {
            self.exit_code = self
                .child
                .try_wait()
                .map_err(|error| format!("process status failed: {error}"))?
                .map(|status| status.code().unwrap_or(1));
            if self.exit_code.is_some() {
                for reader in self.readers.drain(..) {
                    let _ = reader.await;
                }
            }
        }
        let (chunks, next_cursor) = self.output.lock().unwrap().snapshot_after(after);
        Ok(ProcessSnapshot {
            running: self.exit_code.is_none(),
            exit_code: self.exit_code,
            chunks,
            next_cursor,
        })
    }

    pub async fn write_stdin(&mut self, input: &str) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| "managed process stdin is closed".to_string())?;
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|error| format!("stdin write failed: {error}"))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("stdin flush failed: {error}"))
    }

    pub fn terminate(&mut self) {
        // Once the process has been reaped its id (and process group) may be
        // reused, so only signal the tree while it is still running.
        if self.exit_code.is_none() {
            self.containment.terminate();
        }
        let _ = self.child.start_kill();
        self.stdin = None;
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        self.terminate();
        for reader in &self.readers {
            reader.abort();
        }
    }
}

fn spawn_reader<R>(
    mut reader: R,
    stream: &'static str,
    output: Arc<Mutex<OutputRingBuffer<ProcessOutputChunk>>>,
    sequence: Arc<AtomicU64>,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut bytes = vec![0; 4096];
        let mut decoder = Utf8StreamDecoder::default();
        while let Ok(read) = reader.read(&mut bytes).await {
            if read == 0 {
                break;
            }
            let text = decoder.push(&bytes[..read]);
            if text.is_empty() {
                continue;
            }
            let chunk = ProcessOutputChunk {
                seq: sequence.fetch_add(1, Ordering::Relaxed),
                stream,
                text,
            };
            output.lock().unwrap().push(chunk);
        }
        let trailing = decoder.finish();
        if !trailing.is_empty() {
            output.lock().unwrap().push(ProcessOutputChunk {
                seq: sequence.fetch_add(1, Ordering::Relaxed),
                stream,
                text: trailing,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn managed_process_returns_incremental_output_and_exit() {
        let mut process = PolicyExecutor::new()
            .spawn_managed("echo managed_ok", &std::env::temp_dir())
            .unwrap();
        let mut snapshot = process.poll(0).await.unwrap();
        for _ in 0..50 {
            if !snapshot.running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            snapshot = process.poll(0).await.unwrap();
        }
        assert_eq!(snapshot.exit_code, Some(0));
        assert!(snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.text.contains("managed_ok")));
    }
}
