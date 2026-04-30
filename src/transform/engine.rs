use std::io::{BufReader, BufWriter, Cursor, Write};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::input::InputSource;

use super::IO_BUFFER_BYTES;
use super::{
    TransformStrategy,
    detect::candidate_kinds,
    json::{format_json, format_json_value, format_jsonl, trim_line_end},
    types::{FormatKind, FormatOptions},
    xml::format_xml_reader,
};

pub fn transform_source_to_temp(
    source: &InputSource,
    options: &FormatOptions,
    strategy: TransformStrategy,
) -> Result<NamedTempFile> {
    match strategy {
        TransformStrategy::PrettyPrint | TransformStrategy::RecordPrettyPrint => {
            format_source_to_temp(source, options)
        }
        TransformStrategy::Passthrough => passthrough_source_to_temp(source),
    }
}

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
        "failed to format {} as JSON, JSONL, XML, plain text, or Jinja:\n{}",
        source.label(),
        errors.join("\n")
    )
}

fn passthrough_source_to_temp(source: &InputSource) -> Result<NamedTempFile> {
    let mut temp = NamedTempFile::new().context("failed to create passthrough temp file")?;
    {
        let mut input = BufReader::with_capacity(IO_BUFFER_BYTES, source.open()?);
        let mut output = BufWriter::with_capacity(IO_BUFFER_BYTES, temp.as_file_mut());
        copy_source_without_formatting(source, &mut input, &mut output)?;
        output
            .flush()
            .context("failed to flush passthrough output")?;
    }
    Ok(temp)
}

fn passthrough_source_to_writer<W: Write>(source: &InputSource, output: &mut W) -> Result<()> {
    let mut input = BufReader::with_capacity(IO_BUFFER_BYTES, source.open()?);
    copy_source_without_formatting(source, &mut input, output)
}

fn copy_source_without_formatting<R: std::io::Read, W: Write>(
    source: &InputSource,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    std::io::copy(input, output)
        .with_context(|| format!("failed to copy {} without formatting", source.label()))?;
    Ok(())
}

fn try_format_source_to_temp(
    source: &InputSource,
    kind: FormatKind,
    options: &FormatOptions,
) -> Result<NamedTempFile> {
    let mut temp = NamedTempFile::new().context("failed to create formatted temp file")?;
    {
        let mut output = BufWriter::with_capacity(IO_BUFFER_BYTES, temp.as_file_mut());
        try_format_source_to_writer(source, kind, options, &mut output)?;
        output.flush().context("failed to flush formatted output")?;
    }
    Ok(temp)
}

fn try_format_source_to_writer<W: Write>(
    source: &InputSource,
    kind: FormatKind,
    options: &FormatOptions,
    output: &mut W,
) -> Result<()> {
    match kind {
        FormatKind::Auto => unreachable!("auto is expanded before formatting"),
        FormatKind::Json => format_json(source, output, options)
            .with_context(|| format!("failed to format {} as JSON", source.label()))?,
        FormatKind::Jsonl => format_jsonl(source, output, options)
            .with_context(|| format!("failed to format {} as JSONL", source.label()))?,
        FormatKind::Xml => format_xml_reader(
            BufReader::with_capacity(IO_BUFFER_BYTES, source.open()?),
            output,
            options.indent,
        )
        .with_context(|| format!("failed to format {} as XML", source.label()))?,
        FormatKind::Plain | FormatKind::Jinja => passthrough_source_to_writer(source, output)?,
    }
    Ok(())
}

pub fn format_record_to_string(input: &[u8], kind: FormatKind, indent: usize) -> Result<String> {
    let output = format_record_to_bytes(input, kind, indent)?;
    String::from_utf8(output).context("formatted record was not valid UTF-8")
}

pub fn format_record_to_bytes(input: &[u8], kind: FormatKind, indent: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(record_output_capacity(input.len()));
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
        FormatKind::Plain | FormatKind::Jinja => output.extend_from_slice(input),
    }
    Ok(output)
}

fn record_output_capacity(input_len: usize) -> usize {
    if input_len >= 1024 * 1024 {
        return input_len.saturating_add(input_len / 3);
    }

    input_len.saturating_mul(2).clamp(128, 8192)
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

    let formatted = format_record_bytes(line, options)?;
    Ok(String::from_utf8_lossy(&formatted)
        .lines()
        .map(str::to_owned)
        .collect())
}

pub(crate) fn format_record_bytes(line: &[u8], options: FormatOptions) -> Result<Vec<u8>> {
    let trimmed = trim_record_line_end(line);
    if trim_ascii_ws(trimmed).is_empty() {
        return Ok(Vec::new());
    }

    let formatted = match options.kind {
        FormatKind::Auto => record_format_kind(trimmed)
            .and_then(|kind| format_record_to_bytes(trimmed, kind, options.indent).ok()),
        FormatKind::Json | FormatKind::Jsonl => Some(format_record_to_bytes(
            trimmed,
            FormatKind::Json,
            options.indent,
        )?),
        FormatKind::Xml => Some(format_record_to_bytes(
            trimmed,
            FormatKind::Xml,
            options.indent,
        )?),
        FormatKind::Plain | FormatKind::Jinja => None,
    };

    Ok(formatted.unwrap_or_else(|| trimmed.to_vec()))
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
