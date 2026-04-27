use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::line_index::ViewFile;

use super::{
    EVENT_DRAIN_BUDGET, EVENT_DRAIN_LIMIT, JUMP_BUFFER_MAX_DIGITS, MOUSE_HORIZONTAL_COLUMNS,
    MOUSE_SCROLL_LINES, SEARCH_CHUNK_LINES, TAIL_ROW_OFFSET,
};

pub(super) struct ViewState {
    pub(super) top: usize,
    pub(super) top_row_offset: usize,
    pub(super) top_max_row_offset: usize,
    pub(super) wrap_bounds_stale: bool,
    pub(super) x: usize,
    pub(super) wrap: bool,
    pub(super) jump_buffer: String,
    pub(super) search_active: bool,
    pub(super) search_buffer: String,
    pub(super) search_query: String,
    pub(super) search_message: Option<String>,
    pub(super) search_task: Option<SearchTask>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            top: 0,
            top_row_offset: 0,
            top_max_row_offset: 0,
            wrap_bounds_stale: false,
            x: 0,
            wrap: true,
            jump_buffer: String::new(),
            search_active: false,
            search_buffer: String::new(),
            search_query: String::new(),
            search_message: None,
            search_task: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SearchTask {
    pub(super) query: String,
    pub(super) direction: SearchDirection,
    pub(super) next_line: usize,
    pub(super) remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Default)]
pub(super) struct EventAction {
    pub(super) dirty: bool,
    pub(super) quit: bool,
}

impl EventAction {
    fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
    }
}

pub(super) fn drain_events(
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> Result<EventAction> {
    let started = Instant::now();
    let mut action = EventAction::default();
    let mut processed = 0;

    loop {
        let event = event::read().context("failed to read terminal event")?;
        let next = handle_event(event, state, line_count, page);
        let needs_layout = next.dirty && state.wrap_bounds_stale;
        action.merge(next);
        processed += 1;

        if action.quit
            || needs_layout
            || processed >= EVENT_DRAIN_LIMIT
            || started.elapsed() >= EVENT_DRAIN_BUDGET
            || !event::poll(Duration::ZERO).context("failed to poll terminal event")?
        {
            break;
        }
    }

    Ok(action)
}

pub(super) fn handle_event(
    event: Event,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> EventAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => EventAction::default(),
        Event::Key(key) => handle_key_event(key.code, key.modifiers, state, line_count, page),
        Event::Mouse(mouse) if !state.has_active_prompt() => {
            handle_mouse_event(mouse.kind, mouse.modifiers, state, line_count)
        }
        Event::Mouse(_) => EventAction::default(),
        Event::Resize(_, _) => EventAction {
            dirty: true,
            quit: false,
        },
        _ => EventAction::default(),
    }
}

pub(super) fn handle_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> EventAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return EventAction {
            dirty: false,
            quit: true,
        };
    }

    if state.search_active {
        return EventAction {
            dirty: handle_search_input_key(code, modifiers, state, line_count),
            quit: false,
        };
    }

    if !state.jump_buffer.is_empty() {
        return EventAction {
            dirty: handle_jump_input_key(code, modifiers, state, line_count),
            quit: false,
        };
    }

    let dirty = match code {
        KeyCode::Char(ch) if accepts_jump_digit(ch, modifiers) => {
            push_jump_digit(state, ch);
            true
        }
        KeyCode::Char('/') if plain_key(modifiers) => start_search_prompt(state),
        KeyCode::Char('n') if plain_key(modifiers) => {
            start_repeat_search(state, line_count, SearchDirection::Forward)
        }
        KeyCode::Char('N') if plain_key(modifiers) => {
            start_repeat_search(state, line_count, SearchDirection::Backward)
        }
        KeyCode::Enter => false,
        KeyCode::Esc if state.search_task.is_some() => cancel_search_task(state),
        KeyCode::Esc if state.search_message.is_some() => clear_search_message(state),
        KeyCode::Char('q') | KeyCode::Esc => {
            return EventAction {
                dirty: false,
                quit: true,
            };
        }
        KeyCode::Char('w') => {
            state.wrap = !state.wrap;
            reset_top_row_offset(state);
            true
        }
        KeyCode::Down | KeyCode::Char('j') => scroll_down(state, line_count),
        KeyCode::Up | KeyCode::Char('k') => scroll_up(state, line_count),
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
            page_down(state, line_count, page)
        }
        KeyCode::PageUp | KeyCode::Char('b') => page_up(state, line_count, page),
        KeyCode::Home | KeyCode::Char('g') => set_top(state, 0),
        KeyCode::End | KeyCode::Char('G') => set_file_end(state, line_count),
        KeyCode::Right | KeyCode::Char('l') if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    EventAction { dirty, quit: false }
}

