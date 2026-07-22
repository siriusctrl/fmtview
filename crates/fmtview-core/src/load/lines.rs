use std::{fs::File, io::Read};

use anyhow::{Context, Result};
use memchr::memchr_iter;
use tempfile::NamedTempFile;

pub(crate) fn index_lines(temp: &NamedTempFile) -> Result<Vec<u64>> {
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
        for index in memchr_iter(b'\n', &buf[..read]) {
            let line_start = offset + index as u64 + 1;
            if line_start < len {
                offsets.push(line_start);
            }
        }
        offset += read as u64;
    }

    Ok(offsets)
}

pub(crate) fn strip_line_end(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

pub(crate) fn strip_byte_line_end(line: &mut Vec<u8>) {
    if line.ends_with(b"\n") {
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
    }
}
