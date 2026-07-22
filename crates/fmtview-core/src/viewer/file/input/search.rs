use crate::viewer::{KeyCode, KeyModifiers};
use anyhow::Result;

use crate::load::ViewFile;

use super::{
    keys::accepts_search_char,
    state::{FooterMessageKind, ViewState},
};
use crate::viewer::file::SEARCH_CHUNK_LINES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct SearchTarget {
    pub(in crate::viewer) line: usize,
    pub(in crate::viewer) byte_index: usize,
}

#[derive(Debug, Clone)]
pub(in crate::viewer) struct SearchTask {
    pub(in crate::viewer) query: String,
    pub(in crate::viewer) direction: SearchDirection,
    pub(in crate::viewer) next_line: usize,
    pub(in crate::viewer) remaining: usize,
    pub(in crate::viewer) awaiting_older: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct SearchMatchIndex {
    pub(in crate::viewer) query: String,
    pub(in crate::viewer) counted_lines: usize,
    pub(in crate::viewer) matches: usize,
    pub(in crate::viewer) line_match_totals: Vec<usize>,
    pub(in crate::viewer) exact: bool,
}

impl SearchMatchIndex {
    fn new(query: String) -> Self {
        Self {
            query,
            counted_lines: 0,
            matches: 0,
            line_match_totals: Vec::new(),
            exact: false,
        }
    }
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
    state.clear_tool_navigation();
    state.search_active = true;
    state.search_buffer.clear();
    state.search_task = None;
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.structure_cursor = None;
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

pub(in crate::viewer) fn clear_search_session(state: &mut ViewState) -> bool {
    let was_active = state.has_search_session() || state.footer_message.is_some();
    state.search_active = false;
    state.search_buffer.clear();
    state.search_query.clear();
    state.search_task = None;
    state.search_index = None;
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.clear_footer_message();
    was_active
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
        state.set_footer_message("no search query", FooterMessageKind::Warning);
        return true;
    }

    let start = repeat_search_start(
        state.search_cursor.unwrap_or(state.top),
        line_count,
        direction,
    );
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

    state.clear_tool_navigation();
    state.search_query = query.clone();
    state.set_footer_message(format!("searching: {query}"), FooterMessageKind::Info);
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.structure_cursor = None;
    ensure_search_match_index(state, &query);
    if line_count == 0 {
        state.search_task = None;
        state.set_footer_message(format!("not found: {query}"), FooterMessageKind::Warning);
        return true;
    }

    state.search_task = Some(SearchTask {
        query,
        direction,
        next_line: start_line.min(line_count.saturating_sub(1)),
        remaining: line_count,
        awaiting_older: false,
    });
    true
}

fn ensure_search_match_index(state: &mut ViewState, query: &str) {
    if state
        .search_index
        .as_ref()
        .is_some_and(|index| index.query == query)
    {
        return;
    }

    state.search_index = Some(SearchMatchIndex::new(query.to_owned()));
}

pub(in crate::viewer) fn clear_footer_message(state: &mut ViewState) -> bool {
    state.clear_footer_message()
}

pub(in crate::viewer) fn process_search_index_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
) -> Result<bool> {
    if state.search_query.is_empty() {
        state.search_index = None;
        return Ok(false);
    }

    let Some(mut index) = state.search_index.take() else {
        return Ok(false);
    };
    if index.query != state.search_query {
        state.search_index = Some(SearchMatchIndex::new(state.search_query.clone()));
        return Ok(true);
    }
    if index.exact {
        state.search_index = Some(index);
        return Ok(false);
    }

    let line_count = file.line_count();
    let old_counted_lines = index.counted_lines;
    let old_matches = index.matches;
    let old_exact = index.exact;

    if index.counted_lines < line_count {
        let count = (line_count - index.counted_lines).min(SEARCH_CHUNK_LINES);
        let lines = file.read_window(index.counted_lines, count)?;
        for line in &lines {
            index.matches = index
                .matches
                .saturating_add(line.matches(&index.query).count());
            index.line_match_totals.push(index.matches);
        }
        index.counted_lines = index.counted_lines.saturating_add(lines.len());
    }

    if file.line_count_exact() && index.counted_lines >= file.line_count() {
        index.exact = true;
    }

    let dirty = index.counted_lines != old_counted_lines
        || index.matches != old_matches
        || index.exact != old_exact;
    state.search_index = Some(index);
    let ordinal_dirty = update_search_match_ordinal(file, state)?;
    Ok(dirty || ordinal_dirty)
}

