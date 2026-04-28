use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::diff::{DiffModel, UnifiedDiffRow};

use super::super::super::{
    palette::{diff_added_style, diff_removed_style, gutter_style, plain_style},
    render::wrapped_row_count,
};
use super::styles::DiffSide;
use super::{
    DiffCellStyle, fill_row, push_number, push_structured_content, push_styled_text,
    push_wrapped_content_slice, render_message_window, styled_text_line,
    wrapped_content_visual_count,
};

fn render_unified_row(
    row: &UnifiedDiffRow,
    model: &DiffModel,
    width: usize,
    x: usize,
) -> Line<'static> {
    match row {
        UnifiedDiffRow::Message { text } => styled_text_line(text, width, plain_style()),
        UnifiedDiffRow::Context {
            left,
            right,
            content,
        } => render_unified_content(
            Some(*left),
            Some(*right),
            ' ',
            gutter_style(),
            content,
            model,
            width,
            x,
            None,
        ),
        UnifiedDiffRow::Delete {
            left,
            content,
            change,
        } => render_unified_content(
            Some(*left),
            None,
            '-',
            diff_removed_style(),
            content,
            model,
            width,
            x,
            Some(DiffCellStyle {
                side: DiffSide::Removed,
                change: *change,
            }),
        ),
        UnifiedDiffRow::Insert {
            right,
            content,
            change,
        } => render_unified_content(
            None,
            Some(*right),
            '+',
            diff_added_style(),
            content,
            model,
            width,
            x,
            Some(DiffCellStyle {
                side: DiffSide::Added,
                change: *change,
            }),
        ),
    }
}

pub(super) fn render_unified_row_window(
    row: &UnifiedDiffRow,
    model: &DiffModel,
    row_offset: usize,
    height: usize,
    width: usize,
    x: usize,
    wrap: bool,
) -> Vec<Line<'static>> {
    if !wrap {
        return (row_offset == 0)
            .then(|| render_unified_row(row, model, width, x))
            .into_iter()
            .collect();
    }

    match row {
        UnifiedDiffRow::Message { text } => {
            render_message_window(text, row_offset, height, width, plain_style())
        }
        UnifiedDiffRow::Context {
            left,
            right,
            content,
        } => render_unified_content_window(
            Some(*left),
            Some(*right),
            ' ',
            gutter_style(),
            content,
            model,
            row_offset,
            height,
            width,
            None,
        ),
        UnifiedDiffRow::Delete {
            left,
            content,
            change,
        } => render_unified_content_window(
            Some(*left),
            None,
            '-',
            diff_removed_style(),
            content,
            model,
            row_offset,
            height,
            width,
            Some(DiffCellStyle {
                side: DiffSide::Removed,
                change: *change,
            }),
        ),
        UnifiedDiffRow::Insert {
            right,
            content,
            change,
        } => render_unified_content_window(
            None,
            Some(*right),
            '+',
            diff_added_style(),
            content,
            model,
            row_offset,
            height,
            width,
            Some(DiffCellStyle {
                side: DiffSide::Added,
                change: *change,
            }),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_unified_content(
    left: Option<usize>,
    right: Option<usize>,
    marker: char,
    marker_style: Style,
    content: &str,
    model: &DiffModel,
    width: usize,
    x: usize,
    diff_style: Option<DiffCellStyle>,
) -> Line<'static> {
    let mut spans = Vec::new();
    let bg_style = diff_style.map(|style| style.line_style());
    push_number(&mut spans, left, model.left_digits(), bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);
    push_number(&mut spans, right, model.right_digits(), bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);
    push_styled_text(&mut spans, &marker.to_string(), marker_style, bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);

    let prefix_width = model.left_digits() + model.right_digits() + 4;
    let content_width = width.saturating_sub(prefix_width);
    let used = prefix_width.saturating_add(push_structured_content(
        &mut spans,
        content,
        x,
        content_width,
        diff_style,
    ));
    if bg_style.is_some() {
        fill_row(&mut spans, width.saturating_sub(used), bg_style);
    }
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
fn render_unified_content_window(
    left: Option<usize>,
    right: Option<usize>,
    marker: char,
    marker_style: Style,
    content: &str,
    model: &DiffModel,
    row_offset: usize,
    height: usize,
    width: usize,
    diff_style: Option<DiffCellStyle>,
) -> Vec<Line<'static>> {
    let prefix_width = model.left_digits() + model.right_digits() + 4;
    let content_width = width.saturating_sub(prefix_width).max(1);
    let bg_style = diff_style.map(|style| style.line_style());
    let visual_count = wrapped_content_visual_count(Some(content), content_width);
    let end = row_offset.saturating_add(height).min(visual_count);

    (row_offset..end)
        .map(|visual_row| {
            let mut spans = Vec::new();
            if visual_row == 0 {
                push_unified_prefix(
                    &mut spans,
                    left,
                    right,
                    marker,
                    marker_style,
                    model,
                    bg_style,
                );
            } else {
                push_unified_continuation_prefix(&mut spans, model, bg_style);
            }
            push_wrapped_content_slice(
                &mut spans,
                content,
                content_width,
                visual_row,
                diff_style,
                bg_style,
                bg_style.is_some(),
            );
            Line::from(spans)
        })
        .collect()
}

fn push_unified_prefix(
    spans: &mut Vec<Span<'static>>,
    left: Option<usize>,
    right: Option<usize>,
    marker: char,
    marker_style: Style,
    model: &DiffModel,
    bg_style: Option<Style>,
) {
    push_number(spans, left, model.left_digits(), bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);
    push_number(spans, right, model.right_digits(), bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);
    push_styled_text(spans, &marker.to_string(), marker_style, bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);
}

fn push_unified_continuation_prefix(
    spans: &mut Vec<Span<'static>>,
    model: &DiffModel,
    bg_style: Option<Style>,
) {
    push_styled_text(
        spans,
        &" ".repeat(model.left_digits()),
        gutter_style(),
        bg_style,
    );
    push_styled_text(spans, " ", gutter_style(), bg_style);
    push_styled_text(
        spans,
        &" ".repeat(model.right_digits()),
        gutter_style(),
        bg_style,
    );
    push_styled_text(spans, "   ", gutter_style(), bg_style);
}

pub(super) fn unified_row_visual_count(
    row: &UnifiedDiffRow,
    model: &DiffModel,
    width: usize,
) -> usize {
    match row {
        UnifiedDiffRow::Message { text } => wrapped_row_count(text, width.max(1), 0),
        UnifiedDiffRow::Context { content, .. }
        | UnifiedDiffRow::Delete { content, .. }
        | UnifiedDiffRow::Insert { content, .. } => {
            let prefix_width = model.left_digits() + model.right_digits() + 4;
            let content_width = width.saturating_sub(prefix_width).max(1);
            wrapped_content_visual_count(Some(content), content_width)
        }
    }
}
