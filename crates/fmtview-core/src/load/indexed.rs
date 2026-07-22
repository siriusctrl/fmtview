use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use super::{
    lines::{index_lines, strip_line_end},
    view_file::ViewFile,
};

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

#[cfg(test)]
mod tests {
    use std::io::Write;

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
}
