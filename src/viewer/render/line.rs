use ratatui::text::{Line, Span};

use super::super::{
    ViewMode,
    highlight::{highlight_content, highlight_content_window_indexed},
    palette::plain_style,
};
use super::{
    cache::{LineRenderIndex, RenderedVisualRow},
    text::{
        byte_index_for_char, char_count, continuation_gutter, line_number_gutter, push_styled_span,
        slice_chars, slice_spans,
    },
    types::RenderContext,
    wrap::{continuation_indent, next_wrap_end, wrap_ranges_window_indexed},
};

#[cfg(test)]
pub(in crate::viewer) fn render_logical_line(
    line: &str,
    line_number: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window(line, line_number, 0, max_rows, context)
}

#[derive(Debug)]
pub(in crate::viewer) struct RenderedLineWindow {
    pub(in crate::viewer) rows: Vec<RenderedVisualRow>,
    pub(in crate::viewer) total_rows: Option<usize>,
}

#[cfg(test)]
pub(in crate::viewer) fn render_logical_line_window(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window_with_status(line, line_number, row_start, max_rows, context)
        .rows
        .into_iter()
        .map(|row| row.line)
        .collect()
}

#[cfg(test)]
pub(in crate::viewer) fn render_logical_line_window_with_status(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> RenderedLineWindow {
    let mut index = LineRenderIndex::default();
    render_logical_line_window_with_status_indexed(
        line,
        line_number,
        row_start,
        max_rows,
        context,
        &mut index,
    )
}

pub(in crate::viewer) fn render_logical_line_window_with_status_indexed(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
    index: &mut LineRenderIndex,
) -> RenderedLineWindow {
    if max_rows == 0 {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows: None,
        };
    }

    if !context.wrap {
        if row_start > 0 {
            return RenderedLineWindow {
                rows: Vec::new(),
                total_rows: Some(1),
            };
        }
        let line_chars = char_count(line);
        return RenderedLineWindow {
            rows: vec![RenderedVisualRow {
                line: styled_segment(
                    line_number_gutter(line_number, context.gutter_digits),
                    line,
                    context.x,
                    context.x.saturating_add(context.width),
                    context.mode,
                ),
                end_byte: byte_index_for_char(
                    line,
                    context.x.saturating_add(context.width).min(line_chars),
                ),
                line_end: context.x.saturating_add(context.width) >= line_chars,
            }],
            total_rows: Some(1),
        };
    }

    let wrap_window = wrap_ranges_window_indexed(
        line,
        context.width,
        continuation_indent(line, context.width),
        row_start,
        max_rows,
        Some(&mut index.wrap),
    );
    let visible_ranges = wrap_window.ranges;
    let total_rows = wrap_window.total_rows;
    let highlight_end_byte = visible_ranges
        .iter()
        .map(|range| range.end_byte)
        .max()
        .unwrap_or(0);
    let Some(first_range) = visible_ranges.first() else {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows,
        };
    };
    let highlight_start_byte = first_range.start_byte;
    let highlight_start_char = first_range.start_char;
    let spans = highlight_content_window_indexed(
        line,
        context.mode,
        highlight_start_byte,
        highlight_end_byte,
        Some(&mut index.highlight),
    );
    let rows = visible_ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let row_index = row_start + index;
            let gutter = if row_index == 0 {
                line_number_gutter(line_number, context.gutter_digits)
            } else {
                continuation_gutter(row_index, context.gutter_digits)
            };
            let mut line_spans = vec![gutter];
            if range.continuation_indent > 0 {
                push_styled_span(
                    &mut line_spans,
                    " ".repeat(range.continuation_indent),
                    plain_style(),
                );
            }
            line_spans.extend(slice_spans(
                &spans,
                range.start_char - highlight_start_char,
                range.end_char - highlight_start_char,
            ));
            RenderedVisualRow {
                line: Line::from(line_spans),
                end_byte: range.end_byte,
                line_end: range.end_byte >= line.len(),
            }
        })
        .collect();
    RenderedLineWindow { rows, total_rows }
}

pub(in crate::viewer) fn rendered_row_count(line: &str, context: RenderContext) -> usize {
    if !context.wrap {
        return 1;
    }

    wrapped_row_count(
        line,
        context.width,
        continuation_indent(line, context.width),
    )
}

pub(in crate::viewer) fn wrapped_row_count(
    line: &str,
    width: usize,
    continuation_indent: usize,
) -> usize {
    if line.is_empty() || width == 0 {
        return 1;
    }

    let mut rows = 0_usize;
    let mut start_byte = 0_usize;
    let mut start_char = 0_usize;
    while start_byte < line.len() {
        let continuation = rows > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        rows = rows.saturating_add(1);
    }

    rows
}

pub(in crate::viewer) fn styled_segment(
    gutter: Span<'static>,
    line: &str,
    start: usize,
    end: usize,
    mode: ViewMode,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(gutter);
    let highlight_prefix = slice_chars(line, 0, end);
    spans.extend(slice_spans(
        &highlight_content(&highlight_prefix, mode),
        start,
        end,
    ));
    Line::from(spans)
}
