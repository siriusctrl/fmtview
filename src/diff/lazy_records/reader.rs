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

pub(super) struct LazyRecordReader {
    label: String,
    options: FormatOptions,
    reader: BufReader<File>,
    line: Vec<u8>,
    pending: VecDeque<FormattedRecord>,
}

impl LazyRecordReader {
    pub(super) fn new(source: &InputSource, options: FormatOptions) -> Result<Self> {
        Ok(Self {
            label: source.label().to_owned(),
            options,
            reader: BufReader::new(source.open()?),
            line: Vec::with_capacity(8192),
            pending: VecDeque::new(),
        })
    }

    pub(super) fn read_record(&mut self) -> Result<Option<FormattedRecord>> {
        if let Some(record) = self.pending.pop_front() {
            return Ok(Some(record));
        }

        self.line.clear();
        let read = self
            .reader
            .read_until(b'\n', &mut self.line)
            .with_context(|| format!("failed to read {}", self.label))?;
        if read == 0 {
            return Ok(None);
        }
        Ok(Some(FormattedRecord {
            lines: transform::format_record_lines(&self.line, self.options)?,
        }))
    }

    pub(super) fn fill_window(
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

    pub(super) fn unread_front(&mut self, records: Vec<FormattedRecord>) {
        for record in records.into_iter().rev() {
            self.pending.push_front(record);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FormattedRecord {
    pub(super) lines: Vec<String>,
}
