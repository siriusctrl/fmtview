use ratatui::text::Line;

use crate::diff::{DiffLayout, DiffModel};

mod cell;

mod side_by_side;
mod styles;
mod unified;

use cell::{
    NumberedCell, fill_row, push_number, push_numbered_cell, push_numbered_cell_window,
    push_structured_content, push_styled_text, push_wrapped_content_slice, render_message_window,
    styled_text_line, wrapped_content_visual_count,
};
use side_by_side::{render_side_row_window, side_row_visual_count};
use styles::DiffCellStyle;
use unified::{render_unified_row_window, unified_row_visual_count};

#[cfg(test)]
pub(super) fn render_rows(
    model: &DiffModel,
    layout: DiffLayout,
    top: usize,
    height: usize,
    width: usize,
    x: usize,
) -> Vec<Line<'static>> {
    render_rows_with_status(model, layout, top, 0, height, width, x, false).rows
}

#[derive(Debug)]
pub(super) struct RenderedDiffWindow {
    pub(super) rows: Vec<Line<'static>>,
    pub(super) bottom_row: usize,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_rows_with_status(
    model: &DiffModel,
    layout: DiffLayout,
    top: usize,
    row_offset: usize,
    height: usize,
    width: usize,
    x: usize,
    wrap: bool,
) -> RenderedDiffWindow {
    if height == 0 {
        return RenderedDiffWindow {
            rows: Vec::new(),
            bottom_row: top,
        };
    }

    let row_count = model.row_count(layout);
    if row_count == 0 || top >= row_count {
        return RenderedDiffWindow {
            rows: Vec::new(),
            bottom_row: row_count.saturating_sub(1),
        };
    }

    let mut rows = Vec::with_capacity(height);
    let mut row_index = top;
    let mut offset = row_offset;
    let mut bottom_row = top;
    while row_index < row_count && rows.len() < height {
        let remaining = height - rows.len();
        let mut rendered =
            render_row_window(model, layout, row_index, offset, remaining, width, x, wrap);
        if rendered.is_empty() && offset > 0 {
            offset = 0;
            rendered =
                render_row_window(model, layout, row_index, offset, remaining, width, x, wrap);
        }
        if rendered.is_empty() {
            break;
        }
        let visual_count = diff_row_visual_count(model, layout, row_index, width, wrap);
        let consumed = rendered.len();
        rows.append(&mut rendered);
        bottom_row = row_index;
        if offset.saturating_add(consumed) < visual_count {
            break;
        }
        row_index = row_index.saturating_add(1);
        offset = 0;
    }

    RenderedDiffWindow { rows, bottom_row }
}

#[allow(clippy::too_many_arguments)]
fn render_row_window(
    model: &DiffModel,
    layout: DiffLayout,
    row_index: usize,
    row_offset: usize,
    height: usize,
    width: usize,
    x: usize,
    wrap: bool,
) -> Vec<Line<'static>> {
    match layout {
        DiffLayout::Unified => model
            .unified_rows()
            .get(row_index)
            .map(|row| render_unified_row_window(row, model, row_offset, height, width, x, wrap))
            .unwrap_or_default(),
        DiffLayout::SideBySide => model
            .side_rows()
            .get(row_index)
            .map(|row| render_side_row_window(row, model, row_offset, height, width, x, wrap))
            .unwrap_or_default(),
    }
}

pub(super) fn diff_row_visual_count(
    model: &DiffModel,
    layout: DiffLayout,
    row_index: usize,
    width: usize,
    wrap: bool,
) -> usize {
    if !wrap {
        return 1;
    }

    match layout {
        DiffLayout::Unified => model
            .unified_rows()
            .get(row_index)
            .map(|row| unified_row_visual_count(row, model, width))
            .unwrap_or(1),
        DiffLayout::SideBySide => model
            .side_rows()
            .get(row_index)
            .map(|row| side_row_visual_count(row, model, width))
            .unwrap_or(1),
    }
}
