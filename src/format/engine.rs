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
