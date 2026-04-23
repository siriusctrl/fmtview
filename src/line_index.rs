use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

pub struct IndexedTempFile {
    label: String,
    temp: NamedTempFile,
    offsets: Vec<u64>,
}

impl IndexedTempFile {
    pub fn new(label: String, temp: NamedTempFile) -> Result<Self> {
        let offsets = index_lines(&temp)?;
        Ok(Self {
            label,
            temp,
            offsets,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn line_count(&self) -> usize {
        self.offsets.len()
    }

    pub fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
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

    let file = File::open(temp.path()).context("failed to open temp file for indexing")?;
    let mut reader = BufReader::new(file);
    let mut offsets = vec![0_u64];
    let mut offset = 0_u64;
    let mut buf = Vec::with_capacity(8192);

    loop {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .context("failed to index formatted output")?;
        if read == 0 {
            break;
        }
        offset += read as u64;
        if offset < len {
            offsets.push(offset);
        }
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
    use std::io::Write;

    use super::*;

    #[test]
    fn indexes_lines_without_trailing_empty_line() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "a\nb\n").unwrap();
        let indexed = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        assert_eq!(indexed.line_count(), 2);
        assert_eq!(indexed.read_window(1, 10).unwrap(), vec!["b"]);
    }
}
