use std::io::{BufRead, BufReader};

use anyhow::{Context, Result};

use crate::{input::InputSource, transform};

pub(super) const SNIFF_BYTES: usize = 1024 * 1024;
const SNIFF_LINES: usize = 16;
/// Cap on the prefix retained for XML/HTML markup sniffing. Strong signals
/// (`<?xml`, `<!doctype html>`, `<html>`) live near the top of the document.
const MARKUP_SNIFF_PREFIX: usize = 4096;

#[derive(Default)]
pub(super) struct TypeSample {
    pub(super) first_non_ws: Option<u8>,
    non_empty_lines: usize,
    parseable_record_lines: usize,
    pub(super) markup_prefix: Vec<u8>,
}

impl TypeSample {
    pub(super) fn read(source: &InputSource) -> Result<Self> {
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
            if sample.markup_prefix.len() < MARKUP_SNIFF_PREFIX {
                let remaining = MARKUP_SNIFF_PREFIX - sample.markup_prefix.len();
                let take = line.len().min(remaining);
                sample.markup_prefix.extend_from_slice(&line[..take]);
            }

            if sample.first_non_ws.is_none() {
                sample.first_non_ws = line
                    .iter()
                    .copied()
                    .find(|byte| !byte.is_ascii_whitespace());
            }

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

    pub(super) fn looks_like_record_stream(&self) -> bool {
        self.non_empty_lines >= 2 && self.parseable_record_lines == self.non_empty_lines
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

fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
