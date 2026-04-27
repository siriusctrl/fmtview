use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::{
    format::{self, FormatKind, FormatOptions},
    input::InputSource,
    line_index::ViewFile,
};

const SNIFF_BYTES: usize = 1024 * 1024;
const SNIFF_LINES: usize = 16;
pub fn should_use_lazy_preview(source: &InputSource, options: &FormatOptions) -> Result<bool> {
    match options.kind {
        FormatKind::Jsonl => Ok(true),
        FormatKind::Json | FormatKind::Xml => Ok(false),
        FormatKind::Auto => {
            let sample = PreviewSample::read(source)?;
            Ok(sample.non_empty_lines >= 2
                && sample.parseable_record_lines == sample.non_empty_lines)
        }
    }
}

pub struct LazyFormattedFile {
    label: String,
    options: FormatOptions,
    len: u64,
    state: RefCell<LazyState>,
}

impl LazyFormattedFile {
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
                spool: NamedTempFile::new().context("failed to create lazy preview spool")?,
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

impl ViewFile for LazyFormattedFile {
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
        let mut file =
            File::open(state.spool.path()).context("failed to open lazy preview spool")?;
        file.seek(SeekFrom::Start(state.line_offsets[start]))
            .context("failed to seek lazy preview spool")?;
        let mut reader = BufReader::new(file);
        let mut lines = Vec::with_capacity(end - start);
        for _ in start..end {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .context("failed to read lazy preview spool")?;
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
    let rendered = format_record_lines(&raw_line, options)?;
    state.raw_line = raw_line;
    for line in rendered {
        state.line_offsets.push(state.spool_len);
        state.raw_line_offsets.push(record_start);
        state
            .spool
            .as_file_mut()
            .write_all(line.as_bytes())
            .context("failed to write lazy preview spool")?;
        state
            .spool
            .as_file_mut()
            .write_all(b"\n")
            .context("failed to write lazy preview spool")?;
        state.spool_len = state
            .spool_len
            .saturating_add(line.len() as u64)
            .saturating_add(1);
    }
    Ok(true)
}

#[derive(Default)]
struct PreviewSample {
    non_empty_lines: usize,
    parseable_record_lines: usize,
}

impl PreviewSample {
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

            let trimmed = trim_ascii_ws(format::trim_record_line_end(&line));
            if trimmed.is_empty() {
                continue;
            }
            sample.non_empty_lines += 1;
            if parseable_record_line(trimmed) {
                sample.parseable_record_lines += 1;
            }
        }

        Ok(sample)
    }
}

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

fn parseable_record_line(line: &[u8]) -> bool {
    record_format_kind(line)
        .and_then(|kind| format::format_record_to_string(line, kind, 2).ok())
        .is_some()
}

fn format_record_lines(line: &[u8], options: FormatOptions) -> Result<Vec<String>> {
    let trimmed = format::trim_record_line_end(line);
    if trim_ascii_ws(trimmed).is_empty() {
        return Ok(vec![String::new()]);
    }

    let formatted = match options.kind {
        FormatKind::Auto => record_format_kind(trimmed)
            .and_then(|kind| format::format_record_to_string(trimmed, kind, options.indent).ok()),
        FormatKind::Json | FormatKind::Jsonl => Some(format::format_record_to_string(
            trimmed,
            FormatKind::Json,
            options.indent,
        )?),
        FormatKind::Xml => Some(format::format_record_to_string(
            trimmed,
            FormatKind::Xml,
            options.indent,
        )?),
    };

    Ok(formatted
        .unwrap_or_else(|| String::from_utf8_lossy(trimmed).into_owned())
        .lines()
        .map(str::to_owned)
        .collect())
}

fn record_format_kind(line: &[u8]) -> Option<FormatKind> {
    match trim_ascii_ws(line).first().copied() {
        Some(b'<') => Some(FormatKind::Xml),
        Some(b'{' | b'[' | b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n') => {
            Some(FormatKind::Json)
        }
        _ => None,
    }
}

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
mod tests {
    use std::{io::Write, time::Instant};

    use tempfile::NamedTempFile;

    use super::*;

    fn temp_source(contents: &[u8]) -> (NamedTempFile, InputSource) {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(contents).unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        (temp, source)
    }

    #[test]
    fn lazy_preview_does_not_require_jsonl_extension() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };

        assert!(should_use_lazy_preview(&source, &options).unwrap());
    }

    #[test]
    fn lazy_preview_reads_only_records_needed_for_first_window() {
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
        let file = LazyFormattedFile::new(&source, options).unwrap();

        let lines = file.read_window(0, 12).unwrap();

        assert_eq!(lines[0], "{");
        assert!(lines.iter().any(|line| line.contains("\"payload\"")));
        assert!(
            file.loaded_record_count() < 4,
            "first window should not scan all input records"
        );
    }

    #[test]
    fn lazy_preview_idle_preload_advances_known_line_count() {
        let mut data = Vec::new();
        for index in 0..10 {
            writeln!(data, "{{\"index\":{index},\"payload\":{{\"ok\":true}}}}").unwrap();
        }
        let (_temp, source) = temp_source(&data);
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let file = LazyFormattedFile::new(&source, options).unwrap();

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
        let file = LazyFormattedFile::new(&source, options).unwrap();

        let error = file.read_window(0, 20).unwrap_err();

        assert!(error.to_string().contains("failed to parse JSON record"));
    }

    #[test]
    fn multiline_whole_documents_do_not_use_lazy_preview() {
        let (_json_temp, json_source) = temp_source(b"{\n  \"items\": [\n    {\"a\": 1}\n  ]\n}\n");
        let (_xml_temp, xml_source) = temp_source(b"<root>\n  <item>one</item>\n</root>\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };

        assert!(!should_use_lazy_preview(&json_source, &options).unwrap());
        assert!(!should_use_lazy_preview(&xml_source, &options).unwrap());
    }

    #[test]
    fn lazy_preview_reads_spooled_lines_after_preload() {
        let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };
        let file = LazyFormattedFile::new(&source, options).unwrap();

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

    #[test]
    fn small_whole_document_does_not_use_lazy_preview() {
        let (_temp, source) = temp_source(b"{\"items\":[{\"a\":1},{\"b\":2}]}\n");
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };

        assert!(!should_use_lazy_preview(&source, &options).unwrap());
    }

    #[test]
    #[ignore = "performance smoke; generates a large temporary JSONL-style file"]
    fn perf_lazy_preview_first_window_does_not_scan_generated_large_file() {
        let mut temp = NamedTempFile::new().unwrap();
        let record = br#"{"level":"info","payload":{"message":"lazy performance smoke","xml":"<root><item id=\"1\"><name>visible</name></item><item id=\"2\"><name>visible</name></item></root>","items":[{"a":1},{"b":2},{"c":{"d":{"e":true}}}]}}"#;
        let target = 128 * 1024 * 1024;
        let mut written = 0_usize;
        while written < target {
            temp.write_all(record).unwrap();
            temp.write_all(b"\n").unwrap();
            written += record.len() + 1;
        }
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        let options = FormatOptions {
            kind: FormatKind::Auto,
            indent: 2,
        };

        let started = Instant::now();
        let file = LazyFormattedFile::new(&source, options).unwrap();
        let lines = file.read_window(0, 40).unwrap();
        let elapsed = started.elapsed();

        eprintln!(
            "lazy generated first window: {elapsed:?}, records read: {}",
            file.loaded_record_count()
        );
        assert!(!lines.is_empty());
        assert!(
            file.loaded_record_count() < 8,
            "first window should read only enough records to fill the viewport"
        );
    }
}
