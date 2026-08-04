use std::{
    collections::{HashMap, HashSet},
    fs::File,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use csv::{ByteRecord, Position, Reader, ReaderBuilder};

use crate::{
    formats::delimited::{DelimitedDialect, resolve_dialect},
    input::InputSource,
    transform::FormatKind,
};

const DEFAULT_PRELOAD_RECORDS: usize = 128;

pub struct DelimitedDataset {
    source: InputSource,
    label: String,
    kind: FormatKind,
    dialect: DelimitedDialect,
    scanner: Reader<File>,
    headers: Vec<String>,
    positions: Vec<Position>,
    complete: bool,
    current_index: Option<usize>,
    current: ByteRecord,
}

pub fn open_delimited_dataset(source: InputSource, kind: FormatKind) -> Result<DelimitedDataset> {
    DelimitedDataset::open(source, kind)
}

impl DelimitedDataset {
    fn open(source: InputSource, kind: FormatKind) -> Result<Self> {
        let dialect = resolve_dialect(source.path(), kind)?;
        let file = source.open()?;
        let mut scanner = reader(file, dialect);
        let mut header = ByteRecord::new();
        let has_header = scanner
            .read_byte_record(&mut header)
            .with_context(|| format!("failed to read headers from {}", source.label()))?;
        let headers = if has_header {
            display_headers(&header)
        } else {
            Vec::new()
        };
        let mut dataset = Self {
            label: source.label().to_owned(),
            source,
            kind,
            dialect,
            scanner,
            headers,
            positions: Vec::new(),
            complete: !has_header,
            current_index: None,
            current: ByteRecord::new(),
        };
        if has_header {
            dataset.scan_one()?;
            if !dataset.positions.is_empty() {
                dataset.load_record(0)?;
            }
        }
        Ok(dataset)
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn kind(&self) -> FormatKind {
        self.kind
    }

    pub const fn dialect(&self) -> DelimitedDialect {
        self.dialect
    }

    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    pub fn record_count(&self) -> usize {
        self.positions.len()
    }

    pub const fn record_count_exact(&self) -> bool {
        self.complete
    }

    pub const fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_value(&self, field: usize) -> String {
        String::from_utf8_lossy(self.current_value_bytes(field)).into_owned()
    }

    pub fn current_value_bytes(&self, field: usize) -> &[u8] {
        self.current.get(field).unwrap_or_default()
    }

    pub fn move_record(&mut self, delta: isize) -> Result<bool> {
        let Some(current) = self.current_index else {
            return Ok(false);
        };
        let target = if delta >= 0 {
            current.saturating_add(delta as usize)
        } else {
            current.saturating_sub(delta.unsigned_abs())
        };
        self.set_record(target)
    }

    pub fn set_record(&mut self, index: usize) -> Result<bool> {
        while index >= self.positions.len() && !self.complete {
            if !self.scan_one()? {
                break;
            }
        }
        if index >= self.positions.len() || self.current_index == Some(index) {
            return Ok(false);
        }
        self.load_record(index)?;
        Ok(true)
    }

    pub fn preload(&mut self, budget: Duration) -> Result<bool> {
        if self.complete {
            return Ok(false);
        }
        let started = Instant::now();
        let before = self.positions.len();
        for _ in 0..DEFAULT_PRELOAD_RECORDS {
            if started.elapsed() >= budget || !self.scan_one()? {
                break;
            }
        }
        Ok(self.positions.len() != before || self.complete)
    }

    fn scan_one(&mut self) -> Result<bool> {
        if self.complete {
            return Ok(false);
        }
        let mut record = ByteRecord::new();
        if !self
            .scanner
            .read_byte_record(&mut record)
            .with_context(|| format!("failed to index record in {}", self.label))?
        {
            self.complete = true;
            return Ok(false);
        }
        let position = record
            .position()
            .cloned()
            .context("CSV reader did not report a record position")?;
        self.positions.push(position);
        self.extend_headers(record.len());
        Ok(true)
    }

    fn load_record(&mut self, index: usize) -> Result<()> {
        let position = self
            .positions
            .get(index)
            .cloned()
            .context("record position was not indexed")?;
        let file = self.source.open()?;
        let mut reader = reader(file, self.dialect);
        reader
            .seek(position)
            .with_context(|| format!("failed to seek record {}", index + 1))?;
        self.current.clear();
        if !reader
            .read_byte_record(&mut self.current)
            .with_context(|| format!("failed to read record {}", index + 1))?
        {
            anyhow::bail!("record {} ended before its indexed position", index + 1);
        }
        self.extend_headers(self.current.len());
        self.current_index = Some(index);
        Ok(())
    }

    fn extend_headers(&mut self, field_count: usize) {
        while self.headers.len() < field_count {
            self.headers
                .push(format!("field_{}", self.headers.len() + 1));
        }
    }
}

fn reader(file: File, dialect: DelimitedDialect) -> Reader<File> {
    ReaderBuilder::new()
        .delimiter(dialect.delimiter())
        .has_headers(false)
        .flexible(true)
        .from_reader(file)
}

fn display_headers(record: &ByteRecord) -> Vec<String> {
    let mut headers = Vec::with_capacity(record.len());
    let mut used = HashSet::with_capacity(record.len());
    let mut next_suffix = HashMap::with_capacity(record.len());
    for (index, raw) in record.iter().enumerate() {
        let decoded = String::from_utf8_lossy(raw).into_owned();
        let base = if decoded.trim().is_empty() {
            format!("field_{}", index + 1)
        } else {
            decoded
        };
        let suffix = next_suffix.entry(base.clone()).or_insert(1_usize);
        let header = loop {
            let candidate = if *suffix == 1 {
                base.clone()
            } else {
                format!("{base} [{suffix}]")
            };
            *suffix += 1;
            if used.insert(candidate.clone()) {
                break candidate;
            }
        };
        headers.push(header);
    }
    headers
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::Builder;

    use super::*;

    fn source(suffix: &str, body: &[u8]) -> (tempfile::NamedTempFile, InputSource) {
        let mut temp = Builder::new().suffix(suffix).tempfile().unwrap();
        temp.write_all(body).unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_path(temp.path(), "sample").unwrap();
        (temp, source)
    }

    #[test]
    fn indexes_quoted_multiline_records_and_seeks_back() {
        let (_temp, source) = source(
            ".csv",
            b"id,payload\n1,\"line one\nline two\"\n2,\"{\"\"ok\"\":true}\"\n",
        );
        let mut dataset = open_delimited_dataset(source, FormatKind::Csv).unwrap();

        assert_eq!(dataset.headers(), &["id", "payload"]);
        assert_eq!(dataset.current_value(1), "line one\nline two");
        assert!(dataset.move_record(1).unwrap());
        assert_eq!(dataset.current_value(0), "2");
        assert_eq!(dataset.current_value(1), r#"{"ok":true}"#);
        assert!(dataset.move_record(-1).unwrap());
        assert_eq!(dataset.current_value(1), "line one\nline two");
    }

    #[test]
    fn sniffs_pipe_delimited_xsv() {
        let (_temp, source) = source(".xsv", b"id|status|payload\n1|ok|hello\n");
        let dataset = open_delimited_dataset(source, FormatKind::Xsv).unwrap();
        assert_eq!(dataset.dialect().delimiter(), b'|');
        assert_eq!(dataset.headers(), &["id", "status", "payload"]);
        assert_eq!(dataset.current_value(2), "hello");
    }

    #[test]
    fn duplicate_headers_receive_stable_unique_names() {
        let (_temp, source) = source(".csv", b"a,a,a,a [2],a,,\n1,2,3,4,5,6,7\n");
        let dataset = open_delimited_dataset(source, FormatKind::Csv).unwrap();

        assert_eq!(
            dataset.headers(),
            &[
                "a",
                "a [2]",
                "a [3]",
                "a [2] [2]",
                "a [4]",
                "field_6",
                "field_7"
            ]
        );
    }

    #[test]
    fn dataset_keeps_temporary_input_open_after_source_drops() {
        let mut temp = Builder::new().suffix(".csv").tempfile().unwrap();
        temp.write_all(b"id,value\n1,first\n2,second\n").unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_temp(temp, "sample");
        let mut dataset = open_delimited_dataset(source, FormatKind::Csv).unwrap();

        assert!(dataset.move_record(1).unwrap());
        assert_eq!(dataset.current_value(1), "second");
    }
}
