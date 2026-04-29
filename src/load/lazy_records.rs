#[cfg(test)]
use std::ffi::OsStr;
use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

#[cfg(test)]
use crate::transform::FormatKind;
use crate::{
    input::InputSource,
    load::ViewFile,
    transform::{self, FormatOptions},
};

#[cfg(test)]
const SNIFF_BYTES: usize = 1024 * 1024;
#[cfg(test)]
const SNIFF_LINES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPlan {
    LazyRecords,
    EagerDocument,
    RawIndexedText,
}

#[cfg(test)]
pub fn load_plan(source: &InputSource, options: &FormatOptions) -> Result<LoadPlan> {
    match options.kind {
        FormatKind::Jsonl => Ok(LoadPlan::LazyRecords),
        FormatKind::Json | FormatKind::Xml => Ok(LoadPlan::EagerDocument),
        FormatKind::Plain | FormatKind::Jinja => Ok(LoadPlan::RawIndexedText),
        FormatKind::Auto => {
            if has_record_extension(source) {
                return Ok(LoadPlan::LazyRecords);
            }
            if has_raw_text_extension(source) {
                return Ok(LoadPlan::RawIndexedText);
            }

            let sample = LoadSample::read(source)?;
            Ok(if sample.looks_like_record_stream() {
                LoadPlan::LazyRecords
            } else {
                LoadPlan::EagerDocument
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
    label: String,
    options: FormatOptions,
    len: u64,
    state: RefCell<LazyState>,
}

impl LazyTransformedFile {
    pub fn new(source: &InputSource, options: FormatOptions) -> Result<Self> {
        let file = source.open()?;
        let len = file
            .metadata()
            .with_context(|| format!("failed to stat {}", source.label()))?
            .len();
        Ok(Self {
            label: source.label().to_owned(),
            options,
            len,
            state: RefCell::new(LazyState {
                reader: BufReader::new(file),
                spool: NamedTempFile::new().context("failed to create lazy load spool")?,
                spool_len: 0,
                raw_offset: 0,
                raw_line: Vec::with_capacity(8192),
                line_offsets: Vec::new(),
                raw_line_offsets: Vec::new(),
                complete: len == 0,
                records_read: 0,
            }),
        })
    }

    #[cfg(test)]
    fn loaded_record_count(&self) -> usize {
        self.state.borrow().records_read
    }

    #[cfg(test)]
    fn indexed_line_count(&self) -> usize {
        self.state.borrow().line_offsets.len()
    }

    fn ensure_lines(&self, needed: usize) -> Result<()> {
        let mut state = self.state.borrow_mut();
        while state.line_offsets.len() < needed && !state.complete {
            if !read_next_record(&mut state, self.options, &self.label)? {
                break;
            }
        }
        Ok(())
    }
}

impl ViewFile for LazyTransformedFile {
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
            .raw_line_offsets
            .get(line)
            .copied()
            .unwrap_or(state.raw_offset)
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
            if !read_next_record(&mut state, self.options, &self.label)? {
                break;
            }
            records += 1;
        }

        Ok(state.line_offsets.len() != start_lines || state.complete)
    }
}

struct LazyState {
    reader: BufReader<File>,
    spool: NamedTempFile,
    spool_len: u64,
    raw_offset: u64,
    raw_line: Vec<u8>,
    line_offsets: Vec<u64>,
    raw_line_offsets: Vec<u64>,
    complete: bool,
    records_read: usize,
}

fn read_next_record(state: &mut LazyState, options: FormatOptions, label: &str) -> Result<bool> {
    let record_start = state.raw_offset;
    let mut raw_line = std::mem::take(&mut state.raw_line);
    raw_line.clear();
    let read = state
        .reader
        .read_until(b'\n', &mut raw_line)
        .with_context(|| format!("failed to read {label}"))?;
    if read == 0 {
        state.raw_line = raw_line;
        state.complete = true;
        return Ok(false);
    }

    state.raw_offset = state.raw_offset.saturating_add(read as u64);
    state.records_read = state.records_read.saturating_add(1);
    let rendered = transform::format_record_lines(&raw_line, options)?;
    state.raw_line = raw_line;
    for line in rendered {
        state.line_offsets.push(state.spool_len);
        state.raw_line_offsets.push(record_start);
        state
            .spool
            .as_file_mut()
            .write_all(line.as_bytes())
            .context("failed to write lazy load spool")?;
        state
            .spool
            .as_file_mut()
            .write_all(b"\n")
            .context("failed to write lazy load spool")?;
        state.spool_len = state
            .spool_len
            .saturating_add(line.len() as u64)
            .saturating_add(1);
    }
    Ok(true)
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

fn strip_line_end(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

#[cfg(test)]
mod tests;
