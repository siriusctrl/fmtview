mod checkpoints;
pub(crate) mod html;
pub(crate) mod jinja;
pub(crate) mod json;
pub(crate) mod jsonl;
pub(crate) mod markdown;
pub(crate) mod plain;
mod shared;
pub(crate) mod toml;
pub(crate) mod xml;

pub(crate) use checkpoints::HighlightCheckpointIndex;
pub(crate) use shared::{
    StructureCandidateKind, detect_markup_kind, first_non_ws_byte, leading_indent,
};

#[cfg(test)]
pub(crate) use json::highlight::highlight_json_like;
#[cfg(test)]
pub(crate) use xml::highlight::highlight_xml_line;

use ratatui::text::Span;

use crate::{
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

pub(crate) const FORMAT_SPECS: &[FormatSpec] = &[
    json::SPEC,
    jsonl::SPEC,
    xml::SPEC,
    html::SPEC,
    toml::SPEC,
    markdown::SPEC,
    plain::SPEC,
    jinja::SPEC,
];

pub(crate) fn kind_for_extension(extension: &str) -> Option<FormatKind> {
    FORMAT_SPECS
        .iter()
        .find(|spec| spec.extensions.contains(&extension))
        .map(|spec| spec.kind)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentShape {
    LineIndexed,
    RecordStream,
    WholeDocument,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FormatSpec {
    pub(crate) kind: FormatKind,
    pub(crate) extensions: &'static [&'static str],
    pub(crate) shape: ContentShape,
    pub(crate) load: LoadPlan,
    pub(crate) transform: TransformStrategy,
}

pub(crate) fn highlight_content(line: &str, format: FormatKind) -> Vec<Span<'static>> {
    highlight_content_window(line, format, 0, line.len())
}

pub(crate) fn highlight_content_window(
    line: &str,
    format: FormatKind,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    highlight_content_window_indexed(line, format, window_start, window_end, None)
}

pub(crate) fn highlight_content_window_indexed(
    line: &str,
    format: FormatKind,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let window_start = window_start.min(line.len());
    let window_end = window_end.min(line.len()).max(window_start);
    match format {
        FormatKind::Json | FormatKind::Jsonl => {
            json::highlight::highlight_json_like_window(line, window_start, window_end, index)
        }
        FormatKind::Xml | FormatKind::Html => {
            xml::highlight::highlight_xml_line_window(line, window_start, window_end, index)
        }
        FormatKind::Toml => {
            toml::highlight::highlight_toml_line_window(line, window_start, window_end, index)
        }
        FormatKind::Markdown => markdown::highlight::highlight_markdown_line_window(
            line,
            window_start,
            window_end,
            index,
        ),
        FormatKind::Jinja => {
            jinja::highlight::highlight_jinja_line_window(line, window_start, window_end, index)
        }
        FormatKind::Plain | FormatKind::Auto => {
            plain::highlight::highlight_plain_window(line, window_start, window_end)
        }
    }
}

pub(crate) fn highlight_structured_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        xml::highlight::highlight_xml_line_window(line, window_start, window_end, index)
    } else {
        json::highlight::highlight_json_like_window(line, window_start, window_end, index)
    }
}

