use std::{fs::File, time::Duration};

use anyhow::{Context, Result};

use crate::{
    input::InputSource,
    load::{
        ViewFile,
        lazy::{LazyBatch, LazyFile, LazyProducer},
        record_stream::FormattedRecordReader,
    },
    transform::FormatOptions,
};

pub struct LazyTransformedRecordsFile {
    inner: LazyFile<RecordTransformProducer>,
}

impl LazyTransformedRecordsFile {
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
    pub(crate) fn loaded_record_count(&self) -> usize {
        self.inner.produced_unit_count()
    }

    #[cfg(test)]
    pub(crate) fn indexed_line_count(&self) -> usize {
        self.inner.indexed_line_count()
    }
}

impl ViewFile for LazyTransformedRecordsFile {
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
    reader: FormattedRecordReader,
}

impl RecordTransformProducer {
    fn new(label: String, file: File, options: FormatOptions) -> Self {
        Self {
            reader: FormattedRecordReader::from_file(label, file, options),
        }
    }
}

impl LazyProducer for RecordTransformProducer {
    fn produce(&mut self, _source_offset: u64) -> Result<LazyBatch> {
        let Some(record) = self.reader.read_record_bytes()? else {
            return Ok(LazyBatch::Complete);
        };
        Ok(LazyBatch::Bytes {
            source_bytes: record.source_bytes,
            source_offset: record.source_offset,
            bytes: record.bytes,
        })
    }
}

#[cfg(test)]
mod tests;
