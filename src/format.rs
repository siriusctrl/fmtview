use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, BufWriter, Cursor, Read, Write},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use quick_xml::{Reader as XmlReader, Writer as XmlWriter, events::Event};
use serde::Serialize;
use serde_json::{Value, ser::PrettyFormatter};
use tempfile::NamedTempFile;

use crate::input::InputSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatKind {
    Auto,
    Json,
    Jsonl,
    Xml,
}

#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    pub kind: FormatKind,
    pub indent: usize,
    pub expand_embedded: bool,
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
        "failed to format {} as JSON, JSONL, or XML:\n{}",
        source.label(),
        errors.join("\n")
    )
}

fn candidate_kinds(source: &InputSource, options: &FormatOptions) -> Result<Vec<FormatKind>> {
    if options.kind != FormatKind::Auto {
        return Ok(vec![options.kind]);
    }

    let detected = detect_kind(source)?;
    let mut kinds = Vec::with_capacity(3);
    push_unique(&mut kinds, detected);

    // JSONL is a common ambiguity: the first byte often looks like JSON, but
    // a whole-file JSON parser will reject the second record as trailing data.
    if detected == FormatKind::Json {
        push_unique(&mut kinds, FormatKind::Jsonl);
    }

    push_unique(&mut kinds, FormatKind::Json);
    push_unique(&mut kinds, FormatKind::Jsonl);
    push_unique(&mut kinds, FormatKind::Xml);
    Ok(kinds)
}

fn push_unique(kinds: &mut Vec<FormatKind>, kind: FormatKind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn detect_kind(source: &InputSource) -> Result<FormatKind> {
    match source
        .path()
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => return Ok(FormatKind::Json),
        Some("jsonl" | "ndjson") => return Ok(FormatKind::Jsonl),
        Some("xml") => return Ok(FormatKind::Xml),
        _ => {}
    }

    let mut reader = BufReader::new(source.open()?);
    let mut buf = [0_u8; 8192];
    let read = reader
        .read(&mut buf)
        .with_context(|| format!("failed to inspect {}", source.label()))?;
    let first = buf[..read]
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace());

    match first {
        Some(b'<') => Ok(FormatKind::Xml),
        Some(b'{' | b'[') => Ok(FormatKind::Json),
        _ => Ok(FormatKind::Jsonl),
    }
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

fn format_json<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    if options.expand_embedded {
        let mut value: Value = serde_json::from_reader(BufReader::new(source.open()?))
            .context("failed to parse JSON")?;
        expand_embedded_strings(&mut value, options.indent);
        write_json_value(&value, output, options.indent)?;
        writeln!(output)?;
        return Ok(());
    }

    let indent = vec![b' '; options.indent];
    let formatter = PrettyFormatter::with_indent(&indent);
    let mut serializer = serde_json::Serializer::with_formatter(&mut *output, formatter);
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(source.open()?));
    serde_transcode::transcode(&mut deserializer, &mut serializer)
        .context("failed to transcode JSON")?;
    deserializer
        .end()
        .context("trailing characters after JSON")?;
    writeln!(output)?;
    Ok(())
}

fn format_jsonl<W: Write>(
    source: &InputSource,
    output: &mut W,
    options: &FormatOptions,
) -> Result<()> {
    let mut reader = BufReader::new(source.open()?);
    let mut line = Vec::with_capacity(8192);
    let mut line_number = 0_usize;

    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .context("failed to read JSONL line")?;
        if read == 0 {
            break;
        }

        line_number += 1;
        let trimmed = trim_line_end(&line);
        if trimmed.iter().all(u8::is_ascii_whitespace) {
            writeln!(output)?;
            continue;
        }

        if options.expand_embedded {
            let mut value: Value = serde_json::from_slice(trimmed)
                .with_context(|| format!("failed to parse JSONL line {line_number}"))?;
            expand_embedded_strings(&mut value, options.indent);
            write_json_value(&value, output, options.indent)
                .with_context(|| format!("failed to write JSONL line {line_number}"))?;
            writeln!(output)?;
            continue;
        }

        let indent = vec![b' '; options.indent];
        let formatter = PrettyFormatter::with_indent(&indent);
        let mut serializer = serde_json::Serializer::with_formatter(&mut *output, formatter);
        let mut deserializer = serde_json::Deserializer::from_slice(trimmed);
        serde_transcode::transcode(&mut deserializer, &mut serializer)
            .with_context(|| format!("failed to transcode JSONL line {line_number}"))?;
        deserializer
            .end()
            .with_context(|| format!("trailing characters after JSONL line {line_number}"))?;
        writeln!(output)?;
    }

    Ok(())
}

fn trim_line_end(mut line: &[u8]) -> &[u8] {
    if line.ends_with(b"\n") {
        line = &line[..line.len() - 1];
    }
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
    line
}

fn format_xml_reader<R: BufRead, W: Write>(input: R, output: &mut W, indent: usize) -> Result<()> {
    let mut reader = XmlReader::from_reader(input);
    reader.config_mut().trim_text(false);
    let mut writer = XmlWriter::new_with_indent(&mut *output, b' ', indent);
    let mut buf = Vec::with_capacity(8192);

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(event) => writer
                .write_event(event)
                .context("failed to write XML event")?,
            Err(error) => return Err(anyhow!(error)),
        }
        buf.clear();
    }

    writeln!(output)?;
    Ok(())
}

fn write_json_value<W: Write>(value: &Value, output: &mut W, indent: usize) -> Result<()> {
    let indent = vec![b' '; indent];
    let formatter = PrettyFormatter::with_indent(&indent);
    let mut serializer = serde_json::Serializer::with_formatter(output, formatter);
    value
        .serialize(&mut serializer)
        .context("failed to write JSON")
}

fn expand_embedded_strings(value: &mut Value, indent: usize) {
    match value {
        Value::String(text) => {
            if let Some(expanded) = expand_string(text, indent) {
                *text = expanded;
            }
        }
        Value::Array(values) => {
            for value in values {
                expand_embedded_strings(value, indent);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                expand_embedded_strings(value, indent);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn expand_string(text: &str, indent: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with('<') {
        return pretty_xml_string(trimmed, indent).ok();
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let mut value: Value = serde_json::from_str(trimmed).ok()?;
        expand_embedded_strings(&mut value, indent);
        let mut out = Vec::new();
        write_json_value(&value, &mut out, indent).ok()?;
        return String::from_utf8(out).ok();
    }

    None
}

fn pretty_xml_string(text: &str, indent: usize) -> Result<String> {
    let mut out = Vec::new();
    format_xml_reader(Cursor::new(text.as_bytes()), &mut out, indent)?;
    let formatted = String::from_utf8(out).context("formatted XML was not UTF-8")?;
    Ok(formatted.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_crlf_line_endings() {
        assert_eq!(trim_line_end(b"{\"a\":1}\r\n"), b"{\"a\":1}");
    }

    #[test]
    fn expands_embedded_xml_string_when_requested() {
        let mut value = serde_json::json!({"xml": "<root><child>1</child></root>"});
        expand_embedded_strings(&mut value, 2);
        assert!(
            value["xml"]
                .as_str()
                .expect("xml value should stay a string")
                .contains("<child>1</child>")
        );
    }

    #[test]
    fn does_not_expand_invalid_xml_like_string() {
        let mut value = serde_json::json!({"xml": "<root>"});
        expand_embedded_strings(&mut value, 2);
        assert_eq!(value["xml"], "<root>");
    }

    #[test]
    fn preserves_empty_jsonl_lines() {
        let line = b"\n";
        assert!(trim_line_end(line).is_empty());
    }
}
