use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    io::{self, Write},
    ops::Range,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::line_index::IndexedTempFile;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const EVENT_DRAIN_BUDGET: Duration = Duration::from_millis(8);
const EVENT_DRAIN_LIMIT: usize = 512;
const MOUSE_SCROLL_LINES: usize = 1;
const MOUSE_HORIZONTAL_COLUMNS: usize = 4;
const RENDER_CACHE_MAX_LINES: usize = 512;
const RENDER_CACHE_MAX_ROWS_PER_LINE: usize = 256;
const WRAP_RENDER_CHUNK_ROWS: usize = 64;
const WRAP_RENDER_CHUNKS_PER_LINE: usize = 64;
const WRAP_CHECKPOINT_INTERVAL_ROWS: usize = 256;
const HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES: usize = 32 * 1024;
const WRAP_PREWARM_LOGICAL_LINES: usize = 4;
const WRAP_GUTTER_MINOR_TICK_ROWS: usize = 8;
const WRAP_GUTTER_MAJOR_TICK_ROWS: usize = 64;
const PREWARM_PAGES: usize = 2;
const PREWARM_MAX_LINES: usize = 192;
const PREWARM_MAX_LINE_BYTES: usize = 16 * 1024;
const PREWARM_BUDGET: Duration = Duration::from_millis(4);
const JUMP_BUFFER_MAX_DIGITS: usize = 20;
const SEARCH_CHUNK_LINES: usize = 4096;
const TAIL_ROW_OFFSET: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Plain,
    Diff,
}

pub fn run(file: IndexedTempFile, mode: ViewMode) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut cleanup = TerminalCleanup::active();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    let result = run_loop(&mut terminal, &file, mode);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )
    .ok();
    cleanup.disarm();
    terminal.show_cursor().ok();

    result
}

struct TerminalCleanup {
    active: bool,
}

