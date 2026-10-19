//! Bounded incremental output ring buffer shared by managed processes and PTY sessions.

use std::collections::VecDeque;

const MAX_BUFFERED_BYTES: usize = 1_000_000;

/// Output chunk carrying a monotonic cursor sequence number and UTF-8 text payload.
pub(crate) trait SequencedChunk: Clone {
    fn seq(&self) -> u64;
    fn text_len(&self) -> usize;
}

/// Bounded ring buffer that evicts oldest chunks when total buffered bytes exceed 1 MB.
pub(crate) struct OutputRingBuffer<T> {
    chunks: VecDeque<T>,
    bytes: usize,
}

impl<T> Default for OutputRingBuffer<T> {
    fn default() -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
        }
    }
}

impl<T: SequencedChunk> OutputRingBuffer<T> {
    pub(crate) fn push(&mut self, chunk: T) {
        self.bytes += chunk.text_len();
        self.chunks.push_back(chunk);
        while self.bytes > MAX_BUFFERED_BYTES {
            let Some(removed) = self.chunks.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.text_len());
        }
    }

    pub(crate) fn snapshot_after(&self, after: u64) -> (Vec<T>, u64) {
        let chunks = self
            .chunks
            .iter()
            .filter(|chunk| chunk.seq() > after)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = self.chunks.back().map_or(after, SequencedChunk::seq);
        (chunks, next_cursor)
    }
}