pub(crate) fn structure_anchor(
    lines: &[String],
    read_start: usize,
    line: usize,
    format: FormatKind,
) -> Option<StructureAnchor> {
    let offset = line.checked_sub(read_start)?;
    let content = lines.get(offset)?;
    let previous = offset
        .checked_sub(1)
        .and_then(|previous| lines.get(previous).map(String::as_str));
    Some(StructureAnchor {
        line,
        kind: structure_candidate_kind_in_window(format, lines, offset)
            .or_else(|| structure_candidate_kind(format, content, previous)),
        indent: leading_indent(content),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StructureAnchor {
    pub(crate) line: usize,
    pub(crate) kind: Option<StructureCandidateKind>,
    pub(crate) indent: usize,
}

pub(crate) fn structure_candidate_kind(
    format: FormatKind,
    line: &str,
    previous_line: Option<&str>,
) -> Option<StructureCandidateKind> {
    match format {
        FormatKind::Json | FormatKind::Jsonl => json::structure::candidate_kind(line),
        FormatKind::Xml | FormatKind::Html => xml::structure::is_start_tag(line.trim_start())
            .then_some(StructureCandidateKind::XmlStartTag),
        FormatKind::Markdown => {
            markdown::structure::is_heading(line).then_some(StructureCandidateKind::MarkdownHeading)
        }
        FormatKind::Toml => {
            toml::structure::is_table(line).then_some(StructureCandidateKind::TomlTable)
        }
        FormatKind::Jinja => {
            jinja::structure::is_block(line).then_some(StructureCandidateKind::JinjaBlock)
        }
        FormatKind::Plain | FormatKind::Auto => {
            plain::structure::is_paragraph_start(line, previous_line)
                .then_some(StructureCandidateKind::PlainParagraph)
        }
    }
}

pub(crate) fn structure_candidate_kind_in_window(
    format: FormatKind,
    lines: &[String],
    offset: usize,
) -> Option<StructureCandidateKind> {
    match format {
        FormatKind::Json | FormatKind::Jsonl => {
            json::structure::candidate_kind_in_window(lines, offset)
        }
        // Xml and Html share the same windowless candidate detection below.
        _ => {
            let line = lines.get(offset).map(String::as_str).unwrap_or_default();
            let previous = lines
                .get(offset.saturating_sub(1))
                .map(String::as_str)
                .filter(|_| offset > 0);
            structure_candidate_kind(format, line, previous)
        }
    }
}

pub(crate) fn structure_block_end(
    format: FormatKind,
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    match format {
        FormatKind::Json | FormatKind::Jsonl => {
            json::structure::block_end(lines, read_start, start_offset, viewport_bottom)
        }
        FormatKind::Xml | FormatKind::Html => {
            xml::structure::block_end(lines, read_start, start_offset, viewport_bottom).or_else(
                || shared::indent_block_end(lines, read_start, start_offset, viewport_bottom),
            )
        }
        FormatKind::Markdown => {
            markdown::structure::block_end(lines, read_start, start_offset, viewport_bottom)
        }
        FormatKind::Toml => {
            toml::structure::block_end(lines, read_start, start_offset, viewport_bottom)
        }
        FormatKind::Jinja => {
            jinja::structure::block_end(lines, read_start, start_offset, viewport_bottom)
        }
        FormatKind::Plain | FormatKind::Auto => {
            plain::structure::block_end(lines, read_start, start_offset, viewport_bottom)
        }
    }
    .or_else(|| {
        shared::eof_block_end(
            lines,
            read_start,
            viewport_bottom,
            line_count,
            line_count_exact,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::transform::FormatKind;

    use super::*;

    #[test]
    #[ignore = "performance smoke; run benches/syntax-performance.sh"]
    fn perf_format_highlight_window() {
        let repeated = r#"<item id=\"1\"><name>visible</name></item>""#.repeat(32_768);
        let line = format!(r#"  "message": "{repeated}""#);
        let window_width = 8 * 1024;
        let mut checkpoints = HighlightCheckpointIndex::default();
        let started = Instant::now();
        let mut spans = 0_usize;

        for start in (0..line.len()).step_by(window_width) {
            let end = start.saturating_add(window_width).min(line.len());
            spans = spans.saturating_add(
                highlight_content_window_indexed(
                    &line,
                    FormatKind::Json,
                    start,
                    end,
                    Some(&mut checkpoints),
                )
                .len(),
            );
        }

        let elapsed = started.elapsed();
        eprintln!(
            "format highlight window: {elapsed:?}, windows={}, input_bytes={}, spans={spans}",
            line.len().div_ceil(window_width),
            line.len()
        );
        assert!(spans > 0);
        assert!(
            elapsed < Duration::from_secs(5),
            "format highlight window took {elapsed:?}"
        );
    }

    #[test]
    fn plain_highlight_preserves_visible_text() {
        let text = "plain {{ not special }} <not-a-tag>";
        let spans = highlight_content(text, FormatKind::Plain);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn jinja_highlight_preserves_visible_text() {
        let text = r#"<h1>{{ title }}</h1>{% if ok %}{# comment #}{% endif %}"#;
        let spans = highlight_content(text, FormatKind::Jinja);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn toml_highlight_preserves_visible_text() {
        let text = r#"database.port = 5432 # local port"#;
        let spans = highlight_content(text, FormatKind::Toml);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn markdown_highlight_preserves_visible_text() {
        let text = r#"## Title with `code` and [link](https://example.com)"#;
        let spans = highlight_content(text, FormatKind::Markdown);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn markdown_fence_maps_known_languages_to_existing_formats() {
        let lines = vec![
            "```json".to_owned(),
            r#"{"ok": true}"#.to_owned(),
            "```".to_owned(),
            "```toml".to_owned(),
            "ok = true".to_owned(),
            "```".to_owned(),
            "```jinja".to_owned(),
            "{{ value }}".to_owned(),
            "```".to_owned(),
            "```unknown".to_owned(),
            "still code".to_owned(),
            "```".to_owned(),
        ];

        let modes = markdown::highlight::markdown_line_formats(
            &lines,
            markdown::highlight::MarkdownFenceState::default(),
        );

        assert_eq!(
            modes,
            vec![
                FormatKind::Markdown,
                FormatKind::Json,
                FormatKind::Markdown,
                FormatKind::Markdown,
                FormatKind::Toml,
                FormatKind::Markdown,
                FormatKind::Markdown,
                FormatKind::Jinja,
                FormatKind::Markdown,
                FormatKind::Markdown,
                FormatKind::Plain,
                FormatKind::Markdown,
            ]
        );
    }

    #[test]
    fn jinja_highlight_handles_windowed_tokens() {
        let text = r#"<div class="item">{{ item.name }}</div>"#;
        let start = text.find("{{").unwrap();
        let spans = highlight_content_window(text, FormatKind::Jinja, start, text.len());
        assert_eq!(span_text(&spans), r#"{{ item.name }}</div>"#);
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