impl TerminalCleanup {
    fn active() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        disable_raw_mode().ok();
        let mut stdout = io::stdout();
        execute!(stdout, DisableMouseCapture, LeaveAlternateScreen).ok();
        stdout.flush().ok();
    }
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    file: &IndexedTempFile,
    mode: ViewMode,
) -> Result<()> {
    let mut state = ViewState::default();
    let mut dirty = true;
    let mut line_cache = LineWindowCache::default();
    let mut render_cache = RenderedLineCache::default();
    let mut tail_cache = TailPositionCache::default();

    loop {
        if state.search_task.is_some() {
            dirty |= process_search_step(file, &mut state)?;
        }

        if dirty {
            draw_view(
                terminal,
                file,
                mode,
                &mut state,
                &mut line_cache,
                &mut render_cache,
                &mut tail_cache,
            )?;
            dirty = false;
        }

        let poll_interval = if state.search_task.is_some() {
            Duration::ZERO
        } else {
            EVENT_POLL_INTERVAL
        };
        if !event::poll(poll_interval).context("failed to poll terminal event")? {
            continue;
        }

        let page = terminal
            .size()
            .map(|size| usize::from(size.height.saturating_sub(4)).max(1))
            .unwrap_or(20);
        let action = drain_events(&mut state, file.line_count(), page)?;
        if action.quit {
            break;
        }
        dirty |= action.dirty;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct ViewState {
    top: usize,
    top_row_offset: usize,
    top_max_row_offset: usize,
    wrap_bounds_stale: bool,
    x: usize,
    wrap: bool,
    jump_buffer: String,
    search_active: bool,
    search_buffer: String,
    search_query: String,
    search_message: Option<String>,
    search_task: Option<SearchTask>,
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
struct SearchTask {
    query: String,
    direction: SearchDirection,
    next_line: usize,
    remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Default)]
struct EventAction {
    dirty: bool,
    quit: bool,
}

impl EventAction {
    fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
    }
}

fn drain_events(state: &mut ViewState, line_count: usize, page: usize) -> Result<EventAction> {
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

fn handle_event(
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

fn handle_key_event(
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

fn handle_jump_input_key(
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

fn handle_search_input_key(
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

fn accepts_jump_digit(ch: char, modifiers: KeyModifiers) -> bool {
    ch.is_ascii_digit()
        && !modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
}

fn accepts_search_char(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

fn plain_key(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

fn push_jump_digit(state: &mut ViewState, ch: char) {
    if state.jump_buffer.len() < JUMP_BUFFER_MAX_DIGITS {
        state.jump_buffer.push(ch);
    }
}

fn pop_jump_digit(state: &mut ViewState) -> bool {
    let old_len = state.jump_buffer.len();
    state.jump_buffer.pop();
    state.jump_buffer.len() != old_len
}

fn clear_jump_buffer(state: &mut ViewState) -> bool {
    let was_active = !state.jump_buffer.is_empty();
    state.jump_buffer.clear();
    was_active
}

fn jump_to_buffered_line(state: &mut ViewState, line_count: usize) -> bool {
    if state.jump_buffer.is_empty() {
        return false;
    }

    let requested = state.jump_buffer.parse::<usize>().unwrap_or(usize::MAX);
    state.jump_buffer.clear();
    set_top(state, target_top_for_line(requested, line_count));
    true
}

fn target_top_for_line(requested: usize, line_count: usize) -> usize {
    if line_count == 0 {
        return 0;
    }

    requested.max(1).min(line_count).saturating_sub(1)
}

fn start_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = true;
    state.search_buffer.clear();
    state.search_message = None;
    state.search_task = None;
    true
}

fn pop_search_char(state: &mut ViewState) -> bool {
    let old_len = state.search_buffer.len();
    state.search_buffer.pop();
    state.search_buffer.len() != old_len
}

fn cancel_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = false;
    state.search_buffer.clear();
    true
}

fn submit_search_buffer(state: &mut ViewState, line_count: usize) -> bool {
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

fn start_repeat_search(
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

fn repeat_search_start(top: usize, line_count: usize, direction: SearchDirection) -> usize {
    if line_count == 0 {
        return 0;
    }

    match direction {
        SearchDirection::Forward => top.saturating_add(1) % line_count,
        SearchDirection::Backward => top.checked_sub(1).unwrap_or(line_count - 1),
    }
}

fn start_search(
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

fn cancel_search_task(state: &mut ViewState) -> bool {
    state.search_task = None;
    state.search_message = Some("search canceled".to_owned());
    true
}

fn clear_search_message(state: &mut ViewState) -> bool {
    let was_active = state.search_message.is_some();
    state.search_message = None;
    was_active
}

fn process_search_step(file: &IndexedTempFile, state: &mut ViewState) -> Result<bool> {
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
    task.remaining = task.remaining.saturating_sub(step.scanned);
    if task.remaining == 0 || step.scanned == 0 {
        state.search_message = Some(format!("not found: {}", task.query));
        return Ok(true);
    }

    state.search_task = Some(task);
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchStep {
    found_line: Option<usize>,
    next_line: usize,
    scanned: usize,
}

fn scan_search_chunk(file: &IndexedTempFile, task: &SearchTask) -> Result<SearchStep> {
    match task.direction {
        SearchDirection::Forward => scan_search_forward(file, task),
        SearchDirection::Backward => scan_search_backward(file, task),
    }
}

fn scan_search_forward(file: &IndexedTempFile, task: &SearchTask) -> Result<SearchStep> {
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
            next_line = 0;
        }
    }

    Ok(SearchStep {
        found_line: None,
        next_line,
        scanned,
    })
}

fn scan_search_backward(file: &IndexedTempFile, task: &SearchTask) -> Result<SearchStep> {
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

fn handle_mouse_event(
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

fn scroll_down(state: &mut ViewState, line_count: usize) -> bool {
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

fn scroll_up(state: &mut ViewState, line_count: usize) -> bool {
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

fn scroll_down_by(state: &mut ViewState, line_count: usize, rows: usize) -> bool {
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

fn scroll_up_by(state: &mut ViewState, line_count: usize, rows: usize) -> bool {
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

fn page_down(state: &mut ViewState, line_count: usize, page: usize) -> bool {
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

fn page_up(state: &mut ViewState, line_count: usize, page: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset > 0 {
        state.top_row_offset = state.top_row_offset.saturating_sub(page);
        return true;
    }

    scroll_logical_by(state, line_count, -(page as isize))
}

fn scroll_logical_by(state: &mut ViewState, line_count: usize, delta: isize) -> bool {
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

fn set_top(state: &mut ViewState, value: usize) -> bool {
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    if old_top == value && old_offset == 0 {
        return false;
    }

    state.top = value;
    reset_top_row_offset(state);
    true
}

fn set_file_end(state: &mut ViewState, line_count: usize) -> bool {
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

fn reset_top_row_offset(state: &mut ViewState) {
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
}

fn scroll_x_by(x: &mut usize, delta: isize) -> bool {
    let old = *x;
    if delta >= 0 {
        *x = x.saturating_add(delta as usize);
    } else {
        *x = x.saturating_sub(delta.unsigned_abs());
    }
    *x != old
}

fn draw_view(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    file: &IndexedTempFile,
    mode: ViewMode,
    state: &mut ViewState,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    tail_cache: &mut TailPositionCache,
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let visible_height = usize::from(size.height.saturating_sub(3));
    let visible_width = usize::from(size.width.saturating_sub(2));
    let gutter_digits = line_number_digits(file.line_count());
    let gutter_width = gutter_digits + 3;
    let content_width = visible_width.saturating_sub(gutter_width);
    let render_context = RenderContext {
        gutter_digits,
        x: state.x,
        width: content_width,
        wrap: state.wrap,
        mode,
    };
    let logical_tail_top = last_full_logical_page_top(file.line_count(), visible_height);
    let tail = if !state.wrap || state.top >= logical_tail_top {
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
    if state.top > max_top {
        state.top = max_top;
        reset_top_row_offset(state);
    }

    let lines = line_cache.read(
        file,
        state.top,
        visible_height,
        visible_height.saturating_mul(2).max(32),
    )?;
    let render_request = RenderRequest {
        context: render_context,
        row_limit: render_row_limit(visible_height),
    };
    if state.top_row_offset == TAIL_ROW_OFFSET {
        state.top_row_offset =
            exact_top_line_tail_offset(lines.lines, visible_height, render_context);
    }
    state.wrap_bounds_stale = false;

    let mut viewport = render_viewport(
        lines.lines,
        state.top + 1,
        state.top_row_offset,
        visible_height,
        render_request,
        render_cache,
        active_search_query(state),
    );
    let mut max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        visible_height,
        render_context,
        render_cache,
        tail,
    );
    if viewport.lines.is_empty() && state.top_row_offset > 0 {
        state.top_row_offset = max_top_row_offset;
        viewport = render_viewport(
            lines.lines,
            state.top + 1,
            state.top_row_offset,
            visible_height,
            render_request,
            render_cache,
            active_search_query(state),
        );
    }
    max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        visible_height,
        render_context,
        render_cache,
        tail,
    );
    if state.top_row_offset > max_top_row_offset
        && render_cache.status(state.top + 1).total_rows.is_some()
    {
        state.top_row_offset = max_top_row_offset;
        viewport = render_viewport(
            lines.lines,
            state.top + 1,
            state.top_row_offset,
            visible_height,
            render_request,
            render_cache,
            active_search_query(state),
        );
        max_top_row_offset = effective_top_row_offset(
            state.top + 1,
            visible_height,
            render_context,
            render_cache,
            tail,
        );
    }
    state.top_max_row_offset = max_top_row_offset;

    let current = if file.line_count() == 0 {
        0
    } else {
        state.top + 1
    };
    let bottom = viewport
        .last_line_number
        .unwrap_or(current)
        .min(file.line_count());
    let progress = viewer_progress_percent(file, render_context, bottom, viewport.bottom);
    let styled = viewport.lines;
    let display_mode = display_mode_text(state);
    let title = format!(
        " {} | {} lines | {}-{} | {:>3}% | {} ",
        file.label(),
        file.line_count(),
        current,
        bottom,
        progress,
        display_mode
    );
    let footer_text = if state.search_active {
        format!(
            " search: {} | Enter find | Backspace edit | Esc cancel ",
            state.search_buffer
        )
    } else if !state.jump_buffer.is_empty() {
        format!(
            " go to line: {} / {} | Enter jump | Backspace edit | Esc cancel ",
            state.jump_buffer,
            file.line_count()
        )
    } else if let Some(message) = &state.search_message {
        format!(" {message} | / search | n/N | Esc clear ")
    } else {
        idle_footer_text(state)
    };

    terminal
        .draw(move |frame| {
            let area = frame.area();
            let [body, footer] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
            let paragraph = Paragraph::new(styled).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
            frame.render_widget(paragraph, body);
            frame.render_widget(
                Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        })
        .context("failed to draw terminal frame")?;

    prewarm_render_cache(
        file,
        line_cache,
        render_cache,
        state.top,
        state.top_row_offset,
        visible_height,
        render_request,
    );

    Ok(())
}

fn active_search_query(state: &ViewState) -> Option<&str> {
    (!state.search_query.is_empty()).then_some(state.search_query.as_str())
}

fn idle_footer_text(state: &ViewState) -> String {
    let wrap_hint = if state.wrap { "w unwrap" } else { "w wrap" };
    let position = wrap_position_text(state)
        .map(|position| format!("{position} | "))
        .unwrap_or_default();
    format!(
        " {position}q/Esc quit | {wrap_hint} | / search n/N | wheel/j/k | 123 Enter | Space/f,b "
    )
}

fn display_mode_text(state: &ViewState) -> String {
    if state.wrap {
        return wrap_position_text(state)
            .map(|position| format!("wrap {position}"))
            .unwrap_or_else(|| "wrap".to_owned());
    }

    format!("nowrap x:{}", state.x)
}

fn wrap_position_text(state: &ViewState) -> Option<String> {
    if !state.wrap || state.top_row_offset == 0 {
        return None;
    }

    Some(format!("+{} rows", format_count(state.top_row_offset)))
}

#[derive(Debug, Default)]
struct LineWindowCache {
    start: usize,
    lines: Vec<String>,
}

struct LineWindow<'a> {
    lines: &'a [String],
}

impl LineWindowCache {
    fn read(
        &mut self,
        file: &IndexedTempFile,
        top: usize,
        height: usize,
        margin: usize,
    ) -> Result<LineWindow<'_>> {
        if height == 0 || top >= file.line_count() {
            return Ok(LineWindow { lines: &[] });
        }

        let cached_end = self.start.saturating_add(self.lines.len());
        let requested_end = top.saturating_add(height).min(file.line_count());
        if top >= self.start && requested_end <= cached_end {
            let start = top - self.start;
            let end = requested_end - self.start;
            return Ok(LineWindow {
                lines: &self.lines[start..end],
            });
        }

        let fetch_start = top.saturating_sub(margin);
        let fetch_count = height
            .saturating_add(margin.saturating_mul(2))
            .min(file.line_count().saturating_sub(fetch_start));
        self.lines = file.read_window(fetch_start, fetch_count)?;
        self.start = fetch_start;

        let start = top - self.start;
        let end = requested_end
            .saturating_sub(self.start)
            .min(self.lines.len());
        Ok(LineWindow {
            lines: &self.lines[start..end],
        })
    }
}

#[derive(Debug, Default)]
struct RenderedLineCache {
    request: Option<RenderRequest>,
    lines: HashMap<usize, CachedRenderedLine>,
    order: VecDeque<usize>,
}

#[derive(Debug, Clone)]
struct RenderedVisualRow {
    line: Line<'static>,
    end_byte: usize,
    line_end: bool,
}

#[derive(Debug, Default)]
struct CachedRenderedLine {
    chunks: VecDeque<RenderedLineChunk>,
    total_rows: Option<usize>,
    index: LineRenderIndex,
}

#[derive(Debug)]
struct RenderedLineChunk {
    start_row: usize,
    rows: Vec<RenderedVisualRow>,
}

#[derive(Debug, Clone, Copy)]
struct RenderedLineStatus {
    known_rows: usize,
    total_rows: Option<usize>,
}

#[derive(Debug, Default)]
struct LineRenderIndex {
    wrap: WrapCheckpointIndex,
    highlight: HighlightCheckpointIndex,
}

#[derive(Debug, Default)]
struct WrapCheckpointIndex {
    checkpoints: Vec<WrapCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrapCheckpoint {
    row: usize,
    start_byte: usize,
    start_char: usize,
}

#[derive(Debug, Default)]
struct HighlightCheckpointIndex {
    json_value_strings: Vec<XmlHighlightCheckpoint>,
    xml_lines: Vec<XmlHighlightCheckpoint>,
}

#[derive(Debug, Clone)]
struct XmlHighlightCheckpoint {
    byte: usize,
    state: XmlPairState,
}

impl RenderedLineCache {
    fn get_or_render(
        &mut self,
        line: &str,
        line_number: usize,
        request: RenderRequest,
    ) -> Vec<Line<'static>> {
        self.get_or_render_window(line, line_number, 0, request.row_limit, request)
            .into_iter()
            .map(|row| row.line)
            .collect()
    }

    fn get_or_render_window(
        &mut self,
        line: &str,
        line_number: usize,
        row_start: usize,
        max_rows: usize,
        request: RenderRequest,
    ) -> Vec<RenderedVisualRow> {
        if self.request != Some(request) {
            self.request = Some(request);
            self.lines.clear();
            self.order.clear();
        }

        if max_rows == 0 {
            return Vec::new();
        }

        if !self.lines.contains_key(&line_number) {
            self.evict_until_room();
            self.order.push_back(line_number);
        }

        match self.lines.entry(line_number) {
            Entry::Occupied(mut entry) => {
                entry
                    .get_mut()
                    .render_window(line, line_number, row_start, max_rows, request)
            }
            Entry::Vacant(entry) => {
                let mut cached = CachedRenderedLine::default();
                let rows = cached.render_window(line, line_number, row_start, max_rows, request);
                entry.insert(cached);
                rows
            }
        }
    }

    fn status(&self, line_number: usize) -> RenderedLineStatus {
        self.lines
            .get(&line_number)
            .map(CachedRenderedLine::status)
            .unwrap_or(RenderedLineStatus {
                known_rows: 0,
                total_rows: None,
            })
    }

    fn evict_until_room(&mut self) {
        while self.lines.len() >= RENDER_CACHE_MAX_LINES {
            if let Some(line_number) = self.order.pop_front() {
                self.lines.remove(&line_number);
            } else {
                break;
            }
        }
    }
}

impl CachedRenderedLine {
    fn render_window(
        &mut self,
        line: &str,
        line_number: usize,
        row_start: usize,
        max_rows: usize,
        request: RenderRequest,
    ) -> Vec<RenderedVisualRow> {
        if let Some(rows) = self.cached_window(row_start, max_rows) {
            return rows;
        }

        if self
            .total_rows
            .is_some_and(|total_rows| row_start >= total_rows)
        {
            return Vec::new();
        }

        let chunk_rows = if request.context.wrap {
            max_rows.max(WRAP_RENDER_CHUNK_ROWS)
        } else {
            max_rows
        };
        let rendered = render_logical_line_window_with_status_indexed(
            line,
            line_number,
            row_start,
            chunk_rows,
            request.context,
            &mut self.index,
        );
        if let Some(total_rows) = rendered.total_rows {
            self.total_rows = Some(total_rows);
        }
        if !rendered.rows.is_empty() {
            self.chunks.push_back(RenderedLineChunk {
                start_row: row_start,
                rows: rendered.rows,
            });
            while self.chunks.len() > WRAP_RENDER_CHUNKS_PER_LINE {
                self.chunks.pop_front();
            }
        }

        self.cached_window(row_start, max_rows).unwrap_or_default()
    }

    fn cached_window(&self, row_start: usize, max_rows: usize) -> Option<Vec<RenderedVisualRow>> {
        let desired_end = row_start.saturating_add(max_rows);
        self.chunks.iter().find_map(|chunk| {
            let chunk_end = chunk.start_row.saturating_add(chunk.rows.len());
            if row_start < chunk.start_row || row_start >= chunk_end {
                return None;
            }
            if chunk_end < desired_end
                && self
                    .total_rows
                    .is_none_or(|total_rows| total_rows > chunk_end)
            {
                return None;
            }
            let start = row_start - chunk.start_row;
            let end = start.saturating_add(max_rows).min(chunk.rows.len());
            Some(chunk.rows[start..end].to_vec())
        })
    }

    fn status(&self) -> RenderedLineStatus {
        let known_rows = self
            .chunks
            .iter()
            .map(|chunk| chunk.start_row.saturating_add(chunk.rows.len()))
            .max()
            .unwrap_or(0);
        RenderedLineStatus {
            known_rows,
            total_rows: self.total_rows,
        }
    }
}

impl WrapCheckpointIndex {
    fn start_for(&self, row_start: usize) -> WrapCheckpoint {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.row <= row_start)
            .copied()
            .unwrap_or(WrapCheckpoint {
                row: 0,
                start_byte: 0,
                start_char: 0,
            })
    }

    fn remember(&mut self, checkpoint: WrapCheckpoint) {
        if checkpoint.row == 0 || checkpoint.row % WRAP_CHECKPOINT_INTERVAL_ROWS != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&checkpoint.row, |existing| existing.row)
        {
            Ok(_) => {}
            Err(position) => self.checkpoints.insert(position, checkpoint),
        }
    }
}

impl HighlightCheckpointIndex {
    fn json_value_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.json_value_strings, byte)
    }

    fn xml_line_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.xml_lines, byte)
    }

    fn remember_json_value(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.json_value_strings, byte, state);
    }

    fn remember_xml_line(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.xml_lines, byte, state);
    }
}

fn checkpoint_before(
    checkpoints: &[XmlHighlightCheckpoint],
    byte: usize,
) -> Option<XmlHighlightCheckpoint> {
    checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.byte <= byte)
        .cloned()
}

fn remember_xml_checkpoint(
    checkpoints: &mut Vec<XmlHighlightCheckpoint>,
    byte: usize,
    state: &XmlPairState,
) {
    let next_byte = checkpoints
        .last()
        .map(|checkpoint| {
            checkpoint
                .byte
                .saturating_add(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES)
        })
        .unwrap_or(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES);
    if byte < next_byte {
        return;
    }

    match checkpoints.binary_search_by_key(&byte, |checkpoint| checkpoint.byte) {
        Ok(_) => {}
        Err(position) => checkpoints.insert(
            position,
            XmlHighlightCheckpoint {
                byte,
                state: state.clone(),
            },
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ViewPosition {
    top: usize,
    row_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TailPositionKey {
    line_count: usize,
    visible_height: usize,
    width: usize,
}

#[derive(Debug, Default)]
struct TailPositionCache {
    key: Option<TailPositionKey>,
    position: Option<ViewPosition>,
}

impl TailPositionCache {
    fn position(
        &mut self,
        file: &IndexedTempFile,
        visible_height: usize,
        context: RenderContext,
    ) -> Result<ViewPosition> {
        if !context.wrap {
            return Ok(ViewPosition {
                top: last_full_logical_page_top(file.line_count(), visible_height),
                row_offset: 0,
            });
        }

        let key = TailPositionKey {
            line_count: file.line_count(),
            visible_height,
            width: context.width,
        };
        if self.key == Some(key) {
            if let Some(position) = self.position {
                return Ok(position);
            }
        }

        let position = compute_tail_position(file, visible_height, context)?;
        self.key = Some(key);
        self.position = Some(position);
        Ok(position)
    }
}

fn compute_tail_position(
    file: &IndexedTempFile,
    visible_height: usize,
    context: RenderContext,
) -> Result<ViewPosition> {
    let line_count = file.line_count();
    if line_count == 0 || visible_height == 0 {
        return Ok(ViewPosition {
            top: 0,
            row_offset: 0,
        });
    }

    if !context.wrap {
        return Ok(ViewPosition {
            top: last_full_logical_page_top(line_count, visible_height),
            row_offset: 0,
        });
    }

    let mut needed_rows = visible_height;
    let mut end = line_count;
    while end > 0 {
        let start = end.saturating_sub(visible_height.max(32));
        let lines = file.read_window(start, end - start)?;
        for (index, line) in lines.iter().enumerate().rev() {
            let line_index = start + index;
            let rows = rendered_row_count(line, context);
            if rows >= needed_rows {
                return Ok(ViewPosition {
                    top: line_index,
                    row_offset: rows - needed_rows,
                });
            }
            needed_rows -= rows;
        }
        end = start;
    }

    Ok(ViewPosition {
        top: 0,
        row_offset: 0,
    })
}

fn last_full_logical_page_top(line_count: usize, visible_height: usize) -> usize {
    line_count.saturating_sub(visible_height.max(1))
}

fn is_after_tail(state: &ViewState, tail: ViewPosition) -> bool {
    state.top > tail.top || (state.top == tail.top && state.top_row_offset > tail.row_offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderRequest {
    context: RenderContext,
    row_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderContext {
    gutter_digits: usize,
    x: usize,
    width: usize,
    wrap: bool,
    mode: ViewMode,
}

#[derive(Debug)]
struct RenderedViewport {
    lines: Vec<Line<'static>>,
    last_line_number: Option<usize>,
    bottom: Option<ViewportBottom>,
}

#[derive(Debug, Clone, Copy)]
struct ViewportBottom {
    line_index: usize,
    byte_end: usize,
    line_end: bool,
}

fn render_viewport(
    lines: &[String],
    first_line_number: usize,
    top_row_offset: usize,
    height: usize,
    request: RenderRequest,
    cache: &mut RenderedLineCache,
    search_query: Option<&str>,
) -> RenderedViewport {
    let mut rendered = Vec::with_capacity(height);
    let mut last_line_number = None;

    let Some((top_line, remaining_lines)) = lines.split_first() else {
        return RenderedViewport {
            lines: rendered,
            last_line_number,
            bottom: None,
        };
    };

    let mut bottom = None;
    if height > 0 {
        let top_rows = cache.get_or_render_window(
            top_line,
            first_line_number,
            top_row_offset,
            height.saturating_add(1),
            request,
        );
        if !top_rows.is_empty() {
            last_line_number = Some(first_line_number);
        }
        for row in top_rows.into_iter().take(height) {
            bottom = Some(ViewportBottom {
                line_index: first_line_number - 1,
                byte_end: row.end_byte,
                line_end: row.line_end,
            });
            rendered.push(apply_search_highlight(
                row.line,
                search_query,
                request.context.gutter_digits,
            ));
        }
    }

    for (index, line) in remaining_lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        let line_number = first_line_number + index + 1;
        let rows = cache.get_or_render_window(line, line_number, 0, remaining, request);
        let taken = rows.len().min(remaining);
        if taken > 0 {
            last_line_number = Some(line_number);
        }
        for row in rows.into_iter().take(remaining) {
            bottom = Some(ViewportBottom {
                line_index: line_number - 1,
                byte_end: row.end_byte,
                line_end: row.line_end,
            });
            rendered.push(apply_search_highlight(
                row.line,
                search_query,
                request.context.gutter_digits,
            ));
        }
    }

    RenderedViewport {
        lines: rendered,
        last_line_number,
        bottom,
    }
}

#[cfg(test)]
fn viewport_reaches_file_end(viewport: &RenderedViewport, line_count: usize) -> bool {
    viewport
        .bottom
        .is_some_and(|bottom| bottom.line_end && bottom.line_index + 1 >= line_count)
}

fn exact_top_line_tail_offset(
    lines: &[String],
    visible_height: usize,
    context: RenderContext,
) -> usize {
    if visible_height == 0 || !context.wrap {
        return 0;
    }

    let Some(line) = lines.first() else {
        return 0;
    };

    rendered_row_count(line, context).saturating_sub(visible_height)
}

fn effective_top_row_offset(
    line_number: usize,
    visible_height: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
    tail: Option<ViewPosition>,
) -> usize {
    let mut max_offset = top_line_tail_offset(line_number, visible_height, context, cache);
    if context.wrap
        && let Some(tail) = tail
        && tail.top + 1 == line_number
    {
        max_offset = max_offset.max(tail.row_offset);
    }
    max_offset
}

fn top_line_tail_offset(
    line_number: usize,
    visible_height: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
) -> usize {
    if visible_height == 0 || !context.wrap {
        return 0;
    }

    let status = cache.status(line_number);
    match status.total_rows {
        Some(rows) => rows.saturating_sub(visible_height),
        None if status.known_rows > 0 => usize::MAX,
        None => 0,
    }
}

fn apply_search_highlight(
    line: Line<'static>,
    query: Option<&str>,
    gutter_digits: usize,
) -> Line<'static> {
    let Some(query) = query else {
        return line;
    };
    if query.is_empty() {
        return line;
    }

    let visual_text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let ranges = search_match_ranges(&visual_text, query, gutter_digits + 3);
    if ranges.is_empty() {
        return line;
    }

    Line {
        style: line.style,
        alignment: line.alignment,
        spans: apply_search_ranges_to_spans(&line.spans, &ranges),
    }
}

fn search_match_ranges(text: &str, query: &str, start_char: usize) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let total_chars = text.chars().count();
    if start_char >= total_chars {
        return Vec::new();
    }

    let search_text = slice_chars(text, start_char, total_chars);
    let query_len = query.chars().count();
    search_text
        .match_indices(query)
        .map(|(byte_index, _)| {
            let start = start_char + search_text[..byte_index].chars().count();
            start..start + query_len
        })
        .collect()
}

fn apply_search_ranges_to_spans(
    spans: &[Span<'static>],
    ranges: &[Range<usize>],
) -> Vec<Span<'static>> {
    let mut highlighted = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = text.chars().count();
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let split_points = search_split_points(span_start, span_end, ranges);
        for window in split_points.windows(2) {
            let start = window[0];
            let end = window[1];
            if start == end {
                continue;
            }

            let mut style = span.style;
            if range_is_highlighted(start, end, ranges) {
                style = style.bg(search_match_bg());
            }
            highlighted.push(Span::styled(
                slice_chars(text, start - span_start, end - span_start),
                style,
            ));
        }
    }

    highlighted
}

fn search_split_points(span_start: usize, span_end: usize, ranges: &[Range<usize>]) -> Vec<usize> {
    let mut points = vec![span_start, span_end];
    for range in ranges {
        let start = range.start.max(span_start).min(span_end);
        let end = range.end.max(span_start).min(span_end);
        if start < end {
            points.push(start);
            points.push(end);
        }
    }
    points.sort_unstable();
    points.dedup();
    points
}

fn range_is_highlighted(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| start >= range.start && end <= range.end)
}

fn prewarm_render_cache(
    file: &IndexedTempFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    top: usize,
    top_row_offset: usize,
    visible_height: usize,
    request: RenderRequest,
) {
    if visible_height == 0 || file.line_count() == 0 {
        return;
    }
    if request.context.wrap {
        prewarm_wrapped_render_cache(
            file,
            line_cache,
            render_cache,
            top,
            top_row_offset,
            visible_height,
            request,
        );
        return;
    }

    let side = visible_height.saturating_mul(PREWARM_PAGES);
    let start = top.saturating_sub(side);
    let count = visible_height
        .saturating_add(side.saturating_mul(2))
        .min(PREWARM_MAX_LINES)
        .min(file.line_count().saturating_sub(start));
    let margin = visible_height.saturating_mul(2).max(32);
    let Ok(lines) = line_cache.read(file, start, count, margin) else {
        return;
    };

    let started = Instant::now();
    for (index, line) in lines.lines.iter().enumerate() {
        if line.len() > PREWARM_MAX_LINE_BYTES {
            continue;
        }
        render_cache.get_or_render(line, start + index + 1, request);
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

fn prewarm_wrapped_render_cache(
    file: &IndexedTempFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    top: usize,
    top_row_offset: usize,
    visible_height: usize,
    request: RenderRequest,
) {
    let count = visible_height
        .saturating_add(WRAP_PREWARM_LOGICAL_LINES)
        .min(file.line_count().saturating_sub(top));
    let Ok(lines) = line_cache.read(file, top, count, WRAP_PREWARM_LOGICAL_LINES) else {
        return;
    };

    let started = Instant::now();
    if let Some(line) = lines.lines.first() {
        prewarm_wrapped_line_chunks(
            render_cache,
            line,
            top + 1,
            top_row_offset,
            visible_height,
            request,
        );
        if started.elapsed() >= PREWARM_BUDGET {
            return;
        }
    }

    for (index, line) in lines
        .lines
        .iter()
        .enumerate()
        .skip(1)
        .take(WRAP_PREWARM_LOGICAL_LINES)
    {
        render_cache.get_or_render_window(line, top + index + 1, 0, visible_height, request);
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

fn prewarm_wrapped_line_chunks(
    render_cache: &mut RenderedLineCache,
    line: &str,
    line_number: usize,
    top_row_offset: usize,
    visible_height: usize,
    request: RenderRequest,
) {
    let status = render_cache.status(line_number);
    if status.total_rows.is_none() && status.known_rows > 0 {
        render_cache.get_or_render_window(
            line,
            line_number,
            status.known_rows,
            visible_height,
            request,
        );
    }

    if top_row_offset > 0 {
        let previous = top_row_offset.saturating_sub(WRAP_RENDER_CHUNK_ROWS);
        render_cache.get_or_render_window(line, line_number, previous, visible_height, request);
    }
}

fn render_row_limit(visible_height: usize) -> usize {
    visible_height
        .saturating_mul(2)
        .clamp(32, RENDER_CACHE_MAX_ROWS_PER_LINE)
}

#[cfg(test)]
fn render_logical_line(
    line: &str,
    line_number: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window(line, line_number, 0, max_rows, context)
}

#[derive(Debug)]
struct RenderedLineWindow {
    rows: Vec<RenderedVisualRow>,
    total_rows: Option<usize>,
}

#[cfg(test)]
fn render_logical_line_window(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window_with_status(line, line_number, row_start, max_rows, context)
        .rows
        .into_iter()
        .map(|row| row.line)
        .collect()
}

#[cfg(test)]
fn render_logical_line_window_with_status(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> RenderedLineWindow {
    let mut index = LineRenderIndex::default();
    render_logical_line_window_with_status_indexed(
        line,
        line_number,
        row_start,
        max_rows,
        context,
        &mut index,
    )
}

fn render_logical_line_window_with_status_indexed(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
    index: &mut LineRenderIndex,
) -> RenderedLineWindow {
    if max_rows == 0 {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows: None,
        };
    }

    if !context.wrap {
        if row_start > 0 {
            return RenderedLineWindow {
                rows: Vec::new(),
                total_rows: Some(1),
            };
        }
        return RenderedLineWindow {
            rows: vec![RenderedVisualRow {
                line: styled_segment(
                    line_number_gutter(line_number, context.gutter_digits),
                    line,
                    context.x,
                    context.x.saturating_add(context.width),
                    context.mode,
                ),
                end_byte: byte_index_for_char(
                    line,
                    context
                        .x
                        .saturating_add(context.width)
                        .min(line.chars().count()),
                ),
                line_end: context.x.saturating_add(context.width) >= line.chars().count(),
            }],
            total_rows: Some(1),
        };
    }

    let wrap_window = wrap_ranges_window_indexed(
        line,
        context.width,
        continuation_indent(line, context.width),
        row_start,
        max_rows,
        Some(&mut index.wrap),
    );
    let visible_ranges = wrap_window.ranges;
    let total_rows = wrap_window.total_rows;
    let highlight_end_byte = visible_ranges
        .iter()
        .map(|range| range.end_byte)
        .max()
        .unwrap_or(0);
    let Some(first_range) = visible_ranges.first() else {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows,
        };
    };
    let highlight_start_byte = first_range.start_byte;
    let highlight_start_char = first_range.start_char;
    let spans = highlight_content_window_indexed(
        line,
        context.mode,
        highlight_start_byte,
        highlight_end_byte,
        Some(&mut index.highlight),
    );
    let rows = visible_ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let row_index = row_start + index;
            let gutter = if row_index == 0 {
                line_number_gutter(line_number, context.gutter_digits)
            } else {
                continuation_gutter(row_index, context.gutter_digits)
            };
            let mut line_spans = vec![gutter];
            if range.continuation_indent > 0 {
                line_spans.push(Span::styled(
                    " ".repeat(range.continuation_indent),
                    Style::default(),
                ));
            }
            line_spans.extend(slice_spans(
                &spans,
                range.start_char - highlight_start_char,
                range.end_char - highlight_start_char,
            ));
            RenderedVisualRow {
                line: Line::from(line_spans),
                end_byte: range.end_byte,
                line_end: range.end_byte >= line.len(),
            }
        })
        .collect();
    RenderedLineWindow { rows, total_rows }
}

fn rendered_row_count(line: &str, context: RenderContext) -> usize {
    if !context.wrap {
        return 1;
    }

    wrapped_row_count(
        line,
        context.width,
        continuation_indent(line, context.width),
    )
}

fn wrapped_row_count(line: &str, width: usize, continuation_indent: usize) -> usize {
    if line.is_empty() || width == 0 {
        return 1;
    }

    let mut rows = 0_usize;
    let mut start_byte = 0_usize;
    let mut start_char = 0_usize;
    while start_byte < line.len() {
        let continuation = rows > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        rows = rows.saturating_add(1);
    }

    rows
}

fn styled_segment(
    gutter: Span<'static>,
    line: &str,
    start: usize,
    end: usize,
    mode: ViewMode,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(gutter);
    let highlight_prefix = slice_chars(line, 0, end);
    spans.extend(slice_spans(
        &highlight_content(&highlight_prefix, mode),
        start,
        end,
    ));
    Line::from(spans)
}

fn line_number_gutter(line_number: usize, gutter_digits: usize) -> Span<'static> {
    Span::styled(format!("{line_number:>gutter_digits$} │ "), gutter_style())
}

fn continuation_gutter(row_index: usize, gutter_digits: usize) -> Span<'static> {
    let marker = continuation_gutter_marker(row_index);
    Span::styled(format!("{:>gutter_digits$} {marker} ", ""), gutter_style())
}

fn continuation_gutter_marker(row_index: usize) -> char {
    if row_index > 0 && row_index % WRAP_GUTTER_MAJOR_TICK_ROWS == 0 {
        '┠'
    } else if row_index > 0 && row_index % WRAP_GUTTER_MINOR_TICK_ROWS == 0 {
        '┊'
    } else {
        '┆'
    }
}

fn format_count(value: usize) -> String {
    let raw = value.to_string();
    let mut formatted = String::with_capacity(raw.len() + raw.len() / 3);
    let first_group = raw.len() % 3;
    for (index, ch) in raw.chars().enumerate() {
        if index > 0
            && (index == first_group || (index > first_group && (index - first_group) % 3 == 0))
        {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrapRange {
    start_char: usize,
    end_char: usize,
    start_byte: usize,
    end_byte: usize,
    continuation_indent: usize,
}

#[derive(Debug)]
struct WrapWindow {
    ranges: Vec<WrapRange>,
    total_rows: Option<usize>,
}

#[cfg(test)]
fn wrap_ranges(
    line: &str,
    width: usize,
    continuation_indent: usize,
    max_rows: usize,
) -> Vec<WrapRange> {
    wrap_ranges_window(line, width, continuation_indent, 0, max_rows).ranges
}

#[cfg(test)]
fn wrap_ranges_window(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
) -> WrapWindow {
    wrap_ranges_window_indexed(line, width, continuation_indent, row_start, max_rows, None)
}

fn wrap_ranges_window_indexed(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
    mut checkpoints: Option<&mut WrapCheckpointIndex>,
) -> WrapWindow {
    if max_rows == 0 {
        return WrapWindow {
            ranges: Vec::new(),
            total_rows: None,
        };
    }

    if line.is_empty() || width == 0 {
        return WrapWindow {
            ranges: vec![WrapRange {
                start_char: 0,
                end_char: 0,
                start_byte: 0,
                end_byte: 0,
                continuation_indent: 0,
            }],
            total_rows: Some(1),
        };
    }

    let mut ranges = Vec::new();
    let checkpoint = checkpoints
        .as_deref()
        .map(|checkpoints| checkpoints.start_for(row_start))
        .unwrap_or(WrapCheckpoint {
            row: 0,
            start_byte: 0,
            start_char: 0,
        });
    let mut start_byte = checkpoint.start_byte;
    let mut start_char = checkpoint.start_char;
    let mut row = checkpoint.row;
    let target_end = row_start.saturating_add(max_rows);
    while start_byte < line.len() {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember(WrapCheckpoint {
                row,
                start_byte,
                start_char,
            });
        }
        let continuation = row > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        if row >= row_start && row < target_end {
            ranges.push(WrapRange {
                start_char,
                end_char,
                start_byte,
                end_byte,
                continuation_indent: indent,
            });
        }
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        row = row.saturating_add(1);
        if row >= target_end && start_byte < line.len() {
            return WrapWindow {
                ranges,
                total_rows: None,
            };
        }
    }

    WrapWindow {
        ranges,
        total_rows: Some(row.max(1)),
    }
}

fn next_wrap_end(
    line: &str,
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let hard_byte = start_byte.saturating_add(row_width.max(1)).min(line.len());
    if line.as_bytes()[start_byte..hard_byte].is_ascii() {
        return next_wrap_end_ascii(line.as_bytes(), start_byte, start_char, row_width);
    }

    let min_end = (row_width / 2).max(1);
    let mut consumed = 0_usize;
    let mut hard_end = None;
    let mut best_end = None;

    for (offset, ch) in line[start_byte..].char_indices() {
        if consumed >= row_width {
            break;
        }
        consumed += 1;
        let byte_end = start_byte + offset + ch.len_utf8();
        let char_end = start_char + consumed;
        hard_end = Some((byte_end, char_end));
        if consumed >= min_end && (ch.is_whitespace() || matches!(ch, ',' | '>' | '}' | ']' | ';'))
        {
            best_end = Some((byte_end, char_end));
        }
    }

    let Some(hard_end) = hard_end else {
        return (start_byte, start_char);
    };
    if hard_end.0 >= line.len() {
        return hard_end;
    }
    best_end.unwrap_or(hard_end)
}

fn next_wrap_end_ascii(
    bytes: &[u8],
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let row_width = row_width.max(1);
    let hard_byte = start_byte.saturating_add(row_width).min(bytes.len());
    if hard_byte <= start_byte {
        return (start_byte, start_char);
    }
    if hard_byte >= bytes.len() {
        return (bytes.len(), start_char + (bytes.len() - start_byte));
    }

    let min_byte = start_byte + (row_width / 2).max(1).min(hard_byte - start_byte);
    for index in (min_byte..hard_byte).rev() {
        if is_ascii_wrap_boundary(bytes[index]) {
            let end_byte = index + 1;
            return (end_byte, start_char + (end_byte - start_byte));
        }
    }

    (hard_byte, start_char + (hard_byte - start_byte))
}

fn is_ascii_wrap_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b',' | b'>' | b'}' | b']' | b';')
}

fn continuation_indent(line: &str, width: usize) -> usize {
    if width < 8 {
        return 0;
    }

    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        + 2;
    indent.min(24).min(width / 2)
}

fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    if end <= start {
        return Vec::new();
    }

    let mut sliced = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = text.chars().count();
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let overlap_start = start.max(span_start);
        let overlap_end = end.min(span_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = slice_chars(text, overlap_start - span_start, overlap_end - span_start);
        sliced.push(Span::styled(text, span.style));
    }

    sliced
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}

fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len()
}

fn viewer_progress_percent(
    file: &IndexedTempFile,
    context: RenderContext,
    logical_bottom: usize,
    viewport_bottom: Option<ViewportBottom>,
) -> usize {
    if !context.wrap {
        return progress_percent(logical_bottom, file.line_count());
    }

    let bottom = viewport_bottom
        .map(|bottom| viewport_bottom_byte_offset(file, bottom))
        .unwrap_or(0);
    byte_progress_percent(bottom, file.byte_len())
}

fn viewport_bottom_byte_offset(file: &IndexedTempFile, bottom: ViewportBottom) -> u64 {
    if bottom.line_end {
        if bottom.line_index + 1 >= file.line_count() {
            return file.byte_len();
        }
        return file.byte_offset_for_line(bottom.line_index + 1);
    }

    file.byte_offset_for_line(bottom.line_index)
        .saturating_add(bottom.byte_end as u64)
}

fn byte_index_for_char(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

fn byte_progress_percent(position: u64, total: u64) -> usize {
    if total == 0 {
        return 100;
    }

    position
        .min(total)
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(100) as usize
}

fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    highlight_content_window(line, mode, 0, line.len())
}

fn highlight_content_window(
    line: &str,
    mode: ViewMode,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    highlight_content_window_indexed(line, mode, window_start, window_end, None)
}

fn highlight_content_window_indexed(
    line: &str,
    mode: ViewMode,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let window_start = window_start.min(line.len());
    let window_end = window_end.min(line.len()).max(window_start);
    match mode {
        ViewMode::Plain => highlight_structured_window(line, window_start, window_end, index),
        ViewMode::Diff if line.starts_with("@@") => {
            let mut spans = Vec::new();
            push_span_window(
                &mut spans,
                line,
                0,
                line.len(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                window_start,
                window_end,
            );
            spans
        }
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            let mut spans = Vec::new();
            push_span_window(
                &mut spans,
                line,
                0,
                line.len(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                window_start,
                window_end,
            );
            spans
        }
        ViewMode::Diff if line.starts_with('+') => {
            highlight_diff_payload_window(line, Color::Green, window_start, window_end)
        }
        ViewMode::Diff if line.starts_with('-') => {
            highlight_diff_payload_window(line, Color::Red, window_start, window_end)
        }
        ViewMode::Diff => highlight_structured_window(line, window_start, window_end, index),
    }
}

fn highlight_diff_payload_window(
    line: &str,
    color: Color,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    push_span_window(
        &mut spans,
        line,
        0,
        1,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
        window_start,
        window_end,
    );
    spans.extend(highlight_structured_window(
        &line[1..],
        window_start.saturating_sub(1),
        window_end.saturating_sub(1),
        None,
    ));
    spans
}

fn highlight_structured_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        highlight_xml_line_window(line, window_start, window_end, index)
    } else {
        highlight_json_like_window(line, window_start, window_end, index)
    }
}

#[cfg(test)]
fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    highlight_json_like_window(line, 0, line.len(), None)
}

fn highlight_json_like_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    mut index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    let mut value_string_state = None;
    if let Some(checkpoint_index) = index.as_deref_mut()
        && let Some(checkpoint) = checkpoint_index.json_value_before(window_start)
    {
        cursor = checkpoint.byte;
        value_string_state = Some(checkpoint.state);
    }

    while cursor < line.len() && cursor < window_end {
        if let Some(state) = value_string_state.take() {
            let (end, closed) = highlight_json_value_string_continue_window(
                line,
                cursor,
                window_end,
                state,
                window_start..window_end,
                &mut spans,
                index.as_deref_mut(),
            );
            cursor = end;
            if !closed {
                break;
            }
            continue;
        }

        let rest = &line[cursor..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            let end = take_while(line, cursor, char::is_whitespace);
            push_span_window(
                &mut spans,
                line,
                cursor,
                end,
                Style::default(),
                window_start,
                window_end,
            );
            cursor = end;
            continue;
        }

        if ch == '"' {
            if json_quote_starts_value(line, cursor) {
                let (end, closed) = highlight_json_string_value_window(
                    line,
                    cursor,
                    window_end,
                    window_start,
                    window_end,
                    &mut spans,
                    index.as_deref_mut(),
                );
                cursor = end;
                if !closed {
                    break;
                }
                continue;
            }

            let (end, closed) = json_string_end_until(line, cursor, window_end);
            if closed && json_string_is_key(line, end) {
                push_span_window(
                    &mut spans,
                    line,
                    cursor,
                    end,
                    key_style(),
                    window_start,
                    window_end,
                );
            } else {
                let (end, closed) = highlight_json_string_value_window(
                    line,
                    cursor,
                    window_end,
                    window_start,
                    window_end,
                    &mut spans,
                    index.as_deref_mut(),
                );
                cursor = end;
                if !closed {
                    break;
                }
                continue;
            }
            cursor = end;
            continue;
        }

        if ch == '-' || ch.is_ascii_digit() {
            let end = take_while(line, cursor, is_json_number_char);
            push_span_window(
                &mut spans,
                line,
                cursor,
                end,
                number_style(),
                window_start,
                window_end,
            );
            cursor = end;
            continue;
        }

        if let Some((word, style)) = json_keyword(rest) {
            push_span_window(
                &mut spans,
                line,
                cursor,
                cursor + word.len(),
                style,
                window_start,
                window_end,
            );
            cursor += word.len();
            continue;
        }

        if "{}[]:,".contains(ch) {
            push_span_window(
                &mut spans,
                line,
                cursor,
                cursor + ch.len_utf8(),
                punctuation_style(),
                window_start,
                window_end,
            );
            cursor += ch.len_utf8();
            continue;
        }

        push_span_window(
            &mut spans,
            line,
            cursor,
            cursor + ch.len_utf8(),
            Style::default(),
            window_start,
            window_end,
        );
        cursor += ch.len_utf8();
    }

    spans
}

fn highlight_json_string_value_window(
    source: &str,
    start: usize,
    limit: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
    checkpoints: Option<&mut HighlightCheckpointIndex>,
) -> (usize, bool) {
    let inner_start = start + '"'.len_utf8();
    push_span_window(
        spans,
        source,
        start,
        inner_start,
        string_style(),
        window_start,
        window_end,
    );
    highlight_json_value_string_continue_window(
        source,
        inner_start,
        limit,
        XmlPairState::default(),
        window_start..window_end,
        spans,
        checkpoints,
    )
}

fn highlight_json_value_string_continue_window(
    source: &str,
    start: usize,
    limit: usize,
    mut state: XmlPairState,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
    mut checkpoints: Option<&mut HighlightCheckpointIndex>,
) -> (usize, bool) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = start;
    let mut plain_start = start;
    let limit = limit.min(source.len());

    while index < limit {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember_json_value(index, &state);
        }

        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= limit)
        {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            push_span_window(
                spans,
                source,
                index,
                escape_end,
                escape_style(),
                window_start,
                window_end,
            );
            index = escape_end;
            plain_start = index;
            continue;
        }

        let Some(ch) = source[index..limit].chars().next() else {
            break;
        };

        if ch == '"' {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            let end = index + ch.len_utf8();
            push_span_window(
                spans,
                source,
                index,
                end,
                string_style(),
                window_start,
                window_end,
            );
            return (end, true);
        }

        if ch == '<' {
            let rest = &source[index..limit];
            let tag_end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(limit);
            let tag = &source[index..tag_end];
            if looks_like_xml_tag(tag) {
                push_span_window(
                    spans,
                    source,
                    plain_start,
                    index,
                    string_style(),
                    window_start,
                    window_end,
                );
                if tag_end <= window_start {
                    apply_xml_tag_state(tag, &mut state, 0);
                } else {
                    highlight_xml_tag_window(
                        source,
                        index,
                        tag_end,
                        &mut state,
                        0,
                        window_start..window_end,
                        spans,
                    );
                }
                index = tag_end;
                plain_start = index;
                continue;
            }
        }

        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        limit,
        string_style(),
        window_start,
        window_end,
    );
    (limit, false)
}

