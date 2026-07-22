use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Seek, SeekFrom},
};

use anyhow::{Context, Result};

use super::ViewFile;

const RAW_VIEW_CHUNK_BYTES: u64 = 32 * 1024;

pub(crate) struct RawRecordViewFile {
    label: String,
    file: RefCell<File>,
    offset: u64,
    len: u64,
}

impl RawRecordViewFile {
    pub(crate) fn new(
        mut file: File,
        source_label: &str,
        offset: u64,
        raw_len: u64,
        source_line: usize,
    ) -> Result<Self> {
        let mut len = raw_len;
        if len > 0 && byte_at(&mut file, offset.saturating_add(len - 1))? == b'\n' {
            len -= 1;
        }
        if len > 0 && byte_at(&mut file, offset.saturating_add(len - 1))? == b'\r' {
            len -= 1;
        }
        Ok(Self {
            label: format!("{source_label} | raw record at line {}", source_line + 1),
            file: RefCell::new(file),
            offset,
            len,
        })
    }

    fn chunk_count(&self) -> usize {
        usize::try_from(self.len.div_ceil(RAW_VIEW_CHUNK_BYTES))
            .unwrap_or(usize::MAX)
            .max(1)
    }

    fn boundary(&self, chunk: usize, file: &mut File) -> Result<u64> {
        let nominal = u64::try_from(chunk)
            .unwrap_or(u64::MAX)
            .saturating_mul(RAW_VIEW_CHUNK_BYTES)
            .min(self.len);
        if nominal == 0 || nominal >= self.len {
            return Ok(nominal);
        }

        file.seek(SeekFrom::Start(self.offset.saturating_add(nominal)))
            .context("failed to seek raw record chunk boundary")?;
        let mut lookahead = [0_u8; 4];
        let read = file
            .read(&mut lookahead)
            .context("failed to read raw record chunk boundary")?;
        let advance = lookahead[..read]
            .iter()
            .take_while(|byte| is_utf8_continuation(**byte))
            .take(3)
            .count();
        Ok(nominal.saturating_add(advance as u64).min(self.len))
    }
}

impl ViewFile for RawRecordViewFile {
    fn label(&self) -> &str {
        &self.label
    }

    fn line_count(&self) -> usize {
        self.chunk_count()
    }

    fn byte_len(&self) -> u64 {
        self.len
    }

    fn byte_offset_for_line(&self, line: usize) -> u64 {
        u64::try_from(line)
            .unwrap_or(u64::MAX)
            .saturating_mul(RAW_VIEW_CHUNK_BYTES)
            .min(self.len)
    }

    fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
        if count == 0 || start >= self.chunk_count() {
            return Ok(Vec::new());
        }
        let end = start.saturating_add(count).min(self.chunk_count());
        let mut file = self.file.borrow_mut();
        let mut lines = Vec::with_capacity(end - start);
        for chunk in start..end {
            let chunk_start = self.boundary(chunk, &mut file)?;
            let chunk_end = self.boundary(chunk.saturating_add(1), &mut file)?;
            let chunk_len = usize::try_from(chunk_end.saturating_sub(chunk_start))
                .context("raw record chunk was too large")?;
            file.seek(SeekFrom::Start(self.offset.saturating_add(chunk_start)))
                .context("failed to seek raw record spool")?;
            let mut bytes = vec![0_u8; chunk_len];
            file.read_exact(&mut bytes)
                .context("failed to read raw record spool")?;
            lines
                .push(String::from_utf8(bytes).unwrap_or_else(|error| {
                    String::from_utf8_lossy(error.as_bytes()).into_owned()
                }));
        }
        Ok(lines)
    }
}

fn byte_at(file: &mut File, offset: u64) -> Result<u8> {
    file.seek(SeekFrom::Start(offset))
        .context("failed to seek raw record ending")?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)
        .context("failed to read raw record ending")?;
    Ok(byte[0])
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn raw_record_view_reads_bounded_utf8_chunks_without_the_delimiter() {
        let mut spool = NamedTempFile::new().unwrap();
        let text = format!(
            "{{\"text\":\"{}é{}\"}}\r\n",
            "a".repeat(32 * 1024 - 11),
            "b".repeat(64)
        );
        spool.write_all(text.as_bytes()).unwrap();
        spool.flush().unwrap();
        let view = RawRecordViewFile::new(
            spool.reopen().unwrap(),
            "fixture.jsonl",
            0,
            text.len() as u64,
            12,
        )
        .unwrap();

        let chunks = view.read_window(0, view.line_count()).unwrap();

        assert_eq!(chunks.concat(), text.trim_end());
        assert!(chunks.iter().all(|chunk| chunk.len() <= 32 * 1024 + 3));
        assert!(view.label().contains("raw record at line 13"));
    }

    #[test]
    fn invalid_utf8_continuations_adjust_a_chunk_boundary_by_at_most_three_bytes() {
        let mut spool = NamedTempFile::new().unwrap();
        let mut bytes = vec![b'a'; RAW_VIEW_CHUNK_BYTES as usize];
        bytes.extend_from_slice(&[0x80; 4]);
        bytes.push(b'\n');
        spool.write_all(&bytes).unwrap();
        spool.flush().unwrap();
        let view = RawRecordViewFile::new(
            spool.reopen().unwrap(),
            "invalid.jsonl",
            0,
            bytes.len() as u64,
            0,
        )
        .unwrap();
        let mut file = spool.reopen().unwrap();

        assert_eq!(
            view.boundary(1, &mut file).unwrap(),
            RAW_VIEW_CHUNK_BYTES + 3
        );
        assert_eq!(view.read_window(0, 1).unwrap().len(), 1);
    }

    #[test]
    fn empty_raw_record_still_has_one_empty_virtual_line() {
        let mut spool = NamedTempFile::new().unwrap();
        spool.write_all(b"\n").unwrap();
        spool.flush().unwrap();
        let view = RawRecordViewFile::new(spool.reopen().unwrap(), "empty.jsonl", 0, 1, 0).unwrap();

        assert_eq!(view.line_count(), 1);
        assert_eq!(view.read_window(0, 1).unwrap(), vec![""]);
    }
}
