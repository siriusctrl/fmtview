use std::io;

use ratatui::backend::CrosstermBackend;

use crate::diff::{DiffLayout, DiffModel};

use super::super::super::{
    render::ViewPosition,
    terminal::{ScrollHint, ViewerTerminal},
};
use super::super::{DIFF_SCROLL_HINT_MAX_ROWS, DiffViewState, render::diff_row_visual_count};

#[derive(Debug, Clone, Copy)]
pub(in crate::viewer::diff_view) enum DiffJump {
    Next,
    Previous,
}

pub(in crate::viewer::diff_view) fn jump_change(
    model: &DiffModel,
    state: &mut DiffViewState,
    direction: DiffJump,
    page: usize,
) -> bool {
    let changes = model.changed_rows(state.layout);
    if changes.is_empty() {
        state.message = Some("no differences".to_owned());
        return true;
    }
    let targets = change_block_starts(changes);

    let anchor = state.change_cursor.unwrap_or(state.top);
    let target = match direction {
        DiffJump::Next => targets
            .iter()
            .copied()
            .find(|row| *row > anchor)
            .unwrap_or(targets[0]),
        DiffJump::Previous => targets
            .iter()
            .rev()
            .copied()
            .find(|row| *row < anchor)
            .unwrap_or(*targets.last().unwrap_or(&0)),
    };
    state.change_cursor = Some(target);
    state.top_row_offset = 0;
    set_top(
        state,
        target.saturating_sub(diff_context_rows(page)),
        0,
        model.row_count(state.layout),
    )
}

pub(in crate::viewer::diff_view) fn change_block_starts(changes: &[usize]) -> Vec<usize> {
    changes
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, row)| {
            (index == 0 || row > changes[index - 1].saturating_add(1)).then_some(row)
        })
        .collect()
}

fn diff_context_rows(page: usize) -> usize {
    if page < 4 {
        return 0;
    }

    (page / 3).clamp(2, 8).min(page.saturating_sub(1))
}

pub(in crate::viewer::diff_view) fn scroll_by(
    state: &mut DiffViewState,
    model: &DiffModel,
    visible_height: usize,
    width: usize,
    delta: isize,
) -> bool {
    if delta == 0 {
        return false;
    }

    let old = (state.top, state.top_row_offset);
    let steps = delta.unsigned_abs();
    for _ in 0..steps {
        if delta > 0 {
            if !scroll_down_visual_row(state, model, width) {
                break;
            }
        } else if !scroll_up_visual_row(state, model, width) {
            break;
        }
    }
    clamp_top(state, model, width);
    if delta > 0 && old != (state.top, state.top_row_offset) {
        let tail = tail_position(model, state.layout, visible_height, width, state.wrap);
        if visual_position_after((state.top, state.top_row_offset), tail) {
            state.top = tail.0;
            state.top_row_offset = tail.1;
        }
    }
    old != (state.top, state.top_row_offset)
}

fn scroll_down_visual_row(state: &mut DiffViewState, model: &DiffModel, width: usize) -> bool {
    let line_count = model.row_count(state.layout);
    if line_count == 0 || state.top >= line_count {
        return false;
    }
    let visual_count = diff_row_visual_count(model, state.layout, state.top, width, state.wrap);
    if state.top_row_offset + 1 < visual_count {
        state.top_row_offset += 1;
        return true;
    }
    if state.top + 1 < line_count {
        state.top += 1;
        state.top_row_offset = 0;
        return true;
    }
    false
}

fn scroll_up_visual_row(state: &mut DiffViewState, model: &DiffModel, width: usize) -> bool {
    if state.top_row_offset > 0 {
        state.top_row_offset -= 1;
        return true;
    }
    if state.top > 0 {
        state.top -= 1;
        state.top_row_offset =
            diff_row_visual_count(model, state.layout, state.top, width, state.wrap)
                .saturating_sub(1);
        return true;
    }
    false
}

pub(super) fn set_top(
    state: &mut DiffViewState,
    top: usize,
    row_offset: usize,
    line_count: usize,
) -> bool {
    let old = (state.top, state.top_row_offset);
    if line_count == 0 {
        state.top = 0;
        state.top_row_offset = 0;
    } else {
        state.top = top.min(line_count - 1);
        state.top_row_offset = row_offset;
    }
    old != (state.top, state.top_row_offset)
}

pub(super) fn set_tail_top(
    state: &mut DiffViewState,
    model: &DiffModel,
    visible_height: usize,
    width: usize,
) -> bool {
    let old = (state.top, state.top_row_offset);
    let (top, row_offset) = tail_position(model, state.layout, visible_height, width, state.wrap);
    state.top = top;
    state.top_row_offset = row_offset;
    old != (state.top, state.top_row_offset)
}

pub(in crate::viewer::diff_view) fn clamp_top(
    state: &mut DiffViewState,
    model: &DiffModel,
    width: usize,
) {
    let row_count = model.row_count(state.layout);
    if row_count == 0 {
        state.top = 0;
        state.top_row_offset = 0;
        return;
    }
    state.top = state.top.min(row_count - 1);
    let visual_count = diff_row_visual_count(model, state.layout, state.top, width, state.wrap);
    state.top_row_offset = state.top_row_offset.min(visual_count.saturating_sub(1));
}

fn tail_position(
    model: &DiffModel,
    layout: DiffLayout,
    visible_height: usize,
    width: usize,
    wrap: bool,
) -> (usize, usize) {
    let row_count = model.row_count(layout);
    if row_count == 0 {
        return (0, 0);
    }
    let mut needed = visible_height.max(1);
    for row in (0..row_count).rev() {
        let rows = diff_row_visual_count(model, layout, row, width, wrap);
        if rows >= needed {
            return (row, rows - needed);
        }
        needed -= rows;
    }
    (0, 0)
}

fn visual_position_after(left: (usize, usize), right: (usize, usize)) -> bool {
    left.0 > right.0 || (left.0 == right.0 && left.1 > right.1)
}

pub(super) fn scroll_x_by(x: &mut usize, delta: isize) -> bool {
    let old = *x;
    if delta >= 0 {
        *x = x.saturating_add(delta as usize);
    } else {
        *x = x.saturating_sub(delta.unsigned_abs());
    }
    *x != old
}

pub(in crate::viewer::diff_view) fn diff_scroll_hint(
    terminal: &ViewerTerminal<CrosstermBackend<io::Stdout>>,
    position: ViewPosition,
) -> Option<ScrollHint> {
    if let Some(hint) = terminal.scroll_hint(position) {
        return Some(hint);
    }
    let previous = terminal.previous_position()?;
    if previous.row_offset != 0 || position.row_offset != 0 {
        return None;
    }

    let delta = position.top.abs_diff(previous.top);
    if delta == 0 || delta > DIFF_SCROLL_HINT_MAX_ROWS {
        return None;
    }
    let amount = u16::try_from(delta).ok()?;
    if position.top > previous.top {
        Some(ScrollHint::up(amount))
    } else {
        Some(ScrollHint::down(amount))
    }
}