fn highlight_string_segment_window(
    source: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if end <= window_start {
        return;
    }

    let mut index = start;
    let mut plain_start = start;

    while index < end {
        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= end)
        {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            push_span_window(
                spans,
                source,
                index,
                escape_end,
                escape_style(),
                window_start,
                window_end,
            );
            index = escape_end;
            plain_start = index;
            continue;
        }

        let Some(ch) = source[index..end].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        end,
        string_style(),
        window_start,
        window_end,
    );
}

#[cfg(test)]
fn highlight_xml_line(line: &str) -> Vec<Span<'static>> {
    highlight_xml_line_window(line, 0, line.len(), None)
}

fn highlight_xml_line_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let base_depth = xml_depth_from_indent(line);
    let mut spans = Vec::new();
    highlight_inline_xml_window_indexed(
        line,
        0,
        line.len(),
        base_depth,
        window_start..window_end,
        &mut spans,
        index,
    );
    spans
}

fn highlight_inline_xml_window_indexed(
    source: &str,
    start: usize,
    end: usize,
    base_depth: usize,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
    mut checkpoints: Option<&mut HighlightCheckpointIndex>,
) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = start;
    let mut state = XmlPairState::default();
    if let Some(checkpoints) = checkpoints.as_deref_mut()
        && let Some(checkpoint) = checkpoints.xml_line_before(window_start)
    {
        index = checkpoint.byte;
        state = checkpoint.state;
    }

    while index < end && index < window_end {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember_xml_line(index, &state);
        }

        let rest = &source[index..end];
        if rest.starts_with('<') {
            let end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(end);
            let tag = &source[index..end];
            if looks_like_xml_tag(tag) {
                if end <= window_start {
                    apply_xml_tag_state(tag, &mut state, base_depth);
                } else {
                    highlight_xml_tag_window(
                        source,
                        index,
                        end,
                        &mut state,
                        base_depth,
                        window_start..window_end,
                        spans,
                    );
                }
            } else if end > window_start {
                highlight_string_segment_window(
                    source,
                    index,
                    end,
                    window_start,
                    window_end,
                    spans,
                );
            }
            index = end;
        } else {
            let end = rest
                .find('<')
                .map(|position| index + position)
                .unwrap_or(end);
            if end > window_start {
                highlight_string_segment_window(
                    source,
                    index,
                    end,
                    window_start,
                    window_end,
                    spans,
                );
            }
            index = end;
        }
    }
}

