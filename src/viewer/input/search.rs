use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::line_index::ViewFile;

use super::super::SEARCH_CHUNK_LINES;
use super::{keys::accepts_search_char, scroll::set_top, state::ViewState};

#[derive(Debug, Clone)]
pub(in crate::viewer) struct SearchTask {
    pub(in crate::viewer) query: String,
    pub(in crate::viewer) direction: SearchDirection,
    pub(in crate::viewer) next_line: usize,
    pub(in crate::viewer) remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum SearchDirection {
    Forward,
    Backward,
}

pub(in crate::viewer) fn handle_search_input_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
) -> bool {
    match code {
        KeyCode::Char(ch) if accepts_search_char(modifiers) => {
            state.search_buffer.push(ch);
            true
        }
        KeyCode::Enter => submit_search_buffer(state, line_count),
        KeyCode::Backspace => pop_search_char(state),
        KeyCode::Esc => cancel_search_prompt(state),
        _ => false,
    }
}

pub(in crate::viewer) fn start_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = true;
    state.search_buffer.clear();
    state.search_message = None;
    state.search_task = None;
    true
}

pub(in crate::viewer) fn pop_search_char(state: &mut ViewState) -> bool {
    let old_len = state.search_buffer.len();
    state.search_buffer.pop();
    state.search_buffer.len() != old_len
}

pub(in crate::viewer) fn cancel_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = false;
    state.search_buffer.clear();
    true
}

pub(in crate::viewer) fn submit_search_buffer(state: &mut ViewState, line_count: usize) -> bool {
    if state.search_buffer.is_empty() {
        return false;
    }

    let query = state.search_buffer.clone();
    state.search_active = false;
    state.search_buffer.clear();
    start_search(
        state,
        query,
        SearchDirection::Forward,
        state.top,
        line_count,
    )
}

pub(in crate::viewer) fn start_repeat_search(
    state: &mut ViewState,
    line_count: usize,
    direction: SearchDirection,
) -> bool {
    if state.search_query.is_empty() {
        state.search_message = Some("no search query".to_owned());
        return true;
    }

    let start = repeat_search_start(state.top, line_count, direction);
    start_search(
        state,
        state.search_query.clone(),
        direction,
        start,
        line_count,
    )
}

pub(in crate::viewer) fn repeat_search_start(
    top: usize,
    line_count: usize,
    direction: SearchDirection,
) -> usize {
    if line_count == 0 {
        return 0;
    }

    match direction {
        SearchDirection::Forward => top.saturating_add(1) % line_count,
        SearchDirection::Backward => top.checked_sub(1).unwrap_or(line_count - 1),
    }
}

pub(in crate::viewer) fn start_search(
    state: &mut ViewState,
    query: String,
    direction: SearchDirection,
    start_line: usize,
    line_count: usize,
) -> bool {
    if query.is_empty() {
        return false;
    }

    state.search_query = query.clone();
    state.search_message = Some(format!("searching: {query}"));
    if line_count == 0 {
        state.search_task = None;
        state.search_message = Some(format!("not found: {query}"));
        return true;
    }

    state.search_task = Some(SearchTask {
        query,
        direction,
        next_line: start_line.min(line_count.saturating_sub(1)),
        remaining: line_count,
    });
    true
}

pub(in crate::viewer) fn cancel_search_task(state: &mut ViewState) -> bool {
    state.search_task = None;
    state.search_message = Some("search canceled".to_owned());
    true
}

pub(in crate::viewer) fn clear_search_message(state: &mut ViewState) -> bool {
    let was_active = state.search_message.is_some();
    state.search_message = None;
    was_active
}

pub(in crate::viewer) fn process_search_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
) -> Result<bool> {
    let Some(mut task) = state.search_task.take() else {
        return Ok(false);
    };

    let step = scan_search_chunk(file, &task)?;
    if let Some(line) = step.found_line {
        set_top(state, line);
        state.search_message = Some(format!("match: {}", task.query));
        return Ok(true);
    }

    task.next_line = step.next_line;
    let incomplete_index = !file.line_count_exact();
    task.remaining = task.remaining.saturating_sub(step.scanned);
    if incomplete_index
        && task.direction == SearchDirection::Forward
        && task.remaining == 0
        && step.scanned > 0
    {
        task.remaining = SEARCH_CHUNK_LINES;
    }
    if task.remaining == 0 || step.scanned == 0 {
        state.search_message = Some(format!("not found: {}", task.query));
        return Ok(true);
    }

    state.search_task = Some(task);
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct SearchStep {
    pub(in crate::viewer) found_line: Option<usize>,
    pub(in crate::viewer) next_line: usize,
    pub(in crate::viewer) scanned: usize,
}

pub(in crate::viewer) fn scan_search_chunk(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    match task.direction {
        SearchDirection::Forward => scan_search_forward(file, task),
        SearchDirection::Backward => scan_search_backward(file, task),
    }
}

pub(in crate::viewer) fn scan_search_forward(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    let line_count = file.line_count();
    let exact_line_count = file.line_count_exact();
    if line_count == 0 || task.remaining == 0 {
        return Ok(SearchStep {
            found_line: None,
            next_line: 0,
            scanned: 0,
        });
    }

    let mut next_line = task.next_line.min(line_count - 1);
    let mut scanned = 0;
    let limit = task.remaining.min(SEARCH_CHUNK_LINES);

    while scanned < limit {
        let count = (line_count - next_line).min(limit - scanned);
        let lines = file.read_window(next_line, count)?;
        if lines.is_empty() {
            break;
        }

        for (offset, line) in lines.iter().enumerate() {
            if line.contains(&task.query) {
                return Ok(SearchStep {
                    found_line: Some(next_line + offset),
                    next_line: next_line + offset,
                    scanned: scanned + offset + 1,
                });
            }
        }

        scanned += lines.len();
        next_line = next_line.saturating_add(lines.len());
        if next_line >= line_count {
            next_line = if exact_line_count { 0 } else { line_count };
        }
    }

    Ok(SearchStep {
        found_line: None,
        next_line,
        scanned,
    })
}

pub(in crate::viewer) fn scan_search_backward(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    let line_count = file.line_count();
    if line_count == 0 || task.remaining == 0 {
        return Ok(SearchStep {
            found_line: None,
            next_line: 0,
            scanned: 0,
        });
    }

    let mut next_line = task.next_line.min(line_count - 1);
    let mut scanned = 0;
    let limit = task.remaining.min(SEARCH_CHUNK_LINES);

    while scanned < limit {
        let count = (next_line + 1).min(limit - scanned);
        let start = next_line + 1 - count;
        let lines = file.read_window(start, count)?;
        if lines.is_empty() {
            break;
        }

        for (offset, line) in lines.iter().enumerate().rev() {
            if line.contains(&task.query) {
                return Ok(SearchStep {
                    found_line: Some(start + offset),
                    next_line: start + offset,
                    scanned: scanned + (count - offset),
                });
            }
        }

        scanned += lines.len();
        next_line = start.checked_sub(1).unwrap_or(line_count - 1);
    }

    Ok(SearchStep {
        found_line: None,
        next_line,
        scanned,
    })
}
