use ratatui::{style::Style, text::Span};

use super::super::{
    WRAP_GUTTER_MAJOR_TICK_ROWS, WRAP_GUTTER_MINOR_TICK_ROWS,
    palette::{gutter_style, plain_style},
};

pub(in crate::viewer) fn line_number_gutter(
    line_number: usize,
    gutter_digits: usize,
) -> Span<'static> {
    Span::styled(format!("{line_number:>gutter_digits$} │ "), gutter_style())
}

pub(in crate::viewer) fn continuation_gutter(
    row_index: usize,
    gutter_digits: usize,
) -> Span<'static> {
    let marker = continuation_gutter_marker(row_index);
    Span::styled(format!("{:>gutter_digits$} {marker} ", ""), gutter_style())
}

pub(in crate::viewer) fn continuation_gutter_marker(row_index: usize) -> char {
    if row_index > 0 && row_index % WRAP_GUTTER_MAJOR_TICK_ROWS == 0 {
        '┠'
    } else if row_index > 0 && row_index % WRAP_GUTTER_MINOR_TICK_ROWS == 0 {
        '┊'
    } else {
        '┆'
    }
}

pub(in crate::viewer) fn format_count(value: usize) -> String {
    let raw = value.to_string();
    let mut formatted = String::with_capacity(raw.len() + raw.len() / 3);
    let first_group = raw.len() % 3;
    for (index, ch) in raw.chars().enumerate() {
        if index > 0
            && (index == first_group || (index > first_group && (index - first_group) % 3 == 0))
        {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted
}

pub(in crate::viewer) fn slice_spans(
    spans: &[Span<'static>],
    start: usize,
    end: usize,
) -> Vec<Span<'static>> {
    if end <= start {
        return Vec::new();
    }

    let mut sliced = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = char_count(text);
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let overlap_start = start.max(span_start);
        let overlap_end = end.min(span_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = slice_chars(text, overlap_start - span_start, overlap_end - span_start);
        push_styled_span(&mut sliced, text, span.style);
    }

    sliced
}

pub(in crate::viewer) fn push_styled_span(
    spans: &mut Vec<Span<'static>>,
    text: String,
    style: Style,
) {
    let style = if style == plain_style() {
        Style::default()
    } else {
        style
    };

    if text.is_empty() {
        return;
    }

    if let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push_str(&text);
        return;
    }

    spans.push(Span::styled(text, style));
}

pub(in crate::viewer) fn slice_chars(text: &str, start: usize, end: usize) -> String {
    if end <= start {
        return String::new();
    }

    if text.is_ascii() {
        let start = start.min(text.len());
        let end = end.min(text.len());
        if end <= start {
            return String::new();
        }
        return text[start..end].to_owned();
    }

    text.chars().skip(start).take(end - start).collect()
}

pub(in crate::viewer) fn char_count(text: &str) -> usize {
    if text.is_ascii() {
        text.len()
    } else {
        text.chars().count()
    }
}

pub(in crate::viewer) fn byte_index_for_char(line: &str, char_index: usize) -> usize {
    if line.is_ascii() {
        return char_index.min(line.len());
    }

    line.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}