fn highlight_xml_tag_window(
    source: &str,
    tag_start: usize,
    end: usize,
    state: &mut XmlPairState,
    base_depth: usize,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = 0;
    let tag = &source[tag_start..end];
    let kind = xml_tag_kind(tag);
    let name_range = xml_tag_name_range(tag);
    let name = name_range.map(|(start, end)| &tag[start..end]);
    let tag_state = apply_xml_tag_state_with_parts(state, kind, name, base_depth);

    while index < tag.len() {
        let rest = &tag[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if let Some((name_start, name_end)) = name_range
            && index == name_start
        {
            let style = if tag_state.matched {
                xml_depth_style(tag_state.depth)
            } else {
                error_style()
            };
            push_span_window(
                spans,
                source,
                tag_start + name_start,
                tag_start + name_end,
                style,
                window_start,
                window_end,
            );
            index = name_end;
            continue;
        }

        if ch.is_whitespace() {
            let end = take_while(tag, index, char::is_whitespace);
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + end,
                Style::default(),
                window_start,
                window_end,
            );
            index = end;
            continue;
        }

        if rest.starts_with("\\\"") || rest.starts_with("\\'") {
            let quote = if rest.starts_with("\\\"") { '"' } else { '\'' };
            let end = escaped_quoted_end(tag, index, quote);
            highlight_string_segment_window(
                source,
                tag_start + index,
                tag_start + end,
                window_start,
                window_end,
                spans,
            );
            index = end;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let end = quoted_end(tag, index, ch);
            highlight_string_segment_window(
                source,
                tag_start + index,
                tag_start + end,
                window_start,
                window_end,
                spans,
            );
            index = end;
            continue;
        }

        if "<>/=?!".contains(ch) {
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + index + ch.len_utf8(),
                punctuation_style(),
                window_start,
                window_end,
            );
            index += ch.len_utf8();
            continue;
        }

        if is_xml_name_char(ch) {
            let end = take_while(tag, index, is_xml_name_char);
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + end,
                attr_style(),
                window_start,
                window_end,
            );
            index = end;
            continue;
        }

        push_span_window(
            spans,
            source,
            tag_start + index,
            tag_start + index + ch.len_utf8(),
            Style::default(),
            window_start,
            window_end,
        );
        index += ch.len_utf8();
    }
}

