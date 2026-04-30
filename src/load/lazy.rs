use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use super::{ViewFile, strip_line_end};

pub(crate) trait LazyProducer {
    fn produce(&mut self, source_offset: u64) -> Result<LazyBatch>;
}

pub(crate) enum LazyBatch {
    Complete,
    Lines {
        source_bytes: u64,
        lines: Vec<LazyLine>,
    },
}

pub(crate) struct LazyLine {
    pub(crate) source_offset: u64,
    pub(crate) text: String,
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
        let mut reader = BufReader::new(file);
        let mut lines = Vec::with_capacity(end - start);
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
        let LazyBatch::Lines {
            source_bytes,
            lines,
        } = batch
        else {
            self.complete = true;
            return Ok(false);
        };

        if source_bytes == 0 && lines.is_empty() {
            bail!("lazy producer returned no progress");
        }

        self.source_offset = self.source_offset.saturating_add(source_bytes);
        self.units_produced = self.units_produced.saturating_add(1);
        for line in lines {
            self.line_offsets.push(self.spool_len);
            self.source_line_offsets.push(line.source_offset);
            self.spool
                .as_file_mut()
                .write_all(line.text.as_bytes())
                .context("failed to write lazy load spool")?;
            self.spool
                .as_file_mut()
                .write_all(b"\n")
                .context("failed to write lazy load spool")?;
            self.spool_len = self
                .spool_len
                .saturating_add(line.text.len() as u64)
                .saturating_add(1);
        }
        Ok(true)
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
    fn lazy_runtime_spools_and_indexes_producer_lines() {
        let file = LazyFile::new(
            "test".to_owned(),
            12,
            TestProducer::with_batches([
                LazyBatch::Lines {
                    source_bytes: 5,
                    lines: vec![
                        LazyLine {
                            source_offset: 0,
                            text: "one".to_owned(),
                        },
                        LazyLine {
                            source_offset: 0,
                            text: "two".to_owned(),
                        },
                    ],
                },
                LazyBatch::Lines {
                    source_bytes: 7,
                    lines: vec![LazyLine {
                        source_offset: 5,
                        text: "three".to_owned(),
                    }],
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
                LazyBatch::Lines {
                    source_bytes: 3,
                    lines: vec![LazyLine {
                        source_offset: 0,
                        text: "a".to_owned(),
                    }],
                },
                LazyBatch::Lines {
                    source_bytes: 3,
                    lines: vec![LazyLine {
                        source_offset: 3,
                        text: "b".to_owned(),
                    }],
                },
                LazyBatch::Lines {
                    source_bytes: 3,
                    lines: vec![LazyLine {
                        source_offset: 6,
                        text: "c".to_owned(),
                    }],
                },
            ]),
        )
        .unwrap();

        assert!(file.preload(10, 2, Duration::from_secs(1)).unwrap());
        assert_eq!(file.produced_unit_count(), 2);
        assert_eq!(file.read_window(0, 10).unwrap(), vec!["a", "b", "c"]);
        assert_eq!(file.produced_unit_count(), 3);
    }
}
