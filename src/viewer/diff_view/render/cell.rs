use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::syntax::{SyntaxKind, highlight_content_window};

use super::super::super::{
    palette::{gutter_style, plain_style},
    render::{
        byte_index_for_char, char_count, continuation_indent, wrap_ranges_window_indexed,
        wrapped_row_count,
    },
};
use super::styles::{DiffCellStyle, push_diff_span_segments, slice_char_range};

pub(super) struct NumberedCell<'a> {
    pub(super) number: Option<usize>,
    pub(super) digits: usize,
    pub(super) content: Option<&'a str>,
    pub(super) diff_style: Option<DiffCellStyle>,
}

impl NumberedCell<'_> {
    fn bg_style(&self) -> Option<Style> {
        self.diff_style.map(|style| style.line_style())
    }

    fn marker(&self) -> char {
        self.diff_style
            .filter(|_| self.content.is_some())
            .map(|style| style.marker())
            .unwrap_or(' ')
    }

    fn marker_style(&self) -> Style {
        self.diff_style
            .filter(|_| self.content.is_some())
            .map(|style| style.marker_style())
            .unwrap_or_else(gutter_style)
    }
}

pub(super) fn push_numbered_cell(
    spans: &mut Vec<Span<'static>>,
    cell: NumberedCell<'_>,
    content_width: usize,
    x: usize,
) {
    let bg_style = cell.bg_style();
    push_number(spans, cell.number, cell.digits, bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);
    push_styled_text(
        spans,
        &cell.marker().to_string(),
        cell.marker_style(),
        bg_style,
    );
    push_styled_text(spans, " ", gutter_style(), bg_style);

    let written = cell
        .content
        .map(|content| push_structured_content(spans, content, x, content_width, cell.diff_style))
        .unwrap_or(0);
    fill_row(spans, content_width.saturating_sub(written), bg_style);
}

pub(super) fn push_numbered_cell_window(
    spans: &mut Vec<Span<'static>>,
    cell: NumberedCell<'_>,
    content_width: usize,
    row_offset: usize,
) {
    let bg_style = cell.bg_style();
    if row_offset == 0 {
        push_number(spans, cell.number, cell.digits, bg_style);
        push_styled_text(spans, " ", gutter_style(), bg_style);
        push_styled_text(
            spans,
            &cell.marker().to_string(),
            cell.marker_style(),
            bg_style,
        );
        push_styled_text(spans, " ", gutter_style(), bg_style);
    } else {
        push_styled_text(spans, &" ".repeat(cell.digits), gutter_style(), bg_style);
        push_styled_text(spans, "   ", gutter_style(), bg_style);
    }

    let Some(content) = cell.content else {
        fill_row(spans, content_width, bg_style);
        return;
    };
    push_wrapped_content_slice(
        spans,
        content,
        content_width,
        row_offset,
        cell.diff_style,
        bg_style,
        true,
    );
}

pub(super) fn wrapped_content_visual_count(content: Option<&str>, width: usize) -> usize {
    content
        .map(|content| {
            let width = width.max(1);
            wrapped_row_count(content, width, continuation_indent(content, width))
        })
        .unwrap_or(1)
}

pub(super) fn push_number(
    spans: &mut Vec<Span<'static>>,
    number: Option<usize>,
    digits: usize,
    bg_style: Option<Style>,
) {
    let text = number
        .map(|number| format!("{number:>digits$}"))
        .unwrap_or_else(|| " ".repeat(digits));
    push_styled_text(spans, &text, gutter_style(), bg_style);
}

pub(super) fn push_structured_content(
    spans: &mut Vec<Span<'static>>,
    content: &str,
    x: usize,
    width: usize,
    diff_style: Option<DiffCellStyle>,
) -> usize {
    if width == 0 {
        return 0;
    }

    let start = byte_index_for_char(content, x);
    let end = byte_index_for_char(content, x.saturating_add(width));
    let highlighted = highlight_content_window(content, SyntaxKind::Structured, start, end);
    let mut cursor = x;
    let mut written = 0_usize;
    for span in highlighted {
        let text = span.content.as_ref();
        let count = char_count(text);
        push_diff_span_segments(spans, text, cursor, span.style, diff_style);
        cursor = cursor.saturating_add(count);
        written = written.saturating_add(count);
    }
    written
}

pub(super) fn push_structured_content_range(
    spans: &mut Vec<Span<'static>>,
    content: &str,
    start_char: usize,
    end_char: usize,
    start_byte: usize,
    end_byte: usize,
    diff_style: Option<DiffCellStyle>,
) -> usize {
    if start_char >= end_char {
        return 0;
    }

    let highlighted =
        highlight_content_window(content, SyntaxKind::Structured, start_byte, end_byte);
    let mut cursor = start_char;
    let mut written = 0_usize;
    for span in highlighted {
        let text = span.content.as_ref();
        let count = char_count(text);
        push_diff_span_segments(spans, text, cursor, span.style, diff_style);
        cursor = cursor.saturating_add(count);
        written = written.saturating_add(count);
    }
    written
}

pub(super) fn push_wrapped_content_slice(
    spans: &mut Vec<Span<'static>>,
    content: &str,
    content_width: usize,
    row_offset: usize,
    diff_style: Option<DiffCellStyle>,
    bg_style: Option<Style>,
    fill: bool,
) -> usize {
    let content_width = content_width.max(1);
    let indent = continuation_indent(content, content_width);
    let wrap_window =
        wrap_ranges_window_indexed(content, content_width, indent, row_offset, 1, None);
    let Some(range) = wrap_window.ranges.first() else {
        if fill {
            fill_row(spans, content_width, bg_style);
        }
        return 0;
    };
    if range.continuation_indent > 0 {
        push_styled_text(
            spans,
            &" ".repeat(range.continuation_indent),
            plain_style(),
            bg_style,
        );
    }
    let written = range
        .continuation_indent
        .saturating_add(push_structured_content_range(
            spans,
            content,
            range.start_char,
            range.end_char,
            range.start_byte,
            range.end_byte,
            diff_style,
        ));
    if fill {
        fill_row(spans, content_width.saturating_sub(written), bg_style);
    }
    written
}

pub(super) fn push_styled_text(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    style: Style,
    bg_style: Option<Style>,
) {
    let style = bg_style
        .map(|bg_style| style.patch(bg_style))
        .unwrap_or(style);
    spans.push(Span::styled(text.to_owned(), style));
}

pub(super) fn fill_row(spans: &mut Vec<Span<'static>>, count: usize, bg_style: Option<Style>) {
    if count == 0 {
        return;
    }

    spans.push(Span::styled(
        " ".repeat(count),
        bg_style.unwrap_or_default(),
    ));
}

pub(super) fn styled_text_line(text: &str, width: usize, style: Style) -> Line<'static> {
    let end = byte_index_for_char(text, width);
    Line::from(vec![Span::styled(text[..end].to_owned(), style)])
}

pub(super) fn render_message_window(
    text: &str,
    row_offset: usize,
    height: usize,
    width: usize,
    style: Style,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let window = wrap_ranges_window_indexed(text, width, 0, row_offset, height, None);
    window
        .ranges
        .iter()
        .map(|range| {
            Line::from(vec![Span::styled(
                slice_char_range(text, range.start_char, range.end_char),
                style,
            )])
        })
        .collect()
}
