use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use memchr::memchr_iter;
use tempfile::NamedTempFile;

use super::{ViewFile, strip_line_end};

const DIRECT_READ_LINE_BYTES: u64 = 64 * 1024;

pub(crate) trait LazyProducer {
    fn produce(&mut self, source_offset: u64) -> Result<LazyBatch>;
}

pub(crate) enum LazyBatch {
    Complete,
    Bytes {
        source_bytes: u64,
        source_offset: u64,
        bytes: Vec<u8>,
    },
}

pub(crate) struct LazyFile<P> {
    label: String,
    len: u64,
    state: RefCell<LazyState<P>>,
}

impl<P: LazyProducer> LazyFile<P> {
    pub(crate) fn new(label: String, len: u64, producer: P) -> Result<Self> {
        Ok(Self {
            label,
            len,
            state: RefCell::new(LazyState {
                producer,
                spool: NamedTempFile::new().context("failed to create lazy load spool")?,
                spool_len: 0,
                source_offset: 0,
                line_offsets: Vec::new(),
                source_line_offsets: Vec::new(),
                complete: len == 0,
                units_produced: 0,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn produced_unit_count(&self) -> usize {
        self.state.borrow().units_produced
    }

    #[cfg(test)]
    pub(crate) fn indexed_line_count(&self) -> usize {
        self.state.borrow().line_offsets.len()
    }

    fn ensure_lines(&self, needed: usize) -> Result<()> {
        let mut state = self.state.borrow_mut();
        while state.line_offsets.len() < needed && !state.complete {
            if !state.produce_next_batch()? {
                break;
            }
        }
        Ok(())
    }
}

impl<P: LazyProducer> ViewFile for LazyFile<P> {
    fn label(&self) -> &str {
        &self.label
    }

    fn line_count(&self) -> usize {
        let state = self.state.borrow();
        if state.complete {
            state.line_offsets.len()
        } else {
            state.line_offsets.len().saturating_add(1).max(1)
        }
    }

    fn line_count_exact(&self) -> bool {
        self.state.borrow().complete
    }

    fn byte_len(&self) -> u64 {
        self.len
    }

    fn byte_offset_for_line(&self, line: usize) -> u64 {
        let state = self.state.borrow();
        state
            .source_line_offsets
            .get(line)
            .copied()
            .unwrap_or(state.source_offset)
            .min(self.len)
    }

    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
        if count == 0 {
            return Ok(Vec::new());
        }

        self.ensure_lines(start.saturating_add(count))?;
        let state = self.state.borrow();
        if start >= state.line_offsets.len() {
            return Ok(Vec::new());
        }

        let end = start.saturating_add(count).min(state.line_offsets.len());
        let mut file = File::open(state.spool.path()).context("failed to open lazy load spool")?;
        file.seek(SeekFrom::Start(state.line_offsets[start]))
            .context("failed to seek lazy load spool")?;
        let mut lines = Vec::with_capacity(end - start);

        if (start..end).any(|line_index| state.line_span(line_index) > DIRECT_READ_LINE_BYTES) {
            for line_index in start..end {
                let mut line = vec![0_u8; state.line_span(line_index) as usize];
                file.read_exact(&mut line)
                    .context("failed to read lazy load spool")?;
                strip_byte_line_end(&mut line);
                lines.push(String::from_utf8(line).context("lazy load spool was not valid UTF-8")?);
            }
        } else {
            let mut reader = BufReader::new(file);
            for _ in start..end {
                let mut line = String::new();
                let read = reader
                    .read_line(&mut line)
                    .context("failed to read lazy load spool")?;
                if read == 0 {
                    break;
                }
                strip_line_end(&mut line);
                lines.push(line);
            }
        }
        Ok(lines)
    }

    fn preload(&self, max_lines: usize, max_records: usize, budget: Duration) -> Result<bool> {
        if max_lines == 0 || max_records == 0 || budget.is_zero() {
            return Ok(false);
        }

        let started = Instant::now();
        let mut state = self.state.borrow_mut();
        if state.complete {
            return Ok(false);
        }

        let start_lines = state.line_offsets.len();
        let mut records = 0_usize;
        while !state.complete
            && records < max_records
            && state.line_offsets.len().saturating_sub(start_lines) < max_lines
            && started.elapsed() < budget
        {
            if !state.produce_next_batch()? {
                break;
            }
            records += 1;
        }

        Ok(state.line_offsets.len() != start_lines || state.complete)
    }
}

struct LazyState<P> {
    producer: P,
    spool: NamedTempFile,
    spool_len: u64,
    source_offset: u64,
    line_offsets: Vec<u64>,
    source_line_offsets: Vec<u64>,
    complete: bool,
    units_produced: usize,
}

impl<P: LazyProducer> LazyState<P> {
    fn produce_next_batch(&mut self) -> Result<bool> {
        let batch = self.producer.produce(self.source_offset)?;
        let source_bytes = match batch {
            LazyBatch::Complete => {
                self.complete = true;
                return Ok(false);
            }
            LazyBatch::Bytes {
                source_bytes,
                source_offset,
                bytes,
            } => {
                if source_bytes == 0 && bytes.is_empty() {
                    bail!("lazy producer returned no progress");
                }
                self.append_bytes(source_offset, &bytes)?;
                source_bytes
            }
        };

        self.source_offset = self.source_offset.saturating_add(source_bytes);
        self.units_produced = self.units_produced.saturating_add(1);
        Ok(true)
    }

    fn append_bytes(&mut self, source_offset: u64, bytes: &[u8]) -> Result<()> {
        self.line_offsets.push(self.spool_len);
        self.source_line_offsets.push(source_offset);
        for index in memchr_iter(b'\n', bytes) {
            if index + 1 < bytes.len() {
                self.line_offsets.push(self.spool_len + index as u64 + 1);
                self.source_line_offsets.push(source_offset);
            }
        }
        self.spool
            .as_file_mut()
            .write_all(bytes)
            .context("failed to write lazy load spool")?;
        self.spool
            .as_file_mut()
            .write_all(b"\n")
            .context("failed to write lazy load spool")?;
        self.spool_len = self
            .spool_len
            .saturating_add(bytes.len() as u64)
            .saturating_add(1);
        Ok(())
    }

    fn line_span(&self, line_index: usize) -> u64 {
        let line_start = self.line_offsets[line_index];
        let line_end = self
            .line_offsets
            .get(line_index + 1)
            .copied()
            .unwrap_or(self.spool_len);
        line_end.saturating_sub(line_start)
    }
}

fn strip_byte_line_end(line: &mut Vec<u8>) {
    if line.ends_with(b"\n") {
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, time::Duration};

    use super::*;

    #[derive(Default)]
    struct TestProducer {
        batches: VecDeque<LazyBatch>,
    }

    impl TestProducer {
        fn with_batches(batches: impl IntoIterator<Item = LazyBatch>) -> Self {
            Self {
                batches: batches.into_iter().collect(),
            }
        }
    }

    impl LazyProducer for TestProducer {
        fn produce(&mut self, _source_offset: u64) -> Result<LazyBatch> {
            Ok(self.batches.pop_front().unwrap_or(LazyBatch::Complete))
        }
    }

    #[test]
    fn lazy_runtime_spools_and_indexes_producer_bytes() {
        let file = LazyFile::new(
            "test".to_owned(),
            12,
            TestProducer::with_batches([
                LazyBatch::Bytes {
                    source_bytes: 5,
                    source_offset: 0,
                    bytes: b"one\ntwo".to_vec(),
                },
                LazyBatch::Bytes {
                    source_bytes: 7,
                    source_offset: 5,
                    bytes: b"three".to_vec(),
                },
            ]),
        )
        .unwrap();

        assert_eq!(file.line_count(), 1);
        assert!(!file.line_count_exact());
        assert_eq!(file.read_window(1, 2).unwrap(), vec!["two", "three"]);
        assert_eq!(file.line_count(), 4);
        assert_eq!(file.byte_offset_for_line(0), 0);
        assert_eq!(file.byte_offset_for_line(2), 5);
        assert_eq!(file.byte_offset_for_line(99), 12);
        assert_eq!(file.produced_unit_count(), 2);
    }

    #[test]
    fn lazy_runtime_preload_uses_line_record_and_budget_limits() {
        let file = LazyFile::new(
            "test".to_owned(),
            9,
            TestProducer::with_batches([
                LazyBatch::Bytes {
                    source_bytes: 3,
                    source_offset: 0,
                    bytes: b"a".to_vec(),
                },
                LazyBatch::Bytes {
                    source_bytes: 3,
                    source_offset: 3,
                    bytes: b"b".to_vec(),
                },
                LazyBatch::Bytes {
                    source_bytes: 3,
                    source_offset: 6,
                    bytes: b"c".to_vec(),
                },
            ]),
        )
        .unwrap();

        assert!(file.preload(10, 2, Duration::from_secs(1)).unwrap());
        assert_eq!(file.produced_unit_count(), 2);
        assert_eq!(file.read_window(0, 10).unwrap(), vec!["a", "b", "c"]);
        assert_eq!(file.produced_unit_count(), 3);
    }

    #[test]
    fn lazy_runtime_handles_empty_producer_bytes_as_empty_line() {
        let file = LazyFile::new(
            "test".to_owned(),
            9,
            TestProducer::with_batches([
                LazyBatch::Bytes {
                    source_bytes: 6,
                    source_offset: 0,
                    bytes: b"a\nb\nc".to_vec(),
                },
                LazyBatch::Bytes {
                    source_bytes: 3,
                    source_offset: 6,
                    bytes: Vec::new(),
                },
            ]),
        )
        .unwrap();

        assert_eq!(file.read_window(0, 10).unwrap(), vec!["a", "b", "c", ""]);
        assert_eq!(file.byte_offset_for_line(2), 0);
        assert_eq!(file.byte_offset_for_line(3), 6);
        assert_eq!(file.produced_unit_count(), 2);
    }

    #[test]
    fn lazy_runtime_reads_large_spooled_lines_by_offset() {
        let large = "x".repeat(DIRECT_READ_LINE_BYTES as usize + 1);
        let bytes = format!("head\n{large}\ntail").into_bytes();
        let file = LazyFile::new(
            "test".to_owned(),
            bytes.len() as u64,
            TestProducer::with_batches([LazyBatch::Bytes {
                source_bytes: bytes.len() as u64,
                source_offset: 0,
                bytes,
            }]),
        )
        .unwrap();

        assert_eq!(
            file.read_window(0, 3).unwrap(),
            vec!["head".to_owned(), large, "tail".to_owned()]
        );
    }
}
