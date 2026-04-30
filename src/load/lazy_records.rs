use std::{
    fs::File,
    io::{BufRead, BufReader},
    time::Duration,
};

use anyhow::{Context, Result};

use crate::{
    input::InputSource,
    load::{
        ViewFile,
        lazy::{LazyBatch, LazyFile, LazyProducer},
    },
    transform::{self, FormatOptions},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPlan {
    LazyTransformedRecords,
    EagerTransformedDocument,
    EagerIndexedSource,
}

pub struct LazyTransformedFile {
    inner: LazyFile<RecordTransformProducer>,
}

impl LazyTransformedFile {
    pub fn new(source: &InputSource, options: FormatOptions) -> Result<Self> {
        let file = source.open()?;
        let label = source.label().to_owned();
        let len = file
            .metadata()
            .with_context(|| format!("failed to stat {}", source.label()))?
            .len();
        Ok(Self {
            inner: LazyFile::new(
                label.clone(),
                len,
                RecordTransformProducer::new(label, file, options),
            )?,
        })
    }

    #[cfg(test)]
    fn loaded_record_count(&self) -> usize {
        self.inner.produced_unit_count()
    }

    #[cfg(test)]
    fn indexed_line_count(&self) -> usize {
        self.inner.indexed_line_count()
    }
}

impl ViewFile for LazyTransformedFile {
    fn label(&self) -> &str {
        self.inner.label()
    }

    fn line_count(&self) -> usize {
        self.inner.line_count()
    }

    fn line_count_exact(&self) -> bool {
        self.inner.line_count_exact()
    }

    fn byte_len(&self) -> u64 {
        self.inner.byte_len()
    }

    fn byte_offset_for_line(&self, line: usize) -> u64 {
        self.inner.byte_offset_for_line(line)
    }

    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
        self.inner.read_window(start, count)
    }

    fn preload(&self, max_lines: usize, max_records: usize, budget: Duration) -> Result<bool> {
        self.inner.preload(max_lines, max_records, budget)
    }
}

struct RecordTransformProducer {
    label: String,
    reader: BufReader<File>,
    raw_line: Vec<u8>,
    options: FormatOptions,
}

impl RecordTransformProducer {
    fn new(label: String, file: File, options: FormatOptions) -> Self {
        Self {
            label,
            reader: BufReader::new(file),
            raw_line: Vec::with_capacity(8192),
            options,
        }
    }
}

impl LazyProducer for RecordTransformProducer {
    fn produce(&mut self, source_offset: u64) -> Result<LazyBatch> {
        let record_start = source_offset;
        let mut raw_line = std::mem::take(&mut self.raw_line);
        raw_line.clear();
        let read = self
            .reader
            .read_until(b'\n', &mut raw_line)
            .with_context(|| format!("failed to read {}", self.label))?;
        if read == 0 {
            self.raw_line = raw_line;
            return Ok(LazyBatch::Complete);
        }

        let rendered = transform::format_record_bytes(&raw_line, self.options)?;
        self.raw_line = raw_line;
        Ok(LazyBatch::Bytes {
            source_bytes: read as u64,
            source_offset: record_start,
            bytes: rendered,
        })
    }
}

#[cfg(test)]
mod tests;
