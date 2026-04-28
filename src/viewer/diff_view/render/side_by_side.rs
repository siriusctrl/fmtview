use ratatui::text::{Line, Span};

use crate::diff::{DiffModel, NumberedDiffLine, SideDiffRow, UnifiedDiffRow};

use super::super::super::{
    palette::{gutter_style, plain_style},
    render::wrapped_row_count,
};
use super::styles::DiffSide;
use super::{
    DiffCellStyle, NumberedCell, push_numbered_cell, push_numbered_cell_window,
    render_message_window, styled_text_line, wrapped_content_visual_count,
};

fn render_side_row(row: &SideDiffRow, model: &DiffModel, width: usize, x: usize) -> Line<'static> {
    match row {
        SideDiffRow::Message { unified } => {
            let text = side_message_text(model, *unified).unwrap_or_default();
            styled_text_line(text, width, plain_style())
        }
        SideDiffRow::Context { unified } => {
            let (left, right) = side_context_lines(model, *unified);
            render_side_content(left, right, model, width, x, None, None)
        }
        SideDiffRow::Change { left, right } => render_side_content(
            left.and_then(|index| changed_side_line(model, index)),
            right.and_then(|index| changed_side_line(model, index)),
            model,
            width,
            x,
            left.and_then(|index| changed_side_style(model, index)),
            right.and_then(|index| changed_side_style(model, index)),
        ),
    }
}

pub(super) fn render_side_row_window(
    row: &SideDiffRow,
    model: &DiffModel,
    row_offset: usize,
    height: usize,
    width: usize,
    x: usize,
    wrap: bool,
) -> Vec<Line<'static>> {
    if !wrap {
        return (row_offset == 0)
            .then(|| render_side_row(row, model, width, x))
            .into_iter()
            .collect();
    }

    match row {
        SideDiffRow::Message { unified } => {
            let text = side_message_text(model, *unified).unwrap_or_default();
            render_message_window(text, row_offset, height, width, plain_style())
        }
        SideDiffRow::Context { unified } => {
            let (left, right) = side_context_lines(model, *unified);
            render_side_content_window(left, right, model, row_offset, height, width, None, None)
        }
        SideDiffRow::Change { left, right } => render_side_content_window(
            left.and_then(|index| changed_side_line(model, index)),
            right.and_then(|index| changed_side_line(model, index)),
            model,
            row_offset,
            height,
            width,
            left.and_then(|index| changed_side_style(model, index)),
            right.and_then(|index| changed_side_style(model, index)),
        ),
    }
}

fn side_message_text(model: &DiffModel, unified: usize) -> Option<&str> {
    match model.unified_rows().get(unified) {
        Some(UnifiedDiffRow::Message { text }) => Some(text),
        _ => None,
    }
}

fn side_context_lines(
    model: &DiffModel,
    unified: usize,
) -> (Option<NumberedDiffLine>, Option<NumberedDiffLine>) {
    match model.unified_rows().get(unified) {
        Some(UnifiedDiffRow::Context {
            left,
            right,
            content,
        }) => (
            Some(NumberedDiffLine {
                number: *left,
                content: content.clone(),
            }),
            Some(NumberedDiffLine {
                number: *right,
                content: content.clone(),
            }),
        ),
        _ => (None, None),
    }
}

fn changed_side_line(model: &DiffModel, unified: usize) -> Option<NumberedDiffLine> {
    match model.unified_rows().get(unified) {
        Some(UnifiedDiffRow::Delete { left, content, .. }) => Some(NumberedDiffLine {
            number: *left,
            content: content.clone(),
        }),
        Some(UnifiedDiffRow::Insert { right, content, .. }) => Some(NumberedDiffLine {
            number: *right,
            content: content.clone(),
        }),
        _ => None,
    }
}

fn changed_side_style(model: &DiffModel, unified: usize) -> Option<DiffCellStyle> {
    match model.unified_rows().get(unified) {
        Some(UnifiedDiffRow::Delete { change, .. }) => Some(DiffCellStyle {
            side: DiffSide::Removed,
            change: *change,
        }),
        Some(UnifiedDiffRow::Insert { change, .. }) => Some(DiffCellStyle {
            side: DiffSide::Added,
            change: *change,
        }),
        _ => None,
    }
}