fn apply_xml_tag_state(tag: &str, state: &mut XmlPairState, base_depth: usize) -> XmlTagState {
    let kind = xml_tag_kind(tag);
    let name = xml_tag_name_range(tag).map(|(start, end)| &tag[start..end]);
    apply_xml_tag_state_with_parts(state, kind, name, base_depth)
}

fn apply_xml_tag_state_with_parts(
    state: &mut XmlPairState,
    kind: XmlTagKind,
    name: Option<&str>,
    base_depth: usize,
) -> XmlTagState {
    state.apply(kind, name, base_depth)
}

#[derive(Debug, Clone, Default)]
struct XmlPairState {
    stack: Vec<XmlOpenTag>,
}

#[derive(Debug, Clone)]
struct XmlOpenTag {
    name: String,
    depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XmlTagKind {
    Open,
    Close,
    SelfClosing,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct XmlTagState {
    depth: usize,
    matched: bool,
}

impl XmlPairState {
    fn apply(&mut self, kind: XmlTagKind, name: Option<&str>, base_depth: usize) -> XmlTagState {
        match (kind, name) {
            (XmlTagKind::Open, Some(name)) => {
                let depth = base_depth + self.stack.len();
                self.stack.push(XmlOpenTag {
                    name: name.to_owned(),
                    depth,
                });
                XmlTagState {
                    depth,
                    matched: true,
                }
            }
            (XmlTagKind::SelfClosing, Some(_)) => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
            (XmlTagKind::Close, Some(name)) => match self.stack.pop() {
                Some(open) if open.name == name => XmlTagState {
                    depth: open.depth,
                    matched: true,
                },
                Some(open) => {
                    self.stack.push(open);
                    XmlTagState {
                        depth: base_depth + self.stack.len() - 1,
                        matched: false,
                    }
                }
                None => XmlTagState {
                    depth: base_depth,
                    matched: false,
                },
            },
            _ => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
        }
    }
}

fn looks_like_xml_tag(tag: &str) -> bool {
    tag.starts_with("</")
        || tag.starts_with("<?")
        || tag.starts_with("<!")
        || xml_tag_name_range(tag).is_some()
}

fn xml_tag_kind(tag: &str) -> XmlTagKind {
    if tag.starts_with("</") {
        XmlTagKind::Close
    } else if tag.starts_with("<?") || tag.starts_with("<!") {
        XmlTagKind::Other
    } else if tag.trim_end_matches('>').trim_end().ends_with('/') {
        XmlTagKind::SelfClosing
    } else {
        XmlTagKind::Open
    }
}

fn xml_tag_name_range(tag: &str) -> Option<(usize, usize)> {
    let mut index = if tag.starts_with("</") { 2 } else { 1 };
    while index < tag.len() {
        let ch = tag[index..].chars().next()?;
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }

    let start = index;
    let end = take_while(tag, start, is_xml_name_char);
    (end > start).then_some((start, end))
}

fn xml_depth_from_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        / 2
}

fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

fn json_string_end_until(line: &str, start: usize, limit: usize) -> (usize, bool) {
    if start >= line.len() {
        return (line.len(), false);
    }

    let limit = floor_char_boundary(line, limit.min(line.len()));
    let mut escaped = false;
    let mut index = (start + 1).min(limit);
    while index < limit {
        let Some(ch) = line[index..limit].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return (index + ch.len_utf8(), true);
        }

        index += ch.len_utf8();
    }

