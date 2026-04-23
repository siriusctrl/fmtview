use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    io,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Plain,
    Diff,
}

pub fn run(file: IndexedTempFile, mode: ViewMode) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
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
    terminal.show_cursor().ok();

    result
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

        if !event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal event")? {
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

#[derive(Debug, Clone, Copy)]
struct ViewState {
    top: usize,
    x: usize,
    wrap: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            top: 0,
            x: 0,
            wrap: true,
        }
    }
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
        Event::Mouse(mouse) => handle_mouse_event(mouse.kind, mouse.modifiers, state, line_count),
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
    let dirty = match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            return EventAction {
                dirty: false,
                quit: true,
            };
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
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
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            scroll_by(&mut state.top, line_count, (page / 2).max(1) as isize)
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            scroll_by(&mut state.top, line_count, -((page / 2).max(1) as isize))
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

    let lines = line_cache
        .read(file, state.top, visible_height)
        .unwrap_or_else(|error| vec![format!("failed to read window: {error:#}")]);
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
    let footer_text = " q/Esc quit | wheel/j/k/↑/↓ scroll | Space/f page | b page up | Ctrl-d/u half | g/G top/end | w wrap ";

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

        let inserted = if let Entry::Vacant(entry) = self.lines.entry(line_number) {
            let rows = render_logical_line(line, line_number, request.row_limit, request.context);
            entry.insert(rows);
            true
        } else {
            false
        };
        if inserted {
            self.order.push_back(line_number);
            self.evict_oldest();
        }

        self.lines
            .get(&line_number)
            .expect("rendered line should exist")
    }

    fn evict_oldest(&mut self) {
        while self.lines.len() > RENDER_CACHE_MAX_LINES {
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
) -> Vec<Line<'static>> {
    let mut rendered = Vec::with_capacity(height);

    for (index, line) in lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        let rows = cache.get_or_render(line, first_line_number + index, request);
        rendered.extend(rows.iter().take(remaining).cloned());
    }

    rendered
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
        let ch = rest.chars().next().expect("index should point to a char");

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

        let ch = text[index..]
            .chars()
            .next()
            .expect("index should point to a char");
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
        let ch = rest.chars().next().expect("index should point to a char");

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
            let quote = rest.chars().nth(1).expect("escaped quote should exist");
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
                        depth: base_depth + self.stack.len().saturating_sub(1),
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

#[cfg(test)]
mod tests {
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
}
