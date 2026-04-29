use std::{
    ffi::OsStr,
    io::{BufRead, BufReader},
};

use anyhow::{Context, Result};

use crate::{
    input::InputSource,
    load::LoadPlan,
    syntax::SyntaxKind,
    transform::{self, FormatKind, FormatOptions, TransformStrategy},
};

const SNIFF_BYTES: usize = 1024 * 1024;
const SNIFF_LINES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypeProfile {
    pub(crate) content: FormatKind,
    pub(crate) load: LoadPlan,
    pub(crate) transform: TransformStrategy,
    pub(crate) syntax: SyntaxKind,
}

impl TypeProfile {
    pub(crate) fn resolve(source: &InputSource, options: &FormatOptions) -> Result<Self> {
        if options.kind != FormatKind::Auto {
            return Ok(explicit_profile(options.kind));
        }

        if let Some(kind) = extension_kind(source) {
            return Ok(explicit_profile(kind));
        }

        let sample = TypeSample::read(source)?;
        if sample.looks_like_record_stream() {
            return Ok(explicit_profile(FormatKind::Jsonl));
        }

        Ok(match sample.first_non_ws {
            Some(b'<') => explicit_profile(FormatKind::Xml),
            Some(b'{' | b'[') => explicit_profile(FormatKind::Json),
            _ => TypeProfile {
                content: FormatKind::Jsonl,
                load: LoadPlan::EagerDocument,
                transform: TransformStrategy::RecordPrettyPrint,
                syntax: SyntaxKind::Structured,
            },
        })
    }

    pub(crate) fn format_options(self, indent: usize) -> FormatOptions {
        FormatOptions {
            kind: self.content,
            indent,
        }
    }
}

fn explicit_profile(kind: FormatKind) -> TypeProfile {
    match kind {
        FormatKind::Auto => unreachable!("auto must be resolved before building a type profile"),
        FormatKind::Json => TypeProfile {
            content: FormatKind::Json,
            load: LoadPlan::EagerDocument,
            transform: TransformStrategy::PrettyPrint,
            syntax: SyntaxKind::Structured,
        },
        FormatKind::Jsonl => TypeProfile {
            content: FormatKind::Jsonl,
            load: LoadPlan::LazyRecords,
            transform: TransformStrategy::RecordPrettyPrint,
            syntax: SyntaxKind::Structured,
        },
        FormatKind::Xml => TypeProfile {
            content: FormatKind::Xml,
            load: LoadPlan::EagerDocument,
            transform: TransformStrategy::PrettyPrint,
            syntax: SyntaxKind::Structured,
        },
        FormatKind::Plain => TypeProfile {
            content: FormatKind::Plain,
            load: LoadPlan::RawIndexedText,
            transform: TransformStrategy::Passthrough,
            syntax: SyntaxKind::Plain,
        },
        FormatKind::Jinja => TypeProfile {
            content: FormatKind::Jinja,
            load: LoadPlan::RawIndexedText,
            transform: TransformStrategy::Passthrough,
            syntax: SyntaxKind::Jinja,
        },
    }
}

fn extension_kind(source: &InputSource) -> Option<FormatKind> {
    match source
        .path()
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => Some(FormatKind::Json),
        Some("jsonl" | "ndjson") => Some(FormatKind::Jsonl),
        Some("xml" | "html" | "htm" | "xhtml") => Some(FormatKind::Xml),
        Some("txt" | "text" | "log") => Some(FormatKind::Plain),
        Some("j2" | "jinja" | "jinja2") => Some(FormatKind::Jinja),
        _ => None,
    }
}

#[derive(Default)]
struct TypeSample {
    first_non_ws: Option<u8>,
    non_empty_lines: usize,
    parseable_record_lines: usize,
}

impl TypeSample {
    fn read(source: &InputSource) -> Result<Self> {
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

    fn looks_like_record_stream(&self) -> bool {
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::{Builder as TempFileBuilder, NamedTempFile};

    use super::*;

    fn source_with_suffix(contents: &[u8], suffix: &str) -> (NamedTempFile, InputSource) {
        let mut temp = TempFileBuilder::new().suffix(suffix).tempfile().unwrap();
        temp.write_all(contents).unwrap();
        temp.flush().unwrap();
        let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
        (temp, source)
    }

    #[test]
    fn resolves_plain_extension_to_passthrough_profile() {
        let (_temp, source) = source_with_suffix(b"plain\n", ".txt");
        let profile = TypeProfile::resolve(
            &source,
            &FormatOptions {
                kind: FormatKind::Auto,
                indent: 2,
            },
        )
        .unwrap();

        assert_eq!(profile.content, FormatKind::Plain);
        assert_eq!(profile.load, LoadPlan::RawIndexedText);
        assert_eq!(profile.transform, TransformStrategy::Passthrough);
        assert_eq!(profile.syntax, SyntaxKind::Plain);
    }

    #[test]
    fn resolves_jinja_extension_to_template_profile() {
        let (_temp, source) = source_with_suffix(b"<h1>{{ title }}</h1>\n", ".html.j2");
        let profile = TypeProfile::resolve(
            &source,
            &FormatOptions {
                kind: FormatKind::Auto,
                indent: 2,
            },
        )
        .unwrap();

        assert_eq!(profile.content, FormatKind::Jinja);
        assert_eq!(profile.load, LoadPlan::RawIndexedText);
        assert_eq!(profile.transform, TransformStrategy::Passthrough);
        assert_eq!(profile.syntax, SyntaxKind::Jinja);
    }

    #[test]
    fn resolves_record_stream_to_lazy_jsonl_profile() {
        let (_temp, source) = source_with_suffix(b"{\"a\":1}\n{\"b\":2}\n", ".data");
        let profile = TypeProfile::resolve(
            &source,
            &FormatOptions {
                kind: FormatKind::Auto,
                indent: 2,
            },
        )
        .unwrap();

        assert_eq!(profile.content, FormatKind::Jsonl);
        assert_eq!(profile.load, LoadPlan::LazyRecords);
        assert_eq!(profile.transform, TransformStrategy::RecordPrettyPrint);
        assert_eq!(profile.syntax, SyntaxKind::Structured);
    }
}
