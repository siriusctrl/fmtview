use anyhow::Result;

use crate::load::ViewFile;

use super::{
    input::{self, ViewState, reset_top_row_offset},
    render::{
        LineWindowCache, RenderContext, TailPositionCache, ViewPosition, continuation_indent,
        is_after_tail, last_full_logical_page_top, next_wrap_end, rendered_row_count,
    },
};

pub(super) fn adjust_state_for_visible_height(
    file: &dyn ViewFile,
    state: &mut ViewState,
    visible_height: usize,
    render_context: RenderContext,
    tail_cache: &mut TailPositionCache,
) -> Result<Option<ViewPosition>> {
    let logical_tail_top = last_full_logical_page_top(file.line_count(), visible_height);
    let tail = if file.line_count_exact() && (!state.wrap || state.top >= logical_tail_top) {
        Some(tail_cache.position(file, visible_height, render_context)?)
    } else {
        None
    };
    if let Some(tail) = tail.filter(|tail| is_after_tail(state, *tail)) {
        state.top = tail.top;
        state.top_row_offset = tail.row_offset;
        state.top_max_row_offset = 0;
        state.wrap_bounds_stale = state.wrap;
    }
    let max_top = file.line_count().saturating_sub(1);
    if file.line_count_exact() && state.top > max_top {
        state.top = max_top;
        reset_top_row_offset(state);
    }
    Ok(tail)
}

pub(super) fn resolve_targets_from_view(
    file: &dyn ViewFile,
    state: &mut ViewState,
    line_cache: &mut LineWindowCache,
    visible_height: usize,
    render_context: RenderContext,
    tail_cache: &mut TailPositionCache,
) -> Result<Option<ViewPosition>> {
    resolve_search_target_from_view(file, state, line_cache, visible_height, render_context)?;
    resolve_structure_target_from_view(file, state, line_cache, visible_height, render_context)?;
    adjust_state_for_visible_height(file, state, visible_height, render_context, tail_cache)
}

fn resolve_search_target_from_view(
    file: &dyn ViewFile,
    state: &mut ViewState,
    line_cache: &mut LineWindowCache,
    visible_height: usize,
    render_context: RenderContext,
) -> Result<()> {
    for _ in 0..3 {
        let lines = line_cache.read(
            file,
            state.top,
            visible_height,
            visible_height.saturating_mul(2).max(32),
        )?;
        if !resolve_search_target_position(state, lines.lines, visible_height, render_context) {
            break;
        }
    }
    Ok(())
}

fn resolve_structure_target_from_view(
    file: &dyn ViewFile,
    state: &mut ViewState,
    line_cache: &mut LineWindowCache,
    visible_height: usize,
    render_context: RenderContext,
) -> Result<()> {
    for _ in 0..3 {
        let lines = line_cache.read(
            file,
            state.top,
            visible_height,
            visible_height.saturating_mul(2).max(32),
        )?;
        if !resolve_structure_target_position(state, lines.lines, visible_height, render_context) {
            break;
        }
    }
    Ok(())
}

pub(in crate::viewer) fn resolve_search_target_position(
    state: &mut ViewState,
    lines: &[String],
    visible_height: usize,
    context: RenderContext,
) -> bool {
    let Some(target) = state.search_target else {
        return false;
    };

    let resolution = resolve_target_position(state, lines, visible_height, context, target);
    if resolution.resolved {
        state.search_target = None;
    }
    resolution.changed
}