pub(in crate::viewer) fn process_search_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
) -> Result<bool> {
    let Some(mut task) = state.search_task.take() else {
        return Ok(false);
    };

    if task.awaiting_older {
        if file.has_older_records() {
            state.search_task = Some(task);
            return Ok(false);
        }
        task.awaiting_older = false;
        task.next_line = 0;
        task.remaining = file.line_count();
    }

    let step = scan_search_chunk(file, &task)?;
    if let Some(target) = step.found {
        state.search_cursor = Some(target.line);
        state.search_target = Some(target);
        state.search_match_target = Some(target);
        state.search_match_ordinal = search_match_ordinal_from_index(file, state, target)?;
        state.set_footer_message(format!("match: {}", task.query), FooterMessageKind::Info);
        return Ok(true);
    }

    task.next_line = step.next_line;
    let incomplete_index = !file.at_newer_boundary();
    task.remaining = task.remaining.saturating_sub(step.scanned);
    if incomplete_index
        && task.direction == SearchDirection::Forward
        && task.remaining == 0
        && step.scanned > 0
    {
        task.remaining = SEARCH_CHUNK_LINES;
    }
    let reached_unloaded_prefix = task.direction == SearchDirection::Forward
        && file.has_older_records()
        && task.next_line >= file.line_count();
    if reached_unloaded_prefix
        || ((task.remaining == 0 || step.scanned == 0) && file.has_older_records())
    {
        task.remaining = 0;
        task.awaiting_older = true;
        state.search_task = Some(task);
        return Ok(false);
    }
    if task.remaining == 0 || step.scanned == 0 {
        state.search_target = None;
        state.search_match_ordinal = None;
        state.search_match_target = None;
        state.set_footer_message(
            format!("not found: {}", task.query),
            FooterMessageKind::Warning,
        );
        return Ok(true);
    }

    state.search_task = Some(task);
    Ok(false)
}

fn update_search_match_ordinal(file: &dyn ViewFile, state: &mut ViewState) -> Result<bool> {
    let previous = state.search_match_ordinal;
    state.search_match_ordinal = state
        .search_match_target
        .map(|target| search_match_ordinal_from_index(file, state, target))
        .transpose()?
        .flatten();
    Ok(state.search_match_ordinal != previous)
}

pub(in crate::viewer) fn search_match_ordinal_from_index(
    file: &dyn ViewFile,
    state: &ViewState,
    target: SearchTarget,
) -> Result<Option<usize>> {
    let Some(index) = state.search_index.as_ref() else {
        return Ok(None);
    };
    if index.query != state.search_query
        || index.query.is_empty()
        || target.line >= index.counted_lines
    {
        return Ok(None);
    };

    let lines = file.read_window(target.line, 1)?;
    let Some(target_line) = lines.first() else {
        return Ok(None);
    };
    let byte_index = floor_char_boundary(target_line, target.byte_index.min(target_line.len()));
    let previous_matches = if target.line == 0 {
        0
    } else {
        let Some(matches) = index.line_match_totals.get(target.line - 1) else {
            return Ok(None);
        };
        *matches
    };
    let matches_before_target = target_line[..byte_index].matches(&index.query).count();
    Ok(Some(
        previous_matches
            .saturating_add(matches_before_target)
            .saturating_add(1),
    ))
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct SearchStep {
    pub(in crate::viewer) found: Option<SearchTarget>,
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
    let exact_line_count = file.at_newer_boundary() && !file.has_older_records();
    if line_count == 0 || task.remaining == 0 {
        return Ok(SearchStep {
            found: None,
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
            if let Some(byte_index) = line.find(&task.query) {
                let found_line = next_line + offset;
                return Ok(SearchStep {
                    found: Some(SearchTarget {
                        line: found_line,
                        byte_index,
                    }),
                    next_line: found_line,
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
        found: None,
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
            found: None,
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
            if let Some(byte_index) = line.rfind(&task.query) {
                let found_line = start + offset;
                return Ok(SearchStep {
                    found: Some(SearchTarget {
                        line: found_line,
                        byte_index,
                    }),
                    next_line: found_line,
                    scanned: scanned + (count - offset),
                });
            }
        }

        scanned += lines.len();
        next_line = start.checked_sub(1).unwrap_or(line_count - 1);
    }

    Ok(SearchStep {
        found: None,
        next_line,
        scanned,
    })
}
