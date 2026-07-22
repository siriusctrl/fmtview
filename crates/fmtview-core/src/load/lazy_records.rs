use std::{cell::RefCell, fs::File, rc::Rc, time::Duration};

use anyhow::{Context, Result};

use crate::{
    input::InputSource,
    load::{
        ViewFile,
        lazy::{LazyBatch, LazyFile, LazyProducer},
        record_stream::{RecordRecoveryDiagnostic, RecoveringFormattedRecordReader},
    },
    transform::FormatOptions,
};

pub struct LazyTransformedRecordsFile {
    inner: LazyFile<RecordTransformProducer>,
    notices: Rc<RefCell<RecordRecoveryNotices>>,
}

impl LazyTransformedRecordsFile {
    pub fn new(source: &InputSource, options: FormatOptions) -> Result<Self> {
        let file = source.open()?;
        let label = source.label().to_owned();
        let notices = Rc::new(RefCell::new(RecordRecoveryNotices::default()));
        let len = file
            .metadata()
            .with_context(|| format!("failed to stat {}", source.label()))?
            .len();
        Ok(Self {
            inner: LazyFile::with_raw_source(
                label.clone(),
                len,
                RecordTransformProducer::new(label, file, options, Rc::clone(&notices)),
                Some(source.open()?),
            )?,
            notices,
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

    fn take_notice(&self) -> Option<String> {
        self.notices.borrow_mut().take_notice()
    }

    fn open_raw_record(&self, line: usize) -> Result<Option<Box<dyn ViewFile>>> {
        self.inner.open_raw_record(line)
    }

    fn supports_raw_records(&self) -> bool {
        true
    }
}

struct RecordTransformProducer {
    reader: RecoveringFormattedRecordReader,
    notices: Rc<RefCell<RecordRecoveryNotices>>,
}

impl RecordTransformProducer {
    fn new(
        label: String,
        file: File,
        options: FormatOptions,
        notices: Rc<RefCell<RecordRecoveryNotices>>,
    ) -> Self {
        Self {
            reader: RecoveringFormattedRecordReader::from_file(label, file, options),
            notices,
        }
    }
}

impl LazyProducer for RecordTransformProducer {
    fn produce(&mut self, _source_offset: u64) -> Result<LazyBatch> {
        let Some(record) = self.reader.read_record_bytes()? else {
            return Ok(LazyBatch::Complete);
        };
        if let Some(diagnostic) = record.diagnostic {
            self.notices.borrow_mut().push(diagnostic);
        }
        Ok(LazyBatch::Bytes {
            source_bytes: record.source_bytes,
            source_offset: record.source_offset,
            bytes: record.bytes,
        })
    }
}

#[derive(Default)]
struct RecordRecoveryNotices {
    failures: usize,
    first_record_number: Option<usize>,
}

impl RecordRecoveryNotices {
    fn push(&mut self, diagnostic: RecordRecoveryDiagnostic) {
        self.failures = self.failures.saturating_add(1);
        self.first_record_number = self.first_record_number.or(Some(diagnostic.record_number));
    }

    fn take_notice(&mut self) -> Option<String> {
        let failures = std::mem::take(&mut self.failures);
        let first_record_number = self.first_record_number.take();
        match (failures, first_record_number) {
            (0, _) => None,
            (1, Some(record_number)) => Some(format!(
                "record {record_number} failed JSON parse; showing raw record"
            )),
            (failures, Some(record_number)) => Some(format!(
                "{failures} JSONL records failed to parse starting at record {record_number}; showing raw records"
            )),
            (failures, None) => Some(format!(
                "{failures} JSONL records failed to parse; showing raw records"
            )),
        }
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
    fn malformed_jsonl_record_is_spooled_raw_and_later_records_continue() {
        let (_temp, source) = temp_source(b"{\"ok\":true}\n{\"broken\":\n{\"next\":true}\n");
        let options = FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        let lines = file.read_window(0, 20).unwrap();
        let notice = file.take_notice().unwrap();

        assert!(lines.iter().any(|line| line == "{\"broken\":"));
        assert!(lines.iter().any(|line| line.contains("\"next\"")));
        assert!(notice.contains("record 2 failed JSON parse"));
        assert!(file.take_notice().is_none());
    }

    #[test]
    fn malformed_jsonl_record_notices_are_aggregated() {
        let (_temp, source) = temp_source(b"{\"broken\":\n{\"also_broken\":\n{\"ok\":true}\n");
        let options = FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        file.preload(20, 3, Duration::from_secs(1)).unwrap();
        let notice = file.take_notice().unwrap();

        assert!(notice.contains("2 JSONL records failed"));
        assert!(notice.contains("record 1"));
    }

    #[test]
    fn lazy_view_collapses_large_data_uri_before_spooling_formatted_lines() {
        let mut input = Vec::with_capacity(1024 * 1024 + 64);
        input.extend_from_slice(br#"{"content":"data:image/png;base64,"#);
        input.extend(std::iter::repeat_n(b'A', 1024 * 1024));
        input.extend_from_slice(b"\"}\n");
        let (_temp, source) = temp_source(&input);
        let options = FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

        let lines = file.read_window(0, 8).unwrap();

        assert!(
            lines
                .iter()
                .any(|line| { line.contains("<media image/png; 786432 decoded bytes>") })
        );
        assert!(lines.iter().map(String::len).sum::<usize>() < 1024);
    }

    #[test]
    fn lazy_view_opens_the_exact_source_record_for_a_formatted_line() {
        let input = concat!(
            r#"{"role":"assistant","content":[{"type":"tool_call","arguments":"{\"cmd\":\"cargo  test\"}"}]}"#,
            "\n"
        )
        .as_bytes();
        let (_temp, source) = temp_source(input);
        let options = FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        };
        let file = LazyTransformedRecordsFile::new(&source, options).unwrap();
        let lines = file.read_window(0, 32).unwrap();
        let arguments_line = lines
            .iter()
            .position(|line| line.contains("arguments"))
            .unwrap();

        let raw = file.open_raw_record(arguments_line).unwrap().unwrap();
        let raw = raw.read_window(0, raw.line_count()).unwrap().concat();

        assert_eq!(raw.as_bytes(), input.strip_suffix(b"\n").unwrap());
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
