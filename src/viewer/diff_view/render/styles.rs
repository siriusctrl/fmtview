use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

use crate::diff::{DiffChange, DiffRange};

use super::super::super::{
    palette::{
        diff_added_inline_bg, diff_added_line_bg, diff_added_style, diff_removed_inline_bg,
        diff_removed_line_bg, diff_removed_style,
    },
    render::char_count,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct DiffCellStyle {
    pub(super) side: DiffSide,
    pub(super) change: DiffChange,
}

impl DiffCellStyle {
    pub(super) fn line_style(self) -> Style {
        Style::default().bg(self.line_bg())
    }

    fn inline_style(self) -> Style {
        Style::default()
            .bg(self.inline_bg())
            .add_modifier(Modifier::BOLD)
    }

    pub(super) fn marker(self) -> char {
        match self.side {
            DiffSide::Removed => '-',
            DiffSide::Added => '+',
        }
    }

    pub(super) fn marker_style(self) -> Style {
        match self.side {
            DiffSide::Removed => diff_removed_style(),
            DiffSide::Added => diff_added_style(),
        }
    }

    fn range(self) -> Option<DiffRange> {
        match self.side {
            DiffSide::Removed => self.change.left_range,
            DiffSide::Added => self.change.right_range,
        }
    }

    fn line_bg(self) -> Color {
        match self.side {
            DiffSide::Removed => diff_removed_line_bg(self.change.intensity),
            DiffSide::Added => diff_added_line_bg(self.change.intensity),
        }
    }

    fn inline_bg(self) -> Color {
        match self.side {
            DiffSide::Removed => diff_removed_inline_bg(self.change.intensity),
            DiffSide::Added => diff_added_inline_bg(self.change.intensity),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DiffSide {
    Removed,
    Added,
}

pub(super) fn push_diff_span_segments(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    start_char: usize,
    base_style: Style,
    diff_style: Option<DiffCellStyle>,
) {
    let Some(diff_style) = diff_style else {
        spans.push(Span::styled(text.to_owned(), base_style));
        return;
    };

    let line_style = diff_style.line_style();
    let Some(range) = diff_style.range() else {
        spans.push(Span::styled(text.to_owned(), base_style.patch(line_style)));
        return;
    };
    let text_len = char_count(text);
    let end_char = start_char.saturating_add(text_len);
    if range.end <= start_char || range.start >= end_char {
        spans.push(Span::styled(text.to_owned(), base_style.patch(line_style)));
        return;
    }
    if range.start <= start_char && range.end >= end_char {
        spans.push(Span::styled(
            text.to_owned(),
            base_style
                .patch(line_style)
                .patch(diff_style.inline_style()),
        ));
        return;
    }

    let before_end = range.start.saturating_sub(start_char).min(text_len);
    let inline_start = before_end;
    let inline_end = range.end.saturating_sub(start_char).min(text_len);
    push_optional_segment(spans, text, 0, before_end, base_style.patch(line_style));
    push_optional_segment(
        spans,
        text,
        inline_start,
        inline_end,
        base_style
            .patch(line_style)
            .patch(diff_style.inline_style()),
    );
    push_optional_segment(
        spans,
        text,
        inline_end,
        text_len,
        base_style.patch(line_style),
    );
}

fn push_optional_segment(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    start: usize,
    end: usize,
    style: Style,
) {
    if start >= end {
        return;
    }

    spans.push(Span::styled(slice_char_range(text, start, end), style));
}

pub(super) fn slice_char_range(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}
