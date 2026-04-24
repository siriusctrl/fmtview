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
const PREWARM_PAGES: usize = 2;
const PREWARM_MAX_LINES: usize = 192;
const PREWARM_BUDGET: Duration = Duration::from_millis(4);
const JUMP_BUFFER_MAX_DIGITS: usize = 20;
const SEARCH_CHUNK_LINES: usize = 4096;

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
        action.merge(handle_event(event, state, line_count, page));
        processed += 1;

        if action.quit
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
            true
        }
        KeyCode::Down | KeyCode::Char('j') => scroll_by(&mut state.top, line_count, 1),
        KeyCode::Up | KeyCode::Char('k') => scroll_by(&mut state.top, line_count, -1),
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
            scroll_by(&mut state.top, line_count, page as isize)
        }
        KeyCode::PageUp | KeyCode::Char('b') => {
            scroll_by(&mut state.top, line_count, -(page as isize))
        }
        KeyCode::Home | KeyCode::Char('g') => set_top(&mut state.top, 0),
        KeyCode::End | KeyCode::Char('G') => set_top(&mut state.top, line_count.saturating_sub(1)),
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
    state.top = target_top_for_line(requested, line_count);
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
        state.top = line;
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
        MouseEventKind::ScrollDown => {
            scroll_by(&mut state.top, line_count, MOUSE_SCROLL_LINES as isize)
        }
        MouseEventKind::ScrollUp => {
            scroll_by(&mut state.top, line_count, -(MOUSE_SCROLL_LINES as isize))
        }
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

fn scroll_by(top: &mut usize, line_count: usize, delta: isize) -> bool {
    let old = *top;
    if delta >= 0 {
        *top = top
            .saturating_add(delta as usize)
            .min(line_count.saturating_sub(1));
    } else {
        *top = top.saturating_sub(delta.unsigned_abs());
    }
    *top != old
}

fn set_top(top: &mut usize, value: usize) -> bool {
    let old = *top;
    *top = value;
    *top != old
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
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let visible_height = usize::from(size.height.saturating_sub(3));
    let visible_width = usize::from(size.width.saturating_sub(2));
    let gutter_digits = line_number_digits(file.line_count());
    let gutter_width = gutter_digits + 3;
    let content_width = visible_width.saturating_sub(gutter_width);
    let max_top = file.line_count().saturating_sub(visible_height.max(1));
    state.top = state.top.min(max_top);

    let lines = line_cache.read(file, state.top, visible_height)?;
    let render_context = RenderContext {
        gutter_digits,
        x: state.x,
        width: content_width,
        wrap: state.wrap,
        mode,
    };
    let render_request = RenderRequest {
        context: render_context,
        row_limit: render_row_limit(visible_height),
    };
    let styled = render_visible_lines(
        &lines,
        state.top + 1,
        visible_height,
        render_request,
        render_cache,
        active_search_query(state),
    );

    let current = if file.line_count() == 0 {
        0
    } else {
        state.top + 1
    };
    let bottom = state
        .top
        .saturating_add(visible_height)
        .min(file.line_count());
    let display_mode = if state.wrap {
        "wrap".to_owned()
    } else {
        format!("nowrap x:{}", state.x)
    };
    let title = format!(
        " {} | {} lines | {}-{} | {:>3}% | {} ",
        file.label(),
        file.line_count(),
        current,
        bottom,
        progress_percent(bottom, file.line_count()),
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
        " q/Esc quit | / search n/N | wheel/j/k scroll | 123 Enter jump | Space/f,b page | w wrap "
            .to_owned()
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
        visible_height,
        render_request,
    );

    Ok(())
}

fn active_search_query(state: &ViewState) -> Option<&str> {
    (!state.search_query.is_empty()).then_some(state.search_query.as_str())
}

#[derive(Debug, Default)]
struct LineWindowCache {
    start: usize,
    lines: Vec<String>,
}

