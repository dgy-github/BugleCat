//! Cross-platform raw PTY lifecycle with Windows Job Object containment.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::PolicyExecutor;

const MAX_BUFFERED_BYTES: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputChunk {
    pub seq: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySnapshot {
    pub running: bool,
    pub exit_code: Option<u32>,
    pub chunks: Vec<PtyOutputChunk>,
    pub next_cursor: u64,
}

/// An unrestricted interactive shell attached to a real PTY/ConPTY.
pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
    output: Arc<Mutex<OutputBuffer>>,
    exit_code: Option<u32>,
    #[cfg(windows)]
    job: Option<crate::executor::win_job::Job>,
}

#[derive(Default)]
struct OutputBuffer {
    chunks: VecDeque<PtyOutputChunk>,
    bytes: usize,
}

impl PolicyExecutor {
    /// Start the platform shell in a raw PTY under the same process cap.
    pub fn spawn_pty(&self, cwd: &Path, rows: u16, cols: u16) -> Result<PtyProcess, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("PTY open failed: {error}"))?;
        let mut command = shell_command();
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("PTY shell spawn failed: {error}"))?;
        #[cfg(windows)]
        let job = child
            .process_id()
            .and_then(|pid| crate::executor::win_job::Job::contain(pid, self.active_process_limit));
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("PTY reader failed: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("PTY writer failed: {error}"))?;
        let writer = Arc::new(Mutex::new(writer));
        let output = Arc::new(Mutex::new(OutputBuffer::default()));
        spawn_reader(reader, output.clone(), Arc::downgrade(&writer));
        Ok(PtyProcess {
            master: pair.master,
            child,
            writer: Some(writer),
            output,
            exit_code: None,
            #[cfg(windows)]
            job,
        })
    }
}

impl PtyProcess {
    /// Write exact bytes to the PTY. Callers own the approval boundary.
    pub fn write(&mut self, input: &str) -> Result<(), String> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| "PTY stdin is closed".to_string())?;
        let mut writer = writer.lock().unwrap();
        writer
            .write_all(input.as_bytes())
            .map_err(|error| format!("PTY write failed: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("PTY flush failed: {error}"))
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("PTY resize failed: {error}"))
    }

    pub fn poll(&mut self, after: u64) -> Result<PtySnapshot, String> {
        if self.exit_code.is_none() {
            self.exit_code = self
                .child
                .try_wait()
                .map_err(|error| format!("PTY status failed: {error}"))?
                .map(|status| status.exit_code());
        }
        let output = self.output.lock().unwrap();
        let chunks = output
            .chunks
            .iter()
            .filter(|chunk| chunk.seq > after)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = output.chunks.back().map_or(after, |chunk| chunk.seq);
        Ok(PtySnapshot {
            running: self.exit_code.is_none(),
            exit_code: self.exit_code,
            chunks,
            next_cursor,
        })
    }

    pub fn terminate(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        let _ = self.child.kill();
        self.writer = None;
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn shell_command() -> CommandBuilder {
    #[cfg(windows)]
    {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut command = CommandBuilder::new(shell);
        command.arg("/Q");
        command
    }
    #[cfg(not(windows))]
    {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()))
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    output: Arc<Mutex<OutputBuffer>>,
    writer: Weak<Mutex<Box<dyn Write + Send>>>,
) {
    std::thread::Builder::new()
        .name("ncx-pty-reader".to_string())
        .spawn(move || {
            let sequence = AtomicU64::new(1);
            let mut bytes = vec![0; 4096];
            while let Ok(read) = reader.read(&mut bytes) {
                if read == 0 {
                    break;
                }
                let chunk = PtyOutputChunk {
                    seq: sequence.fetch_add(1, Ordering::Relaxed),
                    text: String::from_utf8_lossy(&bytes[..read]).to_string(),
                };
                // Windows cmd queries the terminal cursor before presenting its
                // first prompt. A PTY transport must answer this ANSI DSR or the
                // shell waits forever before consuming stdin.
                if chunk.text.contains("\x1b[6n") {
                    if let Some(writer) = writer.upgrade() {
                        let mut writer = writer.lock().unwrap();
                        let _ = writer.write_all(b"\x1b[1;1R");
                        let _ = writer.flush();
                    }
                }
                push_chunk(&mut output.lock().unwrap(), chunk);
            }
        })
        .expect("PTY reader thread starts");
}

fn push_chunk(output: &mut OutputBuffer, chunk: PtyOutputChunk) {
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
    use std::time::Duration;

    use super::*;

    #[test]
    fn raw_pty_accepts_stdin_and_returns_output() {
        let mut process = PolicyExecutor::new()
            .spawn_pty(&std::env::temp_dir(), 24, 100)
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        process.write("echo pty_ok\r\n").unwrap();
        let mut snapshot = process.poll(0).unwrap();
        for _ in 0..100 {
            if snapshot
                .chunks
                .iter()
                .any(|chunk| chunk.text.contains("pty_ok"))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
            snapshot = process.poll(0).unwrap();
        }
        let saw_output = snapshot
            .chunks
            .iter()
            .any(|chunk| chunk.text.contains("pty_ok"));
        if !saw_output {
            eprintln!("PTY snapshot without expected output: {snapshot:?}");
            process.terminate();
        }
        assert!(saw_output);
        process.write("exit\r\n").unwrap();
        for _ in 0..100 {
            if !process.poll(snapshot.next_cursor).unwrap().running {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("PTY shell did not exit");
    }
}