fn render_side_content(
    left: Option<NumberedDiffLine>,
    right: Option<NumberedDiffLine>,
    model: &DiffModel,
    width: usize,
    x: usize,
    left_diff: Option<DiffCellStyle>,
    right_diff: Option<DiffCellStyle>,
) -> Line<'static> {
    let (left_width, right_width) = side_content_widths(model, width);
    let mut spans = Vec::new();
    push_numbered_cell(
        &mut spans,
        numbered_cell(left.as_ref(), model.left_digits(), left_diff),
        left_width,
        x,
    );
    spans.push(Span::styled(" │ ", gutter_style()));
    push_numbered_cell(
        &mut spans,
        numbered_cell(right.as_ref(), model.right_digits(), right_diff),
        right_width,
        x,
    );
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
fn render_side_content_window(
    left: Option<NumberedDiffLine>,
    right: Option<NumberedDiffLine>,
    model: &DiffModel,
    row_offset: usize,
    height: usize,
    width: usize,
    left_diff: Option<DiffCellStyle>,
    right_diff: Option<DiffCellStyle>,
) -> Vec<Line<'static>> {
    let (left_width, right_width) = side_content_widths(model, width);
    let visual_count = side_cell_visual_count(left.as_ref(), left_width)
        .max(side_cell_visual_count(right.as_ref(), right_width));
    let end = row_offset.saturating_add(height).min(visual_count);

    (row_offset..end)
        .map(|visual_row| {
            let mut spans = Vec::new();
            push_numbered_cell_window(
                &mut spans,
                numbered_cell(left.as_ref(), model.left_digits(), left_diff),
                left_width,
                visual_row,
            );
            spans.push(Span::styled(" │ ", gutter_style()));
            push_numbered_cell_window(
                &mut spans,
                numbered_cell(right.as_ref(), model.right_digits(), right_diff),
                right_width,
                visual_row,
            );
            Line::from(spans)
        })
        .collect()
}

fn numbered_cell<'a>(
    line: Option<&'a NumberedDiffLine>,
    digits: usize,
    diff_style: Option<DiffCellStyle>,
) -> NumberedCell<'a> {
    NumberedCell {
        number: line.map(|line| line.number),
        digits,
        content: line.map(|line| line.content.as_ref()),
        diff_style,
    }
}

pub(super) fn side_row_visual_count(row: &SideDiffRow, model: &DiffModel, width: usize) -> usize {
    match row {
        SideDiffRow::Message { unified } => side_message_text(model, *unified)
            .map(|text| wrapped_row_count(text, width.max(1), 0))
            .unwrap_or(1),
        SideDiffRow::Context { unified } => {
            let (left, right) = side_context_lines(model, *unified);
            let (left_width, right_width) = side_content_widths(model, width);
            side_cell_visual_count(left.as_ref(), left_width)
                .max(side_cell_visual_count(right.as_ref(), right_width))
        }
        SideDiffRow::Change { left, right } => {
            let (left_width, right_width) = side_content_widths(model, width);
            let left = left.and_then(|index| changed_side_line(model, index));
            let right = right.and_then(|index| changed_side_line(model, index));
            side_cell_visual_count(left.as_ref(), left_width)
                .max(side_cell_visual_count(right.as_ref(), right_width))
        }
    }
}

fn side_content_widths(model: &DiffModel, width: usize) -> (usize, usize) {
    let fixed_width = model.left_digits() + model.right_digits() + 9;
    let content_width = width.saturating_sub(fixed_width);
    let left_width = content_width / 2;
    (left_width, content_width.saturating_sub(left_width))
}

fn side_cell_visual_count(line: Option<&NumberedDiffLine>, width: usize) -> usize {
    wrapped_content_visual_count(line.map(|line| line.content.as_ref()), width)
}