impl LineWindowCache {
    fn read(&mut self, file: &IndexedTempFile, top: usize, height: usize) -> Result<Vec<String>> {
        if height == 0 || top >= file.line_count() {
            return Ok(Vec::new());
        }

        let cached_end = self.start.saturating_add(self.lines.len());
        let requested_end = top.saturating_add(height).min(file.line_count());
        if top >= self.start && requested_end <= cached_end {
            let start = top - self.start;
            let end = requested_end - self.start;
            return Ok(self.lines[start..end].to_vec());
        }

        let margin = height.saturating_mul(2).max(32);
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
        Ok(self.lines[start..end].to_vec())
    }
}

#[derive(Debug, Default)]
struct RenderedLineCache {
    request: Option<RenderRequest>,
    lines: HashMap<usize, Vec<Line<'static>>>,
    order: VecDeque<usize>,
}

impl RenderedLineCache {
    fn get_or_render(
        &mut self,
        line: &str,
        line_number: usize,
        request: RenderRequest,
    ) -> &[Line<'static>] {
        if self.request != Some(request) {
            self.request = Some(request);
            self.lines.clear();
            self.order.clear();
        }

        if !self.lines.contains_key(&line_number) {
            self.evict_until_room();
        }

        match self.lines.entry(line_number) {
            Entry::Occupied(entry) => entry.into_mut().as_slice(),
            Entry::Vacant(entry) => {
                let rows =
                    render_logical_line(line, line_number, request.row_limit, request.context);
                self.order.push_back(line_number);
                entry.insert(rows).as_slice()
            }
        }
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

fn render_visible_lines(
    lines: &[String],
    first_line_number: usize,
    height: usize,
    request: RenderRequest,
    cache: &mut RenderedLineCache,
    search_query: Option<&str>,
) -> Vec<Line<'static>> {
    let mut rendered = Vec::with_capacity(height);

    for (index, line) in lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        let rows = cache.get_or_render(line, first_line_number + index, request);
        rendered.extend(
            rows.iter().take(remaining).cloned().map(|row| {
                apply_search_highlight(row, search_query, request.context.gutter_digits)
            }),
        );
    }

