//! Managed subprocess lifecycle using the same command boundary as PolicyExecutor.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::task::JoinHandle;

use crate::executor::{command_with_env, PolicyExecutor};
use crate::text_encoding::Utf8StreamDecoder;

const MAX_BUFFERED_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutputChunk {
    pub seq: u64,
    pub stream: &'static str,
    pub text: String,
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
    output: Arc<Mutex<OutputBuffer>>,
    readers: Vec<JoinHandle<()>>,
    exit_code: Option<i32>,
    #[cfg(windows)]
    job: Option<crate::executor::win_job::Job>,
}

#[derive(Default)]
struct OutputBuffer {
    chunks: VecDeque<ProcessOutputChunk>,
    bytes: usize,
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
        let mut child = process
            .spawn()
            .map_err(|error| format!("spawn failed: {error}"))?;
        #[cfg(windows)]
        let job = child
            .id()
            .and_then(|pid| crate::executor::win_job::Job::contain(pid, self.active_process_limit));
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "managed process stdout was not piped".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "managed process stderr was not piped".to_string())?;
        let output = Arc::new(Mutex::new(OutputBuffer::default()));
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
            #[cfg(windows)]
            job,
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
        let output = self.output.lock().unwrap();
        let chunks = output
            .chunks
            .iter()
            .filter(|chunk| chunk.seq > after)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = output.chunks.back().map_or(after, |chunk| chunk.seq);
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
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
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
    output: Arc<Mutex<OutputBuffer>>,
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
            push_chunk(&mut output.lock().unwrap(), chunk);
        }
        let trailing = decoder.finish();
        if !trailing.is_empty() {
            push_chunk(
                &mut output.lock().unwrap(),
                ProcessOutputChunk {
                    seq: sequence.fetch_add(1, Ordering::Relaxed),
                    stream,
                    text: trailing,
                },
            );
        }
    })
}

fn push_chunk(output: &mut OutputBuffer, chunk: ProcessOutputChunk) {
    output.bytes += chunk.text.len();
    output.chunks.push_back(chunk);
    while output.bytes > MAX_BUFFERED_BYTES {
        let Some(removed) = output.chunks.pop_front() else {
            break;
        };
        output.bytes = output.bytes.saturating_sub(removed.text.len());
    }
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
