use std::{
    ffi::OsStr,
    io::{BufReader, Read},
};

use anyhow::{Context, Result};

use crate::{formats, input::InputSource};

use super::types::{FormatKind, FormatOptions};

pub(super) fn candidate_kinds(
    source: &InputSource,
    options: &FormatOptions,
) -> Result<Vec<FormatKind>> {
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
    if let Some(kind) = source
        .path()
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
        .and_then(formats::kind_for_extension)
    {
        return Ok(kind);
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
        _ => Ok(FormatKind::Plain),
    }
}