    (limit, false)
}

fn json_quote_starts_value(line: &str, quote_start: usize) -> bool {
    line[..quote_start]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, ':' | '['))
}

fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..].trim_start().starts_with(':')
}

fn is_json_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')
}

fn json_keyword(rest: &str) -> Option<(&str, Style)> {
    for keyword in ["true", "false"] {
        if rest.starts_with(keyword) && keyword_boundary(rest, keyword.len()) {
            return Some((keyword, bool_style()));
        }
    }

    if rest.starts_with("null") && keyword_boundary(rest, "null".len()) {
        Some(("null", null_style()))
    } else {
        None
    }
}

fn keyword_boundary(rest: &str, end: usize) -> bool {
    rest[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

fn escaped_quoted_end(text: &str, start: usize, quote: char) -> usize {
    let pattern = if quote == '"' { "\\\"" } else { "\\'" };
    text[start + pattern.len()..]
        .find(pattern)
        .map(|offset| start + pattern.len() + offset + pattern.len())
        .unwrap_or(text.len())
}

fn escape_token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    if !rest.starts_with('\\') {
        return None;
    }

    let mut chars = rest.chars();
    chars.next()?;
    let escaped = chars.next()?;
    let escaped_start = start + '\\'.len_utf8();
    let escaped_end = escaped_start + escaped.len_utf8();

    if escaped == 'u' {
        let unicode_end = escaped_end + 4;
        if text
            .get(escaped_end..unicode_end)
            .is_some_and(|digits| digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        {
            return Some(unicode_end);
        }
    }

    Some(escaped_end)
}

fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}

fn push_span_window(
    spans: &mut Vec<Span<'static>>,
    source: &str,
    start: usize,
    end: usize,
    style: Style,
    window_start: usize,
    window_end: usize,
) {
    let start = floor_char_boundary(source, start.min(source.len()));
    let end = floor_char_boundary(source, end.min(source.len()));
    let window_start = floor_char_boundary(source, window_start.min(source.len()));
    let window_end = floor_char_boundary(source, window_end.min(source.len()));
    let overlap_start = start.max(window_start);
    let overlap_end = end.min(window_end);
    if overlap_start < overlap_end {
        spans.push(Span::styled(
            source[overlap_start..overlap_end].to_owned(),
            style,
        ));
    }
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn gutter_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn punctuation_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn key_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn xml_depth_style(depth: usize) -> Style {
    const COLORS: [Color; 6] = [
        Color::Cyan,
        Color::Magenta,
        Color::Yellow,
        Color::Green,
        Color::Blue,
        Color::LightCyan,
    ];

    Style::default()
        .fg(COLORS[depth % COLORS.len()])
        .add_modifier(Modifier::BOLD)
}

fn attr_style() -> Style {
    Style::default().fg(Color::Yellow)
}

fn string_style() -> Style {
    Style::default().fg(Color::Green)
}

fn escape_style() -> Style {
    Style::default()
        .fg(Color::LightMagenta)
        .add_modifier(Modifier::BOLD)
}

fn number_style() -> Style {
    Style::default().fg(Color::Magenta)
}

fn bool_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn null_style() -> Style {
    Style::default().fg(Color::Blue)
}

fn error_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

fn search_match_bg() -> Color {
    Color::Rgb(222, 196, 121)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn slices_by_character_not_byte() {
        assert_eq!(slice_chars("a路径b", 1, 3), "路径");
    }

    #[test]
    fn styled_line_keeps_a_gutter() {
        let line = render_logical_line(
            r#"  "name": "fmtview","#,
            12,
            1,
            RenderContext {
                gutter_digits: 3,
                x: 0,
                width: 80,
                wrap: false,
                mode: ViewMode::Plain,
            },
        )
        .remove(0);
        assert_eq!(span_text(&line.spans), r#" 12 │   "name": "fmtview","#);
    }

    #[test]
    fn wrap_uses_continuation_gutter_and_indent() {
        let lines = render_logical_line(
            r#"  "payload": "abcdefghijklmnopqrstuvwxyz","#,
            7,
            3,
            RenderContext {
                gutter_digits: 2,
                x: 0,
                width: 18,
                wrap: true,
                mode: ViewMode::Plain,
            },
        );

        assert!(lines.len() > 1);
        assert!(span_text(&lines[0].spans).starts_with(" 7 │ "));
        assert!(span_text(&lines[1].spans).starts_with("   ┆     "));
    }

    #[test]
    fn continuation_gutter_marks_deep_wrapped_offsets() {
        assert_eq!(span_text(&[continuation_gutter(1, 1)]), "  ┆ ");
        assert_eq!(span_text(&[continuation_gutter(8, 1)]), "  ┊ ");
        assert_eq!(span_text(&[continuation_gutter(64, 1)]), "  ┠ ");
    }

    #[test]
    fn nowrap_applies_horizontal_offset() {
        let lines = render_logical_line(
            "abcdef",
            1,
            1,
            RenderContext {
                gutter_digits: 1,
                x: 2,
                width: 3,
                wrap: false,
                mode: ViewMode::Plain,
            },
        );

        assert_eq!(span_text(&lines[0].spans), "1 │ cde");
    }

    #[test]
    fn mouse_wheel_scrolls_by_logical_line() {
        let mut state = ViewState::default();
        let action = handle_event(
            mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE),
            &mut state,
            10,
            5,
        );

        assert!(action.dirty);
        assert!(!action.quit);
        assert_eq!(state.top, MOUSE_SCROLL_LINES);

        let action = handle_event(
            mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE),
            &mut state,
            10,
            5,
        );

        assert!(action.dirty);
        assert_eq!(state.top, 0);
    }

    #[test]
    fn down_scrolls_inside_overflowing_wrapped_line_first() {
        let mut state = ViewState {
            top_max_row_offset: 2,
            ..ViewState::default()
        };

        let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

        assert!(action.dirty);
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, 1);
        assert!(!state.wrap_bounds_stale);

        state.top_row_offset = state.top_max_row_offset;
        let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

        assert!(action.dirty);
        assert_eq!(state.top, 1);
        assert_eq!(state.top_row_offset, 0);
        assert!(state.wrap_bounds_stale);
    }

    #[test]
    fn batched_scroll_stops_after_crossing_to_unmeasured_wrapped_line() {
        let mut state = ViewState::default();

        assert!(scroll_down_by(&mut state, 10, 3));

        assert_eq!(state.top, 1);
        assert_eq!(state.top_row_offset, 0);
        assert!(state.wrap_bounds_stale);
    }

    #[test]
    fn up_from_logical_line_moves_to_previous_line_tail() {
        let mut state = ViewState {
            top: 1,
            ..ViewState::default()
        };

        let action = handle_key_event(KeyCode::Up, KeyModifiers::NONE, &mut state, 3, 5);

        assert!(action.dirty);
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, TAIL_ROW_OFFSET);
        assert!(state.wrap_bounds_stale);
    }

    #[test]
    fn viewport_can_start_inside_wrapped_logical_line() {
        let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 4,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let mut cache = RenderedLineCache::default();

        let first = render_viewport(&lines, 1, 0, 2, request, &mut cache, None);
        assert_eq!(first.last_line_number, Some(1));
        assert_eq!(span_text(&first.lines[0].spans), "1 │ abcd");
        assert_eq!(span_text(&first.lines[1].spans), "  ┆ efgh");

        let second = render_viewport(&lines, 1, 1, 2, request, &mut cache, None);
        assert_eq!(second.last_line_number, Some(1));
        assert_eq!(span_text(&second.lines[0].spans), "  ┆ efgh");
        assert_eq!(span_text(&second.lines[1].spans), "  ┆ ijkl");
    }

    #[test]
    fn viewport_reports_actual_last_logical_line() {
        let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 4,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let mut cache = RenderedLineCache::default();

        let viewport = render_viewport(&lines, 1, 2, 3, request, &mut cache, None);

        assert_eq!(viewport.last_line_number, Some(2));
        assert_eq!(span_text(&viewport.lines[0].spans), "  ┆ ijkl");
        assert_eq!(span_text(&viewport.lines[1].spans), "2 │ next");
    }

    #[test]
    fn wrapped_progress_advances_by_visible_bytes() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "abcdefghijkl").unwrap();
        writeln!(temp, "next").unwrap();
        let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };

        assert_eq!(
            viewer_progress_percent(
                &file,
                context,
                1,
                Some(ViewportBottom {
                    line_index: 0,
                    byte_end: 8,
                    line_end: false,
                }),
            ),
            44
        );

        assert_eq!(
            viewer_progress_percent(
                &file,
                context,
                1,
                Some(ViewportBottom {
                    line_index: 0,
                    byte_end: 12,
                    line_end: true,
                }),
            ),
            72
        );

        assert_eq!(
            viewer_progress_percent(
                &file,
                context,
                2,
                Some(ViewportBottom {
                    line_index: 1,
                    byte_end: 4,
                    line_end: true,
                }),
            ),
            100
        );
    }

    #[test]
    fn tail_position_keeps_nowrap_last_page_full() {
        assert_eq!(last_full_logical_page_top(10, 3), 7);
        assert_eq!(last_full_logical_page_top(2, 5), 0);
    }

    #[test]
    fn wrapped_tail_position_can_start_inside_last_line() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "prev").unwrap();
        writeln!(temp, "abcdefghijkl").unwrap();
        let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };

        let tail = compute_tail_position(&file, 2, context).unwrap();

        assert_eq!(
            tail,
            ViewPosition {
                top: 1,
                row_offset: 1
            }
        );
    }

    #[test]
    fn wrapped_tail_view_renders_last_full_page() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "{{").unwrap();
        writeln!(temp, "abcdefghijkl").unwrap();
        writeln!(temp, "}}").unwrap();
        let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };
        let request = RenderRequest {
            context,
            row_limit: 8,
        };
        let tail = compute_tail_position(&file, 3, context).unwrap();
        let lines = file.read_window(tail.top, 3).unwrap();
        let mut cache = RenderedLineCache::default();

        let viewport = render_viewport(
            &lines,
            tail.top + 1,
            tail.row_offset,
            3,
            request,
            &mut cache,
            None,
        );

        assert_eq!(
            tail,
            ViewPosition {
                top: 1,
                row_offset: 1
            }
        );
        assert_eq!(viewport.lines.len(), 3);
        assert_eq!(viewport.last_line_number, Some(3));
        assert!(viewport_reaches_file_end(&viewport, file.line_count()));
        assert!(
            tail.row_offset > top_line_tail_offset(tail.top + 1, 3, context, &cache),
            "global file tail may need a deeper offset than the top line's own full-page tail"
        );
        assert_eq!(
            effective_top_row_offset(tail.top + 1, 3, context, &cache, Some(tail)),
            tail.row_offset
        );
        assert_eq!(span_text(&viewport.lines[0].spans), "  ┆ efgh");
        assert_eq!(span_text(&viewport.lines[1].spans), "  ┆ ijkl");
        assert_eq!(span_text(&viewport.lines[2].spans), "3 │ }");
    }

    #[test]
    fn eof_wrap_offset_clamps_to_last_full_page() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "abcdefghijkl").unwrap();
        let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };
        let request = RenderRequest {
            context,
            row_limit: 8,
        };
        let tail = compute_tail_position(&file, 2, context).unwrap();
        let lines = file.read_window(0, 2).unwrap();
        let mut cache = RenderedLineCache::default();

        let partial = render_viewport(&lines, 1, 2, 2, request, &mut cache, None);
        let max_offset = effective_top_row_offset(1, 2, context, &cache, Some(tail));
        let clamped = render_viewport(&lines, 1, max_offset, 2, request, &mut cache, None);
        let progress = viewer_progress_percent(&file, context, 1, clamped.bottom);

        assert_eq!(
            tail,
            ViewPosition {
                top: 0,
                row_offset: 1
            }
        );
        assert!(viewport_reaches_file_end(&partial, file.line_count()));
        assert_eq!(partial.lines.len(), 1);
        assert_eq!(max_offset, 1);
        assert_eq!(clamped.lines.len(), 2);
        assert_eq!(progress, 100);
        assert_eq!(span_text(&clamped.lines[0].spans), "  ┆ efgh");
        assert_eq!(span_text(&clamped.lines[1].spans), "  ┆ ijkl");
    }

    #[test]
    fn page_down_clamps_to_known_wrapped_tail() {
        let mut state = ViewState {
            top_max_row_offset: 5,
            ..ViewState::default()
        };

        let action = handle_key_event(KeyCode::PageDown, KeyModifiers::NONE, &mut state, 3, 10);

        assert!(action.dirty);
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, 5);
    }

    #[test]
    fn top_line_tail_offset_points_to_last_full_view() {
        let lines = ["abcdefghijklmnop".to_owned()];
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };
        let request = RenderRequest {
            context,
            row_limit: 8,
        };
        let mut cache = RenderedLineCache::default();
        cache.get_or_render_window(&lines[0], 1, 0, 8, request);

        assert_eq!(top_line_tail_offset(1, 2, context, &cache), 2);
    }

    #[test]
    fn unknown_wrapped_tail_keeps_scrolling_inside_current_line() {
        let line = "a".repeat((WRAP_RENDER_CHUNK_ROWS + 10) * 4);
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        };
        let request = RenderRequest {
            context,
            row_limit: 8,
        };
        let mut cache = RenderedLineCache::default();
        cache.get_or_render_window(&line, 1, 0, 8, request);
        let mut state = ViewState {
            top_max_row_offset: top_line_tail_offset(1, 2, context, &cache),
            ..ViewState::default()
        };

        assert_eq!(state.top_max_row_offset, usize::MAX);
        assert!(scroll_down_by(&mut state, 2, WRAP_RENDER_CHUNK_ROWS + 1));
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, WRAP_RENDER_CHUNK_ROWS + 1);
        assert!(!state.wrap_bounds_stale);
    }

    #[test]
    fn footer_wrap_hint_matches_current_mode() {
        let state = ViewState::default();
        assert!(idle_footer_text(&state).contains("w unwrap"));

        let state = ViewState {
            wrap: false,
            ..ViewState::default()
        };
        assert!(idle_footer_text(&state).contains("w wrap"));
    }

    #[test]
    fn wrap_position_appears_in_mode_and_footer() {
        let state = ViewState {
            top_row_offset: 12_480,
            ..ViewState::default()
        };

        assert_eq!(display_mode_text(&state), "wrap +12,480 rows");
        assert!(idle_footer_text(&state).starts_with(" +12,480 rows | "));
    }

    #[test]
    fn end_key_targets_wrapped_file_tail_even_on_last_line() {
        let mut state = ViewState::default();

        let action = handle_key_event(KeyCode::End, KeyModifiers::NONE, &mut state, 1, 10);

        assert!(action.dirty);
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, TAIL_ROW_OFFSET);
        assert!(state.wrap_bounds_stale);
    }

    #[test]
    fn digits_plus_enter_jumps_to_line_number() {
        let mut state = ViewState::default();

        handle_key_event(KeyCode::Char('1'), KeyModifiers::NONE, &mut state, 100, 10);
        handle_key_event(KeyCode::Char('2'), KeyModifiers::NONE, &mut state, 100, 10);
        assert_eq!(state.jump_buffer, "12");
        assert_eq!(state.top, 0);

        let action = handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 100, 10);

        assert!(action.dirty);
        assert!(!action.quit);
        assert_eq!(state.jump_buffer, "");
        assert_eq!(state.top, 11);
    }

    #[test]
    fn line_jump_clamps_to_valid_range() {
        let mut state = ViewState::default();

        for ch in "999".chars() {
            handle_key_event(KeyCode::Char(ch), KeyModifiers::NONE, &mut state, 5, 10);
        }
        handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 5, 10);
        assert_eq!(state.top, 4);

        handle_key_event(KeyCode::Char('0'), KeyModifiers::NONE, &mut state, 5, 10);
        handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 5, 10);
        assert_eq!(state.top, 0);
    }

    #[test]
    fn line_jump_supports_backspace_and_escape_cancel() {
        let mut state = ViewState::default();

        handle_key_event(KeyCode::Char('4'), KeyModifiers::NONE, &mut state, 20, 10);
        handle_key_event(KeyCode::Char('2'), KeyModifiers::NONE, &mut state, 20, 10);
        let action = handle_key_event(KeyCode::Backspace, KeyModifiers::NONE, &mut state, 20, 10);
        assert!(action.dirty);
        assert_eq!(state.jump_buffer, "4");

        let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 20, 10);
        assert!(action.dirty);
        assert!(!action.quit);
        assert_eq!(state.jump_buffer, "");

        let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 20, 10);
        assert!(!action.dirty);
        assert!(action.quit);
    }

    #[test]
    fn ctrl_d_and_ctrl_u_are_not_bound() {
        let mut state = ViewState {
            top: 10,
            ..ViewState::default()
        };

        let action = handle_key_event(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            &mut state,
            100,
            20,
        );
        assert!(!action.dirty);
        assert_eq!(state.top, 10);

        let action = handle_key_event(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            &mut state,
            100,
            20,
        );
        assert!(!action.dirty);
        assert_eq!(state.top, 10);
    }

    #[test]
    fn slash_search_finds_and_repeats_matches() {
        let file = indexed_lines(&["alpha", "beta needle", "gamma", "needle again"]);
        let mut state = ViewState::default();

        handle_key_event(KeyCode::Char('/'), KeyModifiers::NONE, &mut state, 4, 10);
        for ch in "needle".chars() {
            handle_key_event(KeyCode::Char(ch), KeyModifiers::NONE, &mut state, 4, 10);
        }
        assert!(state.search_active);

        handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 4, 10);
        assert!(!state.search_active);
        assert_eq!(state.search_query, "needle");
        assert!(state.search_task.is_some());

        assert!(process_search_step(&file, &mut state).unwrap());
        assert_eq!(state.top, 1);
        assert_eq!(state.search_message.as_deref(), Some("match: needle"));

        handle_key_event(KeyCode::Char('n'), KeyModifiers::NONE, &mut state, 4, 10);
        assert!(process_search_step(&file, &mut state).unwrap());
        assert_eq!(state.top, 3);

        handle_key_event(KeyCode::Char('N'), KeyModifiers::NONE, &mut state, 4, 10);
        assert!(process_search_step(&file, &mut state).unwrap());
        assert_eq!(state.top, 1);
    }

    #[test]
    fn search_reports_not_found_and_can_clear_message() {
        let file = indexed_lines(&["alpha", "beta"]);
        let mut state = ViewState::default();

        start_search(
            &mut state,
            "missing".to_owned(),
            SearchDirection::Forward,
            0,
            file.line_count(),
        );
        assert!(process_search_step(&file, &mut state).unwrap());

        assert_eq!(state.top, 0);
        assert_eq!(state.search_message.as_deref(), Some("not found: missing"));

        let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 2, 10);
        assert!(action.dirty);
        assert!(!action.quit);
        assert_eq!(state.search_message, None);
    }

    #[test]
    fn repeated_search_wraps_around_file_edges() {
        let file = indexed_lines(&["needle first", "middle", "needle last"]);
        let mut state = ViewState {
            top: 2,
            search_query: "needle".to_owned(),
            ..ViewState::default()
        };

        handle_key_event(
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut state,
            file.line_count(),
            10,
        );
        assert!(process_search_step(&file, &mut state).unwrap());
        assert_eq!(state.top, 0);

        handle_key_event(
            KeyCode::Char('N'),
            KeyModifiers::NONE,
            &mut state,
            file.line_count(),
            10,
        );
        assert!(process_search_step(&file, &mut state).unwrap());
        assert_eq!(state.top, 2);
    }

    #[test]
    fn search_highlight_adds_background_without_replacing_foreground() {
        let line = render_logical_line(
            r#"  "needle": "needle","#,
            1,
            1,
            RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 80,
                wrap: false,
                mode: ViewMode::Plain,
            },
        )
        .remove(0);

        let highlighted = apply_search_highlight(line, Some("needle"), 1);
        let styles = styles_for_text(&highlighted.spans, "needle");

        assert_eq!(styles.len(), 2);
        assert!(
            styles
                .iter()
                .all(|style| style.bg == Some(search_match_bg()))
        );
        assert!(styles.iter().any(|style| style.fg == Some(Color::Cyan)));
        assert!(styles.iter().any(|style| style.fg == Some(Color::Green)));
    }

    #[test]
    fn shifted_wheel_scrolls_horizontally_in_nowrap() {
        let mut state = ViewState {
            wrap: false,
            ..ViewState::default()
        };
        let action = handle_event(
            mouse_event(MouseEventKind::ScrollDown, KeyModifiers::SHIFT),
            &mut state,
            10,
            5,
        );

        assert!(action.dirty);
        assert_eq!(state.top, 0);
        assert_eq!(state.x, MOUSE_HORIZONTAL_COLUMNS);

        let action = handle_event(
            mouse_event(MouseEventKind::ScrollUp, KeyModifiers::SHIFT),
            &mut state,
            10,
            5,
        );

        assert!(action.dirty);
        assert_eq!(state.x, 0);
    }

    #[test]
    fn rendered_line_cache_reuses_until_context_changes() {
        let mut cache = RenderedLineCache::default();
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 3,
                wrap: false,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };

        let first = {
            let rows = cache.get_or_render("abcdef", 1, request);
            span_text(&rows[0].spans)
        };
        assert_eq!(first, "1 │ abc");

        cache.get_or_render("abcdef", 1, request);
        assert_eq!(cache.lines.len(), 1);

        let shifted = RenderRequest {
            context: RenderContext {
                x: 2,
                ..request.context
            },
            ..request
        };
        let second = {
            let rows = cache.get_or_render("abcdef", 1, shifted);
            span_text(&rows[0].spans)
        };

        assert_eq!(second, "1 │ cde");
        assert_eq!(cache.lines.len(), 1);
    }

    #[test]
    fn wrapped_render_cache_reuses_adjacent_rows_from_chunk() {
        let mut cache = RenderedLineCache::default();
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 4,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let line = "a".repeat(4096);

        let first = cache.get_or_render_window(&line, 1, 100, 2, request);
        assert_eq!(first.len(), 2);
        assert_eq!(cache.lines.get(&1).unwrap().chunks.len(), 1);

        let second = cache.get_or_render_window(&line, 1, 101, 2, request);
        assert_eq!(second.len(), 2);
        assert_eq!(cache.lines.get(&1).unwrap().chunks.len(), 1);
    }

    #[test]
    fn wrapped_render_cache_records_deep_checkpoints() {
        let mut cache = RenderedLineCache::default();
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 16,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let line = format!(
            r#"  "xml": "<root>{}</root>""#,
            r#"<item><name>visible</name></item>"#.repeat(2_000)
        );

        let rows = cache.get_or_render_window(&line, 1, 3_000, 4, request);
        assert_eq!(rows.len(), 4);

        let cached = cache.lines.get(&1).unwrap();
        assert!(
            cached.index.wrap.checkpoints.len() > 4,
            "deep wrapped render should leave reusable row checkpoints"
        );
        assert!(
            !cached.index.highlight.json_value_strings.is_empty(),
            "deep JSON string render should leave XML state checkpoints"
        );

        let checkpointed =
            cache.get_or_render_window(&line, 1, 3_000 + WRAP_RENDER_CHUNK_ROWS + 8, 4, request);
        assert_eq!(checkpointed.len(), 4);
    }

    #[test]
    fn wrapped_deep_window_keeps_embedded_xml_pair_colors() {
        let mut cache = RenderedLineCache::default();
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 12,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let line = format!(
            r#"  "xml": "{}<root><child>visible</child></root>""#,
            "x".repeat(480)
        );
        let row_start = wrap_ranges(
            &line,
            request.context.width,
            continuation_indent(&line, request.context.width),
            80,
        )
        .iter()
        .position(|range| line[range.start_byte..range.end_byte].contains("<child>"))
        .unwrap();

        let rows = cache.get_or_render_window(&line, 1, row_start, 3, request);
        let spans = rows
            .iter()
            .flat_map(|row| row.line.spans.iter().cloned())
            .collect::<Vec<_>>();
        let child_styles = styles_for_text(&spans, "child");

        assert_eq!(child_styles.len(), 2);
        assert_eq!(child_styles[0], child_styles[1]);
    }

    #[test]
    fn wrapped_deep_window_keeps_prefix_xml_state_for_visible_close_tag() {
        let mut cache = RenderedLineCache::default();
        let request = RenderRequest {
            context: RenderContext {
                gutter_digits: 1,
                x: 0,
                width: 12,
                wrap: true,
                mode: ViewMode::Plain,
            },
            row_limit: 8,
        };
        let line = format!(
            r#"  "xml": "<root><child>{}</child></root>""#,
            "x".repeat(480)
        );
        let row_start = wrap_ranges(
            &line,
            request.context.width,
            continuation_indent(&line, request.context.width),
            120,
        )
        .iter()
        .position(|range| line[range.start_byte..range.end_byte].contains("</child>"))
        .unwrap();

        let rows = cache.get_or_render_window(&line, 1, row_start, 2, request);
        let spans = rows
            .iter()
            .flat_map(|row| row.line.spans.iter().cloned())
            .collect::<Vec<_>>();
        let child_styles = styles_for_text(&spans, "child");

        assert_eq!(child_styles, vec![xml_depth_style(1)]);
    }

    #[test]
    #[ignore = "performance smoke; run with cargo test --release perf_huge_wrapped_line_paths -- --ignored --nocapture"]
    fn perf_huge_wrapped_line_paths() {
        let message = format!(
            r#"  "message": "<root>{}</root>""#,
            r#"<item id=\"1\"><name>visible</name></item>"#.repeat(600_000)
        );
        let context = RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 94,
            wrap: true,
            mode: ViewMode::Plain,
        };

        let started = Instant::now();
        let rows = render_logical_line_window_with_status(&message, 5, 0, 27, context);
        let first_window = started.elapsed();
        eprintln!("huge wrapped first-window render: {first_window:?}");
        assert_eq!(rows.rows.len(), 27);
        assert!(
            first_window < Duration::from_millis(1_000),
            "first-window render took {first_window:?}"
        );

        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "{{").unwrap();
        writeln!(temp, r#"  "id": 1,"#).unwrap();
        writeln!(temp, r#"  "kind": "huge-single-line-xml-message","#).unwrap();
        writeln!(temp, r#"  "repeats": 600000,"#).unwrap();
        writeln!(temp, "{message}").unwrap();
        writeln!(temp, "}}").unwrap();
        let file = IndexedTempFile::new("huge".to_owned(), temp).unwrap();

        let started = Instant::now();
        let visible_height = 27;
        let tail = compute_tail_position(&file, visible_height, context).unwrap();
        let tail_elapsed = started.elapsed();
        eprintln!("huge wrapped tail position: {tail_elapsed:?}");
        assert_eq!(tail.top, 4);
        assert!(
            tail_elapsed < Duration::from_millis(1_000),
            "tail position took {tail_elapsed:?}"
        );

        let request = RenderRequest {
            context,
            row_limit: render_row_limit(visible_height),
        };
        let lines = file.read_window(tail.top, visible_height).unwrap();
        let mut cache = RenderedLineCache::default();
        let started = Instant::now();
        let viewport = render_viewport(
            &lines,
            tail.top + 1,
            tail.row_offset,
            visible_height,
            request,
            &mut cache,
            None,
        );
        let tail_render = started.elapsed();
        eprintln!("huge wrapped tail-window render: {tail_render:?}");
        assert_eq!(viewport.lines.len(), visible_height);
        assert_eq!(viewport.last_line_number, Some(6));
        assert!(
            tail_render < Duration::from_millis(1_000),
            "tail-window render took {tail_render:?}"
        );

        let checkpointed_row = tail.row_offset.saturating_sub(WRAP_RENDER_CHUNK_ROWS * 2);
        let started = Instant::now();
        let checkpointed_rows = cache.get_or_render_window(
            &lines[0],
            tail.top + 1,
            checkpointed_row,
            visible_height,
            request,
        );
        let checkpointed_render = started.elapsed();
        eprintln!("huge wrapped checkpointed-window render: {checkpointed_render:?}");
        assert_eq!(checkpointed_rows.len(), visible_height);
        assert!(
            checkpointed_render < Duration::from_millis(200),
            "checkpointed-window render took {checkpointed_render:?}"
        );
    }

    #[test]
    fn json_highlight_preserves_visible_text() {
        let spans = highlight_json_like(r#"  "ok": true, "n": 42, "none": null"#);
        assert_eq!(span_text(&spans), r#"  "ok": true, "n": 42, "none": null"#);
    }

    #[test]
    fn json_string_escape_tokens_are_highlighted() {
        let spans = highlight_json_like(r#"  "text": "line\nnext\t\u263A\\done""#);
        assert_eq!(span_text(&spans), r#"  "text": "line\nnext\t\u263A\\done""#);

        assert_eq!(styles_for_text(&spans, r#"\n"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\t"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\u263A"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\\"#), vec![escape_style()]);
    }

    #[test]
    fn xml_highlight_preserves_visible_text() {
        let spans = highlight_xml_line(r#"<root id="1"><child>value</child></root>"#);
        assert_eq!(
            span_text(&spans),
            r#"<root id="1"><child>value</child></root>"#
        );
    }

    #[test]
    fn embedded_xml_string_uses_tag_pairing() {
        let spans = highlight_json_like(r#"  "xml": "<root><child id=\"1\">v</child></root>""#);
        assert_eq!(
            span_text(&spans),
            r#"  "xml": "<root><child id=\"1\">v</child></root>""#
        );

        let root_styles = styles_for_text(&spans, "root");
        assert_eq!(root_styles.len(), 2);
        assert_eq!(root_styles[0], root_styles[1]);

        let child_styles = styles_for_text(&spans, "child");
        assert_eq!(child_styles.len(), 2);
        assert_eq!(child_styles[0], child_styles[1]);
        assert_ne!(root_styles[0], child_styles[0]);
        assert_eq!(
            styles_for_text(&spans, r#"\""#),
            vec![escape_style(), escape_style()]
        );
    }

    #[test]
    fn mismatched_inline_xml_tag_is_marked() {
        let spans = highlight_json_like(r#"  "xml": "<root></child>""#);
        let child_styles = styles_for_text(&spans, "child");
        assert_eq!(child_styles, vec![error_style()]);
    }

    #[test]
    fn unmatched_inline_xml_close_tag_is_marked() {
        let spans = highlight_json_like(r#"  "xml": "</child>""#);
        let child_styles = styles_for_text(&spans, "child");
        assert_eq!(child_styles, vec![error_style()]);
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
        spans
            .iter()
            .filter(|span| span.content.as_ref() == text)
            .map(|span| span.style)
            .collect()
    }

    fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> Event {
        Event::Mouse(crossterm::event::MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers,
        })
    }

    fn indexed_lines(lines: &[&str]) -> IndexedTempFile {
        let mut temp = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(temp, "{line}").unwrap();
        }
        IndexedTempFile::new("test".to_owned(), temp).unwrap()
    }
}
