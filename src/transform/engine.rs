use std::io::{BufReader, BufWriter, Cursor, Write};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::input::InputSource;

use super::{
    detect::candidate_kinds,
    json::{format_json, format_json_value, format_jsonl, trim_line_end},
    types::{FormatKind, FormatOptions},
    xml::format_xml_reader,
};

pub fn format_source_to_temp(
    source: &InputSource,
    options: &FormatOptions,
) -> Result<NamedTempFile> {
    let candidates = candidate_kinds(source, options)?;
    let mut errors = Vec::new();

    for kind in candidates {
        match try_format_source_to_temp(source, kind, options) {
            Ok(temp) => return Ok(temp),
            Err(error) => errors.push(format!("{kind:?}: {error:#}")),
        }
    }

    bail!(
        "failed to format {} as JSON, JSONL, or XML:\n{}",
        source.label(),
        errors.join("\n")
    )
}

fn try_format_source_to_temp(
    source: &InputSource,
    kind: FormatKind,
    options: &FormatOptions,
) -> Result<NamedTempFile> {
    let mut temp = NamedTempFile::new().context("failed to create formatted temp file")?;
    {
        let mut output = BufWriter::new(temp.as_file_mut());
        match kind {
            FormatKind::Auto => unreachable!("auto is expanded before formatting"),
            FormatKind::Json => format_json(source, &mut output, options)
                .with_context(|| format!("failed to format {} as JSON", source.label()))?,
            FormatKind::Jsonl => format_jsonl(source, &mut output, options)
                .with_context(|| format!("failed to format {} as JSONL", source.label()))?,
            FormatKind::Xml => {
                format_xml_reader(BufReader::new(source.open()?), &mut output, options.indent)
                    .with_context(|| format!("failed to format {} as XML", source.label()))?
            }
        }
        output.flush().context("failed to flush formatted output")?;
    }
    Ok(temp)
}

pub fn format_record_to_string(input: &[u8], kind: FormatKind, indent: usize) -> Result<String> {
    let mut output = Vec::with_capacity(input.len().min(8192));
    match kind {
        FormatKind::Auto => unreachable!("auto must be resolved before formatting a record"),
        FormatKind::Json | FormatKind::Jsonl => {
            format_json_value(Cursor::new(input), &mut output, indent)
                .context("failed to parse JSON record")?
        }
        FormatKind::Xml => {
            format_xml_reader(Cursor::new(input), &mut output, indent)
                .context("failed to parse XML-compatible record")?;
            while output.ends_with(b"\n") || output.ends_with(b"\r") {
                output.pop();
            }
        }
    }
    String::from_utf8(output).context("formatted record was not valid UTF-8")
}

pub fn trim_record_line_end(line: &[u8]) -> &[u8] {
    trim_line_end(line)
}

pub(crate) fn parseable_record_line(line: &[u8]) -> bool {
    record_format_kind(line)
        .and_then(|kind| format_record_to_string(line, kind, 2).ok())
        .is_some()
}

pub(crate) fn format_record_lines(line: &[u8], options: FormatOptions) -> Result<Vec<String>> {
    let trimmed = trim_record_line_end(line);
    if trim_ascii_ws(trimmed).is_empty() {
        return Ok(vec![String::new()]);
    }

    let formatted = match options.kind {
        FormatKind::Auto => record_format_kind(trimmed)
            .and_then(|kind| format_record_to_string(trimmed, kind, options.indent).ok()),
        FormatKind::Json | FormatKind::Jsonl => Some(format_record_to_string(
            trimmed,
            FormatKind::Json,
            options.indent,
        )?),
        FormatKind::Xml => Some(format_record_to_string(
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
