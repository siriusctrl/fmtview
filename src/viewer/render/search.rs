use std::ops::Range;

use ratatui::text::{Line, Span};

use super::super::palette::search_match_bg;
use super::text::{char_count, push_styled_span, slice_chars};

pub(in crate::viewer) fn apply_search_highlight(
    line: Line<'static>,
    query: Option<&str>,
    gutter_digits: usize,
) -> Line<'static> {
    let Some(query) = query else {
        return line;
    };
    if query.is_empty() {
        return line;
    }

    let visual_text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let ranges = search_match_ranges(&visual_text, query, gutter_digits + 3);
    if ranges.is_empty() {
        return line;
    }

    Line {
        style: line.style,
        alignment: line.alignment,
        spans: apply_search_ranges_to_spans(&line.spans, &ranges),
    }
}

pub(in crate::viewer) fn search_match_ranges(
    text: &str,
    query: &str,
    start_char: usize,
) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let total_chars = char_count(text);
    if start_char >= total_chars {
        return Vec::new();
    }

    let search_text = slice_chars(text, start_char, total_chars);
    let query_len = char_count(query);
    search_text
        .match_indices(query)
        .map(|(byte_index, _)| {
            let start = start_char + char_count(&search_text[..byte_index]);
            start..start + query_len
        })
        .collect()
}

pub(in crate::viewer) fn apply_search_ranges_to_spans(
    spans: &[Span<'static>],
    ranges: &[Range<usize>],
) -> Vec<Span<'static>> {
    let mut highlighted = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = char_count(text);
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let split_points = search_split_points(span_start, span_end, ranges);
        for window in split_points.windows(2) {
            let start = window[0];
            let end = window[1];
            if start == end {
                continue;
            }

            let mut style = span.style;
            if range_is_highlighted(start, end, ranges) {
                style = style.bg(search_match_bg());
            }
            push_styled_span(
                &mut highlighted,
                slice_chars(text, start - span_start, end - span_start),
                style,
            );
        }
    }

    highlighted
}

pub(in crate::viewer) fn search_split_points(
    span_start: usize,
    span_end: usize,
    ranges: &[Range<usize>],
) -> Vec<usize> {
    let mut points = vec![span_start, span_end];
    for range in ranges {
        let start = range.start.max(span_start).min(span_end);
        let end = range.end.max(span_start).min(span_end);
        if start < end {
            points.push(start);
            points.push(end);
        }
    }
    points.sort_unstable();
    points.dedup();
    points
}

pub(in crate::viewer) fn range_is_highlighted(
    start: usize,
    end: usize,
    ranges: &[Range<usize>],
) -> bool {
    ranges
        .iter()
        .any(|range| start >= range.start && end <= range.end)
}
