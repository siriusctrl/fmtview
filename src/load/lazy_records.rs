#[cfg(test)]
use std::ffi::OsStr;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    time::Duration,
};

use anyhow::{Context, Result};

#[cfg(test)]
use crate::transform::FormatKind;
use crate::{
    input::InputSource,
    load::{
        ViewFile,
        lazy::{LazyBatch, LazyFile, LazyProducer},
    },
    transform::{self, FormatOptions},
};

#[cfg(test)]
const SNIFF_BYTES: usize = 1024 * 1024;
#[cfg(test)]
const SNIFF_LINES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPlan {
    LazyTransformedRecords,
    EagerTransformedDocument,
    EagerIndexedSource,
}

#[cfg(test)]
pub fn load_plan(source: &InputSource, options: &FormatOptions) -> Result<LoadPlan> {
    match options.kind {
        FormatKind::Jsonl => Ok(LoadPlan::LazyTransformedRecords),
        FormatKind::Json | FormatKind::Xml => Ok(LoadPlan::EagerTransformedDocument),
        FormatKind::Plain | FormatKind::Jinja => Ok(LoadPlan::EagerIndexedSource),
        FormatKind::Auto => {
            if has_record_extension(source) {
                return Ok(LoadPlan::LazyTransformedRecords);
            }
            if has_raw_text_extension(source) {
                return Ok(LoadPlan::EagerIndexedSource);
            }

            let sample = LoadSample::read(source)?;
            Ok(if sample.looks_like_record_stream() {
                LoadPlan::LazyTransformedRecords
            } else {
                LoadPlan::EagerTransformedDocument
            })
        }
    }
}

#[cfg(test)]
fn has_record_extension(source: &InputSource) -> bool {
    matches!(
        source
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jsonl" | "ndjson")
    )
}

#[cfg(test)]
fn has_raw_text_extension(source: &InputSource) -> bool {
    matches!(
        source
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("txt" | "text" | "log" | "j2" | "jinja" | "jinja2")
    )
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
#[derive(Default)]
struct LoadSample {
    non_empty_lines: usize,
    parseable_record_lines: usize,
}

#[cfg(test)]
impl LoadSample {
    fn read(source: &InputSource) -> Result<Self> {
        let mut reader = BufReader::new(source.open()?);
        let mut sample = Self::default();
        let mut bytes_read = 0_usize;
        let mut line = Vec::with_capacity(8192);

        while bytes_read < SNIFF_BYTES && sample.non_empty_lines < SNIFF_LINES {
            line.clear();
            let max = SNIFF_BYTES - bytes_read;
            let read = read_line_limited(&mut reader, &mut line, max)
                .with_context(|| format!("failed to inspect {}", source.label()))?;
            if read == 0 {
                break;
            }
            bytes_read += read;

            let trimmed = trim_ascii_ws(transform::trim_record_line_end(&line));
            if trimmed.is_empty() {
                continue;
            }
            sample.non_empty_lines += 1;
            if transform::parseable_record_line(trimmed) {
                sample.parseable_record_lines += 1;
            }
        }

        Ok(sample)
    }

    fn looks_like_record_stream(&self) -> bool {
        self.non_empty_lines >= 2 && self.parseable_record_lines == self.non_empty_lines
    }
}

#[cfg(test)]
fn read_line_limited<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    limit: usize,
) -> Result<usize> {
    let before = line.len();
    let mut total = 0_usize;
    while total < limit {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(available.len())
            .min(limit - total);
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        total += take;
        if line.ends_with(b"\n") || take == 0 {
            break;
        }
    }
    Ok(line.len() - before)
}

#[cfg(test)]
fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(test)]
mod tests;
