//! Source coordinates for token-preserving whole-document transforms.
//!
//! JSON, XML and HTML preserve the ordered non-whitespace bytes. Align them
//! with bounded buffers after formatting; retain only one number per output
//! line, never the document text. Blank output rows consume at most one source
//! newline so verbatim multiline text keeps its physical source coordinates.
use std::io::BufRead;

use anyhow::{Result, ensure};

pub(super) struct SourceLines {
    pub lines: Vec<usize>,
    pub count: usize,
}

impl SourceLines {
    pub fn build(source: impl BufRead, mut formatted: impl BufRead) -> Result<Self> {
        let mut source = SourceCursor {
            input: source,
            line: 1,
            last: None,
        };
        // The XML reader removes an initial UTF-8 BOM. It occupies bytes but
        // does not change the physical line containing the first token.
        if source.input.fill_buf()?.starts_with(b"\xef\xbb\xbf")
            && !formatted.fill_buf()?.starts_with(b"\xef\xbb\xbf")
        {
            source.input.consume(3);
            source.last = Some(0xbf);
        }
        let mut lines = Vec::new();
        let mut origin = None;
        let mut row_started = false;
        loop {
            let bytes = formatted.fill_buf()?;
            if bytes.is_empty() {
                break;
            }
            let mut pos = 0;
            while pos < bytes.len() {
                let byte = bytes[pos];
                row_started = true;
                if byte.is_ascii_whitespace() {
                    if byte == b'\n' {
                        lines.push(origin.take().unwrap_or(source.line));
                        source.skip_whitespace(true)?;
                        row_started = false;
                    }
                    pos += 1;
                } else {
                    source.skip_whitespace(false)?;
                    origin.get_or_insert(source.line);
                    let len = bytes[pos..]
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(bytes.len() - pos);
                    source.match_bytes(&bytes[pos..pos + len])?;
                    pos += len;
                }
            }
            let len = bytes.len();
            formatted.consume(len);
        }
        if row_started {
            lines.push(origin.unwrap_or(source.line));
        }
        source.skip_whitespace(false)?;
        ensure!(
            source.input.fill_buf()?.is_empty(),
            "unmapped source content"
        );
        let count = match source.last {
            None => 0,
            Some(b'\n') => source.line - 1,
            Some(_) => source.line,
        };
        for line in &mut lines {
            *line = (*line).min(count.max(1));
        }
        Ok(Self { lines, count })
    }
}

struct SourceCursor<R> {
    input: R,
    line: usize,
    last: Option<u8>,
}

impl<R: BufRead> SourceCursor<R> {
    fn skip_whitespace(&mut self, stop_after_newline: bool) -> Result<()> {
        loop {
            let bytes = self.input.fill_buf()?;
            let mut consumed = 0;
            let mut newline = false;
            for &byte in bytes {
                if !byte.is_ascii_whitespace() {
                    break;
                }
                consumed += 1;
                self.last = Some(byte);
                if byte == b'\n' {
                    self.line += 1;
                    newline = true;
                    if stop_after_newline {
                        break;
                    }
                }
            }
            if consumed == 0 {
                return Ok(());
            }
            self.input.consume(consumed);
            if stop_after_newline && newline {
                return Ok(());
            }
        }
    }

    fn match_bytes(&mut self, mut expected: &[u8]) -> Result<()> {
        while !expected.is_empty() {
            self.skip_whitespace(false)?;
            let bytes = self.input.fill_buf()?;
            let available = bytes.len().min(expected.len());
            let len = bytes[..available]
                .iter()
                .position(u8::is_ascii_whitespace)
                .unwrap_or(available);
            ensure!(
                len > 0 && bytes[..len] == expected[..len],
                "formatted content differs from source tokens at source line {}",
                self.line
            );
            self.last = Some(bytes[len - 1]);
            self.input.consume(len);
            expected = &expected[len..];
        }
        Ok(())
    }
}
