use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::{Context, Result};

use crate::{
    input::InputSource,
    transform::{self, FormatOptions},
};

pub(crate) struct RawRecord<'a> {
    pub(crate) source_offset: u64,
    pub(crate) source_bytes: u64,
    pub(crate) bytes: &'a [u8],
}

pub(crate) struct RawRecordReader {
    label: String,
    reader: BufReader<File>,
    line: Vec<u8>,
    source_offset: u64,
}

impl RawRecordReader {
    pub(crate) fn new(source: &InputSource) -> Result<Self> {
        Ok(Self::from_file(source.label().to_owned(), source.open()?))
    }

    pub(crate) fn from_file(label: String, file: File) -> Self {
        Self {
            label,
            reader: BufReader::new(file),
            line: Vec::with_capacity(8192),
            source_offset: 0,
        }
    }

    pub(crate) fn read_record(&mut self) -> Result<Option<RawRecord<'_>>> {
        self.line.clear();
        let read = self
            .reader
            .read_until(b'\n', &mut self.line)
            .with_context(|| format!("failed to read {}", self.label))?;
        if read == 0 {
            return Ok(None);
        }

        let source_offset = self.source_offset;
        self.source_offset = self.source_offset.saturating_add(read as u64);
        Ok(Some(RawRecord {
            source_offset,
            source_bytes: read as u64,
            bytes: &self.line,
        }))
    }
}

pub(crate) struct FormattedRecordReader {
    raw: RawRecordReader,
    options: FormatOptions,
    pending: VecDeque<FormattedRecord>,
}

impl FormattedRecordReader {
    pub(crate) fn new(source: &InputSource, options: FormatOptions) -> Result<Self> {
        Ok(Self {
            raw: RawRecordReader::new(source)?,
            options,
            pending: VecDeque::new(),
        })
    }

    pub(crate) fn from_file(label: String, file: File, options: FormatOptions) -> Self {
        Self {
            raw: RawRecordReader::from_file(label, file),
            options,
            pending: VecDeque::new(),
        }
    }

    pub(crate) fn read_record_bytes(&mut self) -> Result<Option<FormattedRecordBytes>> {
        let Some(raw) = self.raw.read_record()? else {
            return Ok(None);
        };
        Ok(Some(FormattedRecordBytes {
            source_offset: raw.source_offset,
            source_bytes: raw.source_bytes,
            bytes: transform::format_record_bytes(raw.bytes, self.options)?,
        }))
    }

    pub(crate) fn read_record(&mut self) -> Result<Option<FormattedRecord>> {
        if let Some(record) = self.pending.pop_front() {
            return Ok(Some(record));
        }

        let Some(raw) = self.raw.read_record()? else {
            return Ok(None);
        };
        Ok(Some(FormattedRecord {
            lines: transform::format_record_lines(raw.bytes, self.options)?,
        }))
    }

    pub(crate) fn fill_window(
        &mut self,
        window: &mut Vec<FormattedRecord>,
        target: usize,
    ) -> Result<()> {
        while window.len() < target {
            let Some(record) = self.read_record()? else {
                break;
            };
            window.push(record);
        }
        Ok(())
    }

    pub(crate) fn unread_front(&mut self, records: Vec<FormattedRecord>) {
        for record in records.into_iter().rev() {
            self.pending.push_front(record);
        }
    }
}

pub(crate) struct FormattedRecordBytes {
    pub(crate) source_offset: u64,
    pub(crate) source_bytes: u64,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormattedRecord {
    pub(crate) lines: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;
    use crate::transform::FormatKind;

    fn temp_source(contents: &[u8]) -> (NamedTempFile, InputSource) {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(contents).unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        (temp, source)
    }

    #[test]
    fn raw_record_reader_tracks_source_offsets() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n");
        let mut reader = RawRecordReader::new(&source).unwrap();

        let first = reader.read_record().unwrap().unwrap();
        assert_eq!(first.source_offset, 0);
        assert_eq!(first.source_bytes, 8);
        assert_eq!(first.bytes, b"{\"a\":1}\n");

        let second = reader.read_record().unwrap().unwrap();
        assert_eq!(second.source_offset, 8);
        assert_eq!(second.source_bytes, 8);
        assert_eq!(second.bytes, b"{\"b\":2}\n");

        assert!(reader.read_record().unwrap().is_none());
    }

    #[test]
    fn formatted_record_reader_supports_unread_lookahead() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let mut reader = FormattedRecordReader::new(&source, options).unwrap();
        let mut window = Vec::new();

        reader.fill_window(&mut window, 2).unwrap();
        assert_eq!(window.len(), 2);
        assert!(window[0].lines.iter().any(|line| line.contains("\"a\"")));

        let second = window.pop().unwrap();
        reader.unread_front(vec![second]);
        let second = reader.read_record().unwrap().unwrap();
        assert!(second.lines.iter().any(|line| line.contains("\"b\"")));
        assert!(reader.read_record().unwrap().is_none());
    }

    #[test]
    fn formatted_record_bytes_preserve_source_position() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let mut reader = FormattedRecordReader::new(&source, options).unwrap();

        let first = reader.read_record_bytes().unwrap().unwrap();
        assert_eq!(first.source_offset, 0);
        assert_eq!(first.source_bytes, 8);
        assert!(first.bytes.starts_with(b"{\n"));

        let second = reader.read_record_bytes().unwrap().unwrap();
        assert_eq!(second.source_offset, 8);
        assert_eq!(second.source_bytes, 1);
        assert!(second.bytes.is_empty());
    }
}