    rendered
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
    visible_height: usize,
    request: RenderRequest,
) {
    if visible_height == 0 || file.line_count() == 0 {
        return;
    }

    let side = visible_height.saturating_mul(PREWARM_PAGES);
    let start = top.saturating_sub(side);
    let count = visible_height
        .saturating_add(side.saturating_mul(2))
        .min(PREWARM_MAX_LINES)
        .min(file.line_count().saturating_sub(start));
    let Ok(lines) = line_cache.read(file, start, count) else {
        return;
    };

    let started = Instant::now();
    for (index, line) in lines.iter().enumerate() {
        render_cache.get_or_render(line, start + index + 1, request);
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

fn render_row_limit(visible_height: usize) -> usize {
    visible_height
        .saturating_mul(2)
        .clamp(32, RENDER_CACHE_MAX_ROWS_PER_LINE)
}

fn render_logical_line(
    line: &str,
    line_number: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    if max_rows == 0 {
        return Vec::new();
    }

    if !context.wrap {
        return vec![styled_segment(
            line_number_gutter(line_number, context.gutter_digits),
            line,
            context.x,
            context.x.saturating_add(context.width),
            context.mode,
        )];
    }

    let ranges = wrap_ranges(
        line,
        context.width,
        continuation_indent(line, context.width),
        max_rows,
    );
    let highlight_end = ranges.iter().map(|range| range.end).max().unwrap_or(0);
    let highlight_prefix = slice_chars(line, 0, highlight_end);
    let spans = highlight_content(&highlight_prefix, context.mode);
    ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let gutter = if index == 0 {
                line_number_gutter(line_number, context.gutter_digits)
            } else {
                continuation_gutter(context.gutter_digits)
            };
            let mut line_spans = vec![gutter];
            if range.continuation_indent > 0 {
                line_spans.push(Span::styled(
                    " ".repeat(range.continuation_indent),
                    Style::default(),
                ));
            }
            line_spans.extend(slice_spans(&spans, range.start, range.end));
            Line::from(line_spans)
        })
        .collect()
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

fn continuation_gutter(gutter_digits: usize) -> Span<'static> {
    Span::styled(format!("{:>gutter_digits$} ┆ ", ""), gutter_style())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrapRange {
    start: usize,
    end: usize,
    continuation_indent: usize,
}

fn wrap_ranges(
    line: &str,
    width: usize,
    continuation_indent: usize,
    max_rows: usize,
) -> Vec<WrapRange> {
    if max_rows == 0 {
        return Vec::new();
    }

    let max_chars = width.saturating_mul(max_rows).max(1);
    let chars = line
        .chars()
        .take(max_chars.saturating_add(1))
        .collect::<Vec<_>>();
    let char_count = chars.len().min(max_chars);
    if char_count == 0 || width == 0 {
        return vec![WrapRange {
            start: 0,
            end: 0,
            continuation_indent: 0,
        }];
    }

    let mut ranges = Vec::new();
    let mut start = 0;
    while start < char_count && ranges.len() < max_rows {
        let continuation = !ranges.is_empty();
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let hard_end = start.saturating_add(row_width).min(char_count);
        let end = if hard_end < char_count {
            best_wrap_end(&chars, start, hard_end).unwrap_or(hard_end)
        } else {
            hard_end
        };
        let end = end.max(start + 1);
        ranges.push(WrapRange {
            start,
            end,
            continuation_indent: indent,
        });
        start = end;
    }

    ranges
}

fn best_wrap_end(chars: &[char], start: usize, hard_end: usize) -> Option<usize> {
    let min_end = start + ((hard_end - start) / 2).max(1);

    for end in (min_end..=hard_end).rev() {
        let ch = chars[end - 1];
        if ch.is_whitespace() || matches!(ch, ',' | '>' | '}' | ']' | ';') {
            return Some(end);
        }
    }

    None
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

fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    match mode {
        ViewMode::Plain => highlight_structured(line),
        ViewMode::Diff if line.starts_with("@@") => vec![Span::styled(
            line.to_owned(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )],
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            vec![Span::styled(
                line.to_owned(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]
        }
        ViewMode::Diff if line.starts_with('+') => highlight_diff_payload(line, Color::Green),
        ViewMode::Diff if line.starts_with('-') => highlight_diff_payload(line, Color::Red),
        ViewMode::Diff => highlight_structured(line),
    }
}

fn highlight_diff_payload(line: &str, color: Color) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        line[..1].to_owned(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    spans.extend(highlight_structured(&line[1..]));
    spans
}

fn highlight_structured(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        highlight_xml_line(line)
    } else {
        highlight_json_like(line)
    }
}

fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;

    while index < line.len() {
        let rest = &line[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            let end = take_while(line, index, char::is_whitespace);
            push_span(&mut spans, &line[index..end], Style::default());
            index = end;
            continue;
        }

        if ch == '"' {
            let end = json_string_end(line, index);
            if json_string_is_key(line, end) {
                push_span(&mut spans, &line[index..end], key_style());
            } else {
                spans.extend(highlight_json_string_value(&line[index..end]));
            }
            index = end;
            continue;
        }

        if ch == '-' || ch.is_ascii_digit() {
            let end = take_while(line, index, is_json_number_char);
            push_span(&mut spans, &line[index..end], number_style());
            index = end;
            continue;
        }

        if let Some((word, style)) = json_keyword(rest) {
            push_span(&mut spans, word, style);
            index += word.len();
            continue;
        }

        if "{}[]:,".contains(ch) {
            push_span(
                &mut spans,
                &line[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        push_span(
            &mut spans,
            &line[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

fn highlight_json_string_value(text: &str) -> Vec<Span<'static>> {
    if !text.contains('<') {
        return highlight_string_segment(text);
    }

    let mut spans = Vec::new();
    let inner_start = if text.starts_with('"') { 1 } else { 0 };
    let inner_end = if text.len() > inner_start && text.ends_with('"') {
        text.len() - 1
    } else {
        text.len()
    };

    spans.extend(highlight_string_segment(&text[..inner_start]));
    spans.extend(highlight_inline_xml(&text[inner_start..inner_end], 0));
    spans.extend(highlight_string_segment(&text[inner_end..]));
    spans
}

fn highlight_string_segment(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut plain_start = 0;

    while index < text.len() {
        if let Some(end) = escape_token_end(text, index) {
            push_span(&mut spans, &text[plain_start..index], string_style());
            push_span(&mut spans, &text[index..end], escape_style());
            index = end;
            plain_start = index;
            continue;
        }

        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    push_span(&mut spans, &text[plain_start..], string_style());
    spans
}

fn highlight_xml_line(line: &str) -> Vec<Span<'static>> {
    let base_depth = xml_depth_from_indent(line);
    highlight_inline_xml(line, base_depth)
}

fn highlight_inline_xml(line: &str, base_depth: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut state = XmlPairState::default();

    while index < line.len() {
        let rest = &line[index..];
        if rest.starts_with('<') {
            let end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(line.len());
            let tag = &line[index..end];
            if looks_like_xml_tag(tag) {
                spans.extend(highlight_xml_tag(tag, &mut state, base_depth));
            } else {
                spans.extend(highlight_string_segment(tag));
            }
            index = end;
        } else {
            let end = rest
                .find('<')
                .map(|position| index + position)
                .unwrap_or(line.len());
            spans.extend(highlight_string_segment(&line[index..end]));
            index = end;
        }
    }

    spans
}

fn highlight_xml_tag(tag: &str, state: &mut XmlPairState, base_depth: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let kind = xml_tag_kind(tag);
    let name_range = xml_tag_name_range(tag);
    let name = name_range.map(|(start, end)| &tag[start..end]);
    let tag_state = state.apply(kind, name, base_depth);

    while index < tag.len() {
        let rest = &tag[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if let Some((start, end)) = name_range
            && index == start
        {
            let style = if tag_state.matched {
                xml_depth_style(tag_state.depth)
            } else {
                error_style()
            };
            push_span(&mut spans, &tag[start..end], style);
            index = end;
            continue;
        }

        if ch.is_whitespace() {
            let end = take_while(tag, index, char::is_whitespace);
            push_span(&mut spans, &tag[index..end], Style::default());
            index = end;
            continue;
        }

        if rest.starts_with("\\\"") || rest.starts_with("\\'") {
            let quote = if rest.starts_with("\\\"") { '"' } else { '\'' };
            let end = escaped_quoted_end(tag, index, quote);
            spans.extend(highlight_string_segment(&tag[index..end]));
            index = end;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let end = quoted_end(tag, index, ch);
            spans.extend(highlight_string_segment(&tag[index..end]));
            index = end;
            continue;
        }

        if "<>/=?!".contains(ch) {
            push_span(
                &mut spans,
                &tag[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        if is_xml_name_char(ch) {
            let end = take_while(tag, index, is_xml_name_char);
            push_span(&mut spans, &tag[index..end], attr_style());
            index = end;
            continue;
        }

        push_span(
            &mut spans,
            &tag[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

#[derive(Debug, Default)]
struct XmlPairState {
    stack: Vec<XmlOpenTag>,
}

#[derive(Debug)]
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
                    matched: true,
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

fn json_string_end(line: &str, start: usize) -> usize {
    let mut escaped = false;
    for (offset, ch) in line[start + 1..].char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    line.len()
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

fn push_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(text.to_owned(), style));
    }
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
