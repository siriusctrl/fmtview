use ratatui::{style::Style, text::Span};

use crate::tui::palette::{escape_style, plain_style, string_style};

const JSON_VISIBLE_COMPOSITE_LANDMARK_LINES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructureCandidateKind {
    JsonRecordStart,
    JsonArrayItemStart,
    JsonCompositeField,
    JsonRootStart,
    XmlStartTag,
    MarkdownHeading,
    TomlTable,
    JinjaBlock,
    PlainParagraph,
}

impl StructureCandidateKind {
    pub(crate) fn is_landmark_when_visible(self, line_span: Option<usize>) -> bool {
        match self {
            StructureCandidateKind::JsonRecordStart
            | StructureCandidateKind::JsonArrayItemStart
            | StructureCandidateKind::JsonRootStart
            | StructureCandidateKind::MarkdownHeading
            | StructureCandidateKind::TomlTable
            | StructureCandidateKind::JinjaBlock
            | StructureCandidateKind::PlainParagraph => true,
            StructureCandidateKind::XmlStartTag => line_span.is_none_or(|span| span > 1),
            StructureCandidateKind::JsonCompositeField => {
                line_span.is_some_and(|span| span >= JSON_VISIBLE_COMPOSITE_LANDMARK_LINES)
            }
        }
    }
}

pub(crate) fn highlight_string_segment_window(
    source: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if end <= window_start {
        return;
    }

    let mut index = start;
    let mut plain_start = start;

    while index < end {
        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= end)
        {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            push_span_window(
                spans,
                source,
                index,
                escape_end,
                escape_style(),
                window_start,
                window_end,
            );
            index = escape_end;
            plain_start = index;
            continue;
        }

        let Some(ch) = source[index..end].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        end,
        string_style(),
        window_start,
        window_end,
    );
}

pub(crate) fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

pub(crate) fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

pub(crate) fn escaped_quoted_end(text: &str, start: usize, quote: char) -> usize {
    let pattern = if quote == '"' { "\\\"" } else { "\\'" };
    text[start + pattern.len()..]
        .find(pattern)
        .map(|offset| start + pattern.len() + offset + pattern.len())
        .unwrap_or(text.len())
}

pub(crate) fn escape_token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    if !rest.starts_with('\\') {
        return None;
    }

    let mut chars = rest.chars();
    chars.next()?;
    let escaped = chars.next()?;
    let escaped_start = start + '\\'.len_utf8();
    let escaped_end = escaped_start + escaped.len_utf8();

    if escaped == 'u' {
        let unicode_end = escaped_end + 4;
        if text
            .get(escaped_end..unicode_end)
            .is_some_and(|digits| digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        {
            return Some(unicode_end);
        }
    }

    Some(escaped_end)
}

pub(crate) fn push_span_window(
    spans: &mut Vec<Span<'static>>,
    source: &str,
    start: usize,
    end: usize,
    style: Style,
    window_start: usize,
    window_end: usize,
) {
    let start = floor_char_boundary(source, start.min(source.len()));
    let end = floor_char_boundary(source, end.min(source.len()));
    let window_start = floor_char_boundary(source, window_start.min(source.len()));
    let window_end = floor_char_boundary(source, window_end.min(source.len()));
    let overlap_start = start.max(window_start);
    let overlap_end = end.min(window_end);
    if overlap_start < overlap_end {
        let style = normalize_span_style(style);
        push_text_span(spans, source[overlap_start..overlap_end].to_owned(), style);
    }
}

pub(crate) fn leading_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(crate) fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}

pub(crate) fn max_observed_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    if lines.is_empty() || viewport_bottom < read_start {
        return None;
    }
    Some((viewport_bottom - read_start).min(lines.len() - 1))
}

pub(crate) fn max_boundary_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    max_observed_offset(lines, read_start, viewport_bottom.saturating_add(1))
}

pub(crate) fn following_lines(
    lines: &[String],
    start_offset: usize,
    max_offset: usize,
) -> impl Iterator<Item = (usize, &String)> {
    lines
        .iter()
        .enumerate()
        .take(max_offset + 1)
        .skip(start_offset + 1)
}

pub(crate) fn eof_block_end(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    if !line_count_exact || line_count == 0 {
        return None;
    }
    let eof_line = line_count - 1;
    let read_end = read_start.saturating_add(lines.len());
    (eof_line <= viewport_bottom && eof_line < read_end).then_some(eof_line)
}

pub(crate) fn indent_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_indent = leading_indent(lines.get(start_offset)?);
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_indent(line) <= start_indent {
            if is_same_indent_closing_line(line) {
                return Some(read_start + offset);
            }
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn is_same_indent_closing_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.starts_with("</")
        || crate::formats::jinja::structure::keyword(trimmed)
            .is_some_and(|keyword| keyword.starts_with("end"))
}

pub(crate) fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn normalize_span_style(style: Style) -> Style {
    if style == plain_style() {
        Style::default()
    } else {
        style
    }
}

fn push_text_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if text.is_empty() {
        return;
    }

    if style == Style::default()
        && let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push_str(&text);
        return;
    }

    spans.push(Span::styled(text, style));
}