pub(in crate::viewer) fn resolve_structure_target_position(
    state: &mut ViewState,
    _lines: &[String],
    _visible_height: usize,
    _context: RenderContext,
) -> bool {
    let Some(target) = state.structure_target else {
        return false;
    };

    state.structure_target = None;
    position_structure_target_line(state, target.line)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetResolution {
    changed: bool,
    resolved: bool,
}

fn resolve_target_position(
    state: &mut ViewState,
    lines: &[String],
    visible_height: usize,
    context: RenderContext,
    target: input::SearchTarget,
) -> TargetResolution {
    let context_rows = search_context_rows(visible_height);
    match target_visual_position_in_window(lines, state.top, target, context) {
        Some(position)
            if visual_row_is_visible(position.row, state.top_row_offset, visible_height) =>
        {
            TargetResolution {
                changed: false,
                resolved: true,
            }
        }
        Some(position) if target.line == state.top => {
            state.top_row_offset = position.row_in_line.saturating_sub(context_rows);
            state.top_max_row_offset = 0;
            TargetResolution {
                changed: false,
                resolved: true,
            }
        }
        Some(position) if position.row_in_line > context_rows => {
            let changed =
                position_target_visual_line(state, target.line, position.row_in_line, context_rows);
            TargetResolution {
                changed,
                resolved: true,
            }
        }
        Some(position) => {
            if position_target_logical_line(state, target.line, visible_height) {
                TargetResolution {
                    changed: true,
                    resolved: false,
                }
            } else {
                let changed = position_target_visual_line(
                    state,
                    target.line,
                    position.row_in_line,
                    context_rows,
                );
                TargetResolution {
                    changed,
                    resolved: true,
                }
            }
        }
        None => TargetResolution {
            changed: position_target_logical_line(state, target.line, visible_height),
            resolved: false,
        },
    }
}

fn position_target_visual_line(
    state: &mut ViewState,
    target_line: usize,
    target_row: usize,
    context_rows: usize,
) -> bool {
    let old_top = state.top;
    state.top = target_line;
    state.top_row_offset = target_row.saturating_sub(context_rows);
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    state.top != old_top
}

fn position_target_logical_line(
    state: &mut ViewState,
    target_line: usize,
    visible_height: usize,
) -> bool {
    let next_top = target_line.saturating_sub(search_context_rows(visible_height));
    if state.top == next_top && state.top_row_offset == 0 {
        return false;
    }

    state.top = next_top;
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    true
}

fn position_structure_target_line(state: &mut ViewState, target_line: usize) -> bool {
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    state.top = target_line;
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    state.top != old_top || old_offset != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetVisualPosition {
    row: usize,
    row_in_line: usize,
}

fn target_visual_position_in_window(
    lines: &[String],
    first_line: usize,
    target: input::SearchTarget,
    context: RenderContext,
) -> Option<TargetVisualPosition> {
    let target_index = target.line.checked_sub(first_line)?;
    if target_index >= lines.len() {
        return None;
    }

    let mut row = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        if index == target_index {
            let row_in_line = visual_row_for_byte(line, target.byte_index, context);
            return Some(TargetVisualPosition {
                row: row.saturating_add(row_in_line),
                row_in_line,
            });
        }
        row = row.saturating_add(rendered_row_count(line, context));
    }

    None
}

fn visual_row_is_visible(row: usize, top_row_offset: usize, visible_height: usize) -> bool {
    visible_height > 0
        && row >= top_row_offset
        && row.saturating_sub(top_row_offset) < visible_height
}

pub(in crate::viewer) fn search_context_rows(visible_height: usize) -> usize {
    if visible_height < 4 {
        return 0;
    }

    (visible_height / 3)
        .clamp(2, 8)
        .min(visible_height.saturating_sub(1))
}

pub(in crate::viewer) fn visual_row_for_byte(
    line: &str,
    byte_index: usize,
    context: RenderContext,
) -> usize {
    if !context.wrap || line.is_empty() || context.width == 0 {
        return 0;
    }

    let target_byte = floor_char_boundary(line, byte_index.min(line.len()));
    let continuation_indent = continuation_indent(line, context.width);
    let mut start_byte = 0_usize;
    let mut start_char = 0_usize;
    let mut row = 0_usize;

    while start_byte < line.len() {
        let indent = if row > 0 {
            continuation_indent.min(context.width.saturating_sub(1))
        } else {
            0
        };
        let row_width = context.width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        if target_byte < end_byte || end_byte >= line.len() {
            return row;
        }

        start_byte = end_byte.max(start_byte.saturating_add(1)).min(line.len());
        start_char = end_char.max(start_char.saturating_add(1));
        row = row.saturating_add(1);
    }

    row
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