impl ViewState {
    fn has_active_prompt(&self) -> bool {
        self.search_active || !self.jump_buffer.is_empty()
    }
}

pub(super) fn handle_jump_input_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
) -> bool {
    match code {
        KeyCode::Char(ch) if accepts_jump_digit(ch, modifiers) => {
            push_jump_digit(state, ch);
            true
        }
        KeyCode::Enter => jump_to_buffered_line(state, line_count),
        KeyCode::Backspace => pop_jump_digit(state),
        KeyCode::Esc => clear_jump_buffer(state),
        _ => false,
    }
}

pub(super) fn handle_search_input_key(
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

pub(super) fn accepts_jump_digit(ch: char, modifiers: KeyModifiers) -> bool {
    ch.is_ascii_digit()
        && !modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
}

pub(super) fn accepts_search_char(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

pub(super) fn plain_key(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

pub(super) fn push_jump_digit(state: &mut ViewState, ch: char) {
    if state.jump_buffer.len() < JUMP_BUFFER_MAX_DIGITS {
        state.jump_buffer.push(ch);
    }
}

pub(super) fn pop_jump_digit(state: &mut ViewState) -> bool {
    let old_len = state.jump_buffer.len();
    state.jump_buffer.pop();
    state.jump_buffer.len() != old_len
}

pub(super) fn clear_jump_buffer(state: &mut ViewState) -> bool {
    let was_active = !state.jump_buffer.is_empty();
    state.jump_buffer.clear();
    was_active
}

pub(super) fn jump_to_buffered_line(state: &mut ViewState, line_count: usize) -> bool {
    if state.jump_buffer.is_empty() {
        return false;
    }

    let requested = state.jump_buffer.parse::<usize>().unwrap_or(usize::MAX);
    state.jump_buffer.clear();
    set_top(state, target_top_for_line(requested, line_count));
    true
}

pub(super) fn target_top_for_line(requested: usize, line_count: usize) -> usize {
    if line_count == 0 {
        return 0;
    }

    requested.max(1).min(line_count).saturating_sub(1)
}

pub(super) fn start_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = true;
    state.search_buffer.clear();
    state.search_message = None;
    state.search_task = None;
    true
}

pub(super) fn pop_search_char(state: &mut ViewState) -> bool {
    let old_len = state.search_buffer.len();
    state.search_buffer.pop();
    state.search_buffer.len() != old_len
}

pub(super) fn cancel_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = false;
    state.search_buffer.clear();
    true
}

pub(super) fn submit_search_buffer(state: &mut ViewState, line_count: usize) -> bool {
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

pub(super) fn start_repeat_search(
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

pub(super) fn repeat_search_start(
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

pub(super) fn start_search(
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

pub(super) fn cancel_search_task(state: &mut ViewState) -> bool {
    state.search_task = None;
    state.search_message = Some("search canceled".to_owned());
    true
}

pub(super) fn clear_search_message(state: &mut ViewState) -> bool {
    let was_active = state.search_message.is_some();
    state.search_message = None;
    was_active
}

pub(super) fn process_search_step(file: &dyn ViewFile, state: &mut ViewState) -> Result<bool> {
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
pub(super) struct SearchStep {
    pub(super) found_line: Option<usize>,
    pub(super) next_line: usize,
    pub(super) scanned: usize,
}

pub(super) fn scan_search_chunk(file: &dyn ViewFile, task: &SearchTask) -> Result<SearchStep> {
    match task.direction {
        SearchDirection::Forward => scan_search_forward(file, task),
        SearchDirection::Backward => scan_search_backward(file, task),
    }
}

pub(super) fn scan_search_forward(file: &dyn ViewFile, task: &SearchTask) -> Result<SearchStep> {
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

pub(super) fn scan_search_backward(file: &dyn ViewFile, task: &SearchTask) -> Result<SearchStep> {
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

pub(super) fn handle_mouse_event(
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
) -> EventAction {
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        MouseEventKind::ScrollDown => scroll_down_by(state, line_count, MOUSE_SCROLL_LINES),
        MouseEventKind::ScrollUp => scroll_up_by(state, line_count, MOUSE_SCROLL_LINES),
        MouseEventKind::ScrollRight if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollLeft if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    EventAction { dirty, quit: false }
}

pub(super) fn scroll_down(state: &mut ViewState, line_count: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset < state.top_max_row_offset {
        state.top_row_offset = state.top_row_offset.saturating_add(1);
        return true;
    }

    let old = state.top;
    state.top = state
        .top
        .saturating_add(1)
        .min(line_count.saturating_sub(1));
    if state.top != old {
        reset_top_row_offset(state);
        return true;
    }

    false
}

pub(super) fn scroll_up(state: &mut ViewState, line_count: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset > 0 {
        state.top_row_offset = state.top_row_offset.saturating_sub(1);
        return true;
    }

    let old = state.top;
    state.top = state.top.saturating_sub(1);
    if state.top != old {
        state.top_row_offset = if state.wrap { TAIL_ROW_OFFSET } else { 0 };
        state.top_max_row_offset = 0;
        state.wrap_bounds_stale = state.wrap;
        return true;
    }

    false
}

pub(super) fn scroll_down_by(state: &mut ViewState, line_count: usize, rows: usize) -> bool {
    let mut dirty = false;
    for _ in 0..rows {
        if !scroll_down(state, line_count) {
            break;
        }
        dirty = true;
        if state.wrap_bounds_stale {
            break;
        }
    }
    dirty
}

pub(super) fn scroll_up_by(state: &mut ViewState, line_count: usize, rows: usize) -> bool {
    let mut dirty = false;
    for _ in 0..rows {
        if !scroll_up(state, line_count) {
            break;
        }
        dirty = true;
        if state.wrap_bounds_stale {
            break;
        }
    }
    dirty
}

pub(super) fn page_down(state: &mut ViewState, line_count: usize, page: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset < state.top_max_row_offset {
        state.top_row_offset = state
            .top_row_offset
            .saturating_add(page)
            .min(state.top_max_row_offset);
        return true;
    }

    scroll_logical_by(state, line_count, page as isize)
}

pub(super) fn page_up(state: &mut ViewState, line_count: usize, page: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset > 0 {
        state.top_row_offset = state.top_row_offset.saturating_sub(page);
        return true;
    }

    scroll_logical_by(state, line_count, -(page as isize))
}

pub(super) fn scroll_logical_by(state: &mut ViewState, line_count: usize, delta: isize) -> bool {
    let old = state.top;
    if delta >= 0 {
        state.top = state
            .top
            .saturating_add(delta as usize)
            .min(line_count.saturating_sub(1));
    } else {
        state.top = state.top.saturating_sub(delta.unsigned_abs());
    }

    if state.top != old {
        reset_top_row_offset(state);
        return true;
    }

    false
}

pub(super) fn set_top(state: &mut ViewState, value: usize) -> bool {
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    if old_top == value && old_offset == 0 {
        return false;
    }

    state.top = value;
    reset_top_row_offset(state);
    true
}

pub(super) fn set_file_end(state: &mut ViewState, line_count: usize) -> bool {
    let value = line_count.saturating_sub(1);
    let target_offset = if line_count > 0 && state.wrap {
        TAIL_ROW_OFFSET
    } else {
        0
    };
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    if old_top == value && old_offset == target_offset {
        return false;
    }

    state.top = value;
    state.top_row_offset = target_offset;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    true
}

pub(super) fn reset_top_row_offset(state: &mut ViewState) {
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
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
