use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    time::Duration,
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

mod lazy;
mod lazy_records;

pub use lazy_records::{LazyTransformedFile, LoadPlan};

pub trait ViewFile {
    fn label(&self) -> &str;
    fn line_count(&self) -> usize;
    fn line_count_exact(&self) -> bool {
        true
    }
    fn byte_len(&self) -> u64;
    fn byte_offset_for_line(&self, line: usize) -> u64;
    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>>;
    fn preload(&self, _max_lines: usize, _max_records: usize, _budget: Duration) -> Result<bool> {
        Ok(false)
    }
}

pub struct IndexedTempFile {
    label: String,
    temp: NamedTempFile,
    offsets: Vec<u64>,
    len: u64,
}

impl IndexedTempFile {
    pub fn new(label: String, temp: NamedTempFile) -> Result<Self> {
        let offsets = index_lines(&temp)?;
        let len = temp
            .as_file()
            .metadata()
            .context("failed to stat indexed temp file")?
            .len();
        Ok(Self {
            label,
            temp,
            offsets,
            len,
        })
    }
}

impl ViewFile for IndexedTempFile {
    fn label(&self) -> &str {
        &self.label
    }

    fn line_count(&self) -> usize {
        self.offsets.len()
    }

    fn byte_len(&self) -> u64 {
        self.len
    }

    fn byte_offset_for_line(&self, line: usize) -> u64 {
        self.offsets.get(line).copied().unwrap_or(self.len)
    }

    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
        if count == 0 || start >= self.offsets.len() {
            return Ok(Vec::new());
        }

        let mut file = File::open(self.temp.path()).context("failed to open indexed temp file")?;
        file.seek(SeekFrom::Start(self.offsets[start]))
            .context("failed to seek indexed temp file")?;
        let mut reader = BufReader::new(file);
        let mut lines = Vec::with_capacity(count);

        for _ in 0..count {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .context("failed to read indexed line")?;
            if read == 0 {
                break;
            }
            strip_line_end(&mut line);
            lines.push(line);
        }

        Ok(lines)
    }
}

fn index_lines(temp: &NamedTempFile) -> Result<Vec<u64>> {
    let len = temp
        .as_file()
        .metadata()
        .context("failed to stat temp file")?
        .len();
    if len == 0 {
        return Ok(Vec::new());
    }

    let mut file = File::open(temp.path()).context("failed to open temp file for indexing")?;
    let mut offsets = vec![0_u64];
    let mut offset = 0_u64;
    let mut buf = [0_u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buf)
            .context("failed to index formatted output")?;
        if read == 0 {
            break;
        }
        for (index, byte) in buf[..read].iter().enumerate() {
            if *byte == b'\n' {
                let line_start = offset + index as u64 + 1;
                if line_start < len {
                    offsets.push(line_start);
                }
            }
        }
        offset += read as u64;
    }

    Ok(offsets)
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
    use std::{
        io::Write,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn indexes_lines_without_trailing_empty_line() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "a\nb\n").unwrap();
        let indexed = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        assert_eq!(indexed.line_count(), 2);
        assert_eq!(indexed.byte_len(), 4);
        assert_eq!(indexed.byte_offset_for_line(0), 0);
        assert_eq!(indexed.byte_offset_for_line(1), 2);
        assert_eq!(indexed.byte_offset_for_line(2), 4);
        assert_eq!(indexed.read_window(1, 10).unwrap(), vec!["b"]);
    }

    #[test]
    fn indexes_lines_after_long_records() {
        let mut temp = NamedTempFile::new().unwrap();
        let long = "a".repeat(70 * 1024);
        writeln!(temp, "{long}").unwrap();
        writeln!(temp, "b").unwrap();

        let indexed = IndexedTempFile::new("test".to_owned(), temp).unwrap();

        assert_eq!(indexed.line_count(), 2);
        assert_eq!(indexed.byte_offset_for_line(1), long.len() as u64 + 1);
        assert_eq!(indexed.read_window(1, 1).unwrap(), vec!["b"]);
    }

    #[test]
    #[ignore = "performance smoke; run benches/load-performance.sh"]
    fn perf_raw_indexed_load() {
        let mut temp = NamedTempFile::new().unwrap();
        let line = format!("{}\n", "x".repeat(240));
        let lines = 250_000;
        for _ in 0..lines {
            temp.write_all(line.as_bytes()).unwrap();
        }
        temp.flush().unwrap();
        let input_bytes = temp.as_file().metadata().unwrap().len();

        let started = Instant::now();
        let indexed = IndexedTempFile::new("raw".to_owned(), temp).unwrap();
        let elapsed = started.elapsed();
        let window = indexed.read_window(120_000, 120).unwrap();

        eprintln!(
            "raw indexed load: {elapsed:?}, indexed_lines={}, input_bytes={input_bytes}, window_lines={}",
            indexed.line_count(),
            window.len()
        );
        assert_eq!(indexed.line_count(), lines);
        assert_eq!(window.len(), 120);
        assert!(
            elapsed < Duration::from_secs(5),
            "raw indexed load took {elapsed:?}"
        );
    }
}
