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
mod tests {
    use std::{io::Write, time::Duration};

    use tempfile::NamedTempFile;

    use crate::transform::{FormatKind, FormatOptions};

    use super::*;

    fn temp_source(contents: &[u8]) -> (NamedTempFile, InputSource) {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(contents).unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        (temp, source)
    }

    #[test]
    fn lazy_load_reads_only_records_needed_for_first_window() {
        let mut data = Vec::new();
        for index in 0..1000 {
            writeln!(
                data,
                "{{\"index\":{index},\"payload\":{{\"name\":\"item\",\"ok\":true}}}}"
            )
            .unwrap();
        }
        let (_temp, source) = temp_source(&data);
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        let lines = file.read_window(0, 12).unwrap();

        assert_eq!(lines[0], "{");
        assert!(lines.iter().any(|line| line.contains("\"payload\"")));
        assert!(
            file.loaded_record_count() < 4,
            "first window should not scan all input records"
        );
    }

    #[test]
    fn lazy_load_idle_preload_advances_known_line_count() {
        let mut data = Vec::new();
        for index in 0..10 {
            writeln!(data, "{{\"index\":{index},\"payload\":{{\"ok\":true}}}}").unwrap();
        }
        let (_temp, source) = temp_source(&data);
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        file.read_window(0, 4).unwrap();
        let before = file.line_count();
        let changed = file.preload(20, 2, Duration::from_secs(1)).unwrap();
        let after = file.line_count();

        assert!(changed);
        assert!(after > before);
        assert!(!file.line_count_exact());
    }

    #[test]
    fn explicit_jsonl_errors_on_malformed_record() {
        let (_temp, source) = temp_source(b"{\"ok\":true}\n{\"broken\":\n");
        let options = FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        let error = file.read_window(0, 20).unwrap_err();

        assert!(error.to_string().contains("failed to parse JSON record"));
    }

    #[test]
    fn lazy_load_reads_spooled_lines_after_preload() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        assert!(file.preload(20, 3, Duration::from_secs(1)).unwrap());
        assert!(file.indexed_line_count() > 6);
        assert!(file.read_window(0, 1).unwrap()[0].contains('{'));
        assert!(
            file.read_window(file.indexed_line_count().saturating_sub(2), 2)
                .unwrap()
                .iter()
                .any(|line| line.contains("\"c\""))
        );
    }
}
