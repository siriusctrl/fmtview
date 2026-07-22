use anyhow::Result;

use crate::{load::ViewFile, transform::FormatKind};

use super::input::{FooterMessageKind, ViewState};

mod candidate;
mod scan;
mod visibility;

use scan::scan_structure_chunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum StructureDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct StructureTask {
    pub(super) direction: StructureDirection,
    pub(super) next_line: usize,
    pub(super) viewport: Option<StructureViewport>,
    pub(super) awaiting_older: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct StructureViewport {
    pub(in crate::viewer) top: usize,
    pub(in crate::viewer) top_row_offset: usize,
    pub(in crate::viewer) bottom: usize,
    pub(in crate::viewer) bottom_line_end: bool,
    pub(in crate::viewer) x: usize,
    pub(in crate::viewer) width: usize,
    pub(in crate::viewer) wrap: bool,
}

impl StructureViewport {
    fn matches_state(self, state: &ViewState) -> bool {
        self.top == state.top
            && self.top_row_offset == state.top_row_offset
            && self.x == state.x
            && self.wrap == state.wrap
    }
}

#[cfg(test)]
pub(in crate::viewer) fn start_structure_navigation(
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
    direction: StructureDirection,
) -> bool {
    start_structure_navigation_with_older(state, line_count, line_count_exact, false, direction)
}

pub(in crate::viewer) fn start_structure_navigation_with_older(
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
    has_older_records: bool,
    direction: StructureDirection,
) -> bool {
    state.clear_tool_navigation();
    state.structure_task = None;
    state.structure_target = None;
    state.search_target = None;
    state.search_task = None;
    if line_count == 0 {
        set_no_block_message(state, direction);
        return true;
    }

    let anchor = state.structure_cursor.unwrap_or(state.top);
    let awaiting_older =
        direction == StructureDirection::Backward && anchor == 0 && has_older_records;
    let next_line = if awaiting_older {
        0
    } else {
        let Some(next_line) = structure_start_line(anchor, line_count, line_count_exact, direction)
        else {
            set_no_block_message(state, direction);
            return true;
        };
        next_line
    };

    state.clear_footer_message();
    let viewport = state
        .structure_viewport
        .filter(|viewport| viewport.matches_state(state));
    state.structure_task = Some(StructureTask {
        direction,
        next_line,
        viewport,
        awaiting_older,
    });
    true
}

pub(in crate::viewer) fn process_structure_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
    format: FormatKind,
) -> Result<bool> {
    let Some(mut task) = state.structure_task.take() else {
        return Ok(false);
    };

    let step = scan_structure_chunk(file, &task, format)?;
    if let Some(target) = step.found {
        state.structure_target = Some(target);
        state.structure_cursor = Some(target.line);
        state.clear_footer_message();
        return Ok(true);
    }

    task.next_line = step.next_line;
    if step.scanned == 0 || reached_structure_scan_end(file, &task) {
        set_no_block_message(state, task.direction);
        return Ok(true);
    }

    state.structure_task = Some(task);
    Ok(false)
}

fn structure_start_line(
    top: usize,
    line_count: usize,
    line_count_exact: bool,
    direction: StructureDirection,
) -> Option<usize> {
    match direction {
        StructureDirection::Forward => {
            let next = top.saturating_add(1);
            if next < line_count || !line_count_exact {
                Some(next)
            } else {
                None
            }
        }
        StructureDirection::Backward => top.checked_sub(1),
    }
}

fn reached_structure_scan_end(file: &dyn ViewFile, task: &StructureTask) -> bool {
    match task.direction {
        StructureDirection::Forward => {
            file.at_newer_boundary() && task.next_line >= file.line_count()
        }
        StructureDirection::Backward => task.next_line == usize::MAX,
    }
}

fn no_block_message(direction: StructureDirection) -> &'static str {
    match direction {
        StructureDirection::Forward => "no next structure",
        StructureDirection::Backward => "no previous structure",
    }
}

fn set_no_block_message(state: &mut ViewState, direction: StructureDirection) {
    state.set_footer_message(no_block_message(direction), FooterMessageKind::Warning);
    if state.viewport_at_tail {
        state.preserve_tail_on_next_draw = true;
    }
}

#[cfg(test)]
pub(in crate::viewer) fn is_structure_point(
    format: FormatKind,
    line: &str,
    previous_line: Option<&str>,
) -> bool {
    crate::formats::structure_candidate_kind(format, line, previous_line).is_some()
}
