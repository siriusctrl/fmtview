use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, BufWriter, Read, Write},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use quick_xml::{Reader as XmlReader, Writer as XmlWriter, events::Event};
use serde_json::ser::PrettyFormatter;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_crlf_line_endings() {
        assert_eq!(trim_line_end(b"{\"a\":1}\r\n"), b"{\"a\":1}");
    }

    #[test]
    fn preserves_empty_jsonl_lines() {
        let line = b"\n";
        assert!(trim_line_end(line).is_empty());
    }
}
