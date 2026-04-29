use ratatui::{style::Style, text::Span};

use crate::viewer::palette::{escape_style, plain_style, string_style};

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
