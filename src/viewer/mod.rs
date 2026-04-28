mod highlight;
mod input;
mod palette;
mod render;

use std::{
    io::{self, Write},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    event,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::{Buffer, Cell},
    layout::{Constraint, Layout, Rect, Size},
    style::{Color, Modifier},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};

#[cfg(test)]
use crate::line_index::IndexedTempFile;
use crate::line_index::ViewFile;

use input::{ViewState, drain_events, process_search_step, reset_top_row_offset};
use palette::{gutter_style, plain_style};
use render::{
    LineWindowCache, RenderContext, RenderRequest, RenderedLineCache, TailPositionCache,
    ViewPosition, effective_top_row_offset, exact_top_line_tail_offset, format_count,
    is_after_tail, last_full_logical_page_top, line_number_digits, prewarm_render_cache,
    render_row_limit, render_viewport, viewer_progress_percent,
};

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
const LAZY_PRELOAD_LINES: usize = 4096;
const LAZY_PRELOAD_RECORDS: usize = 64;
const LAZY_PRELOAD_BUDGET: Duration = Duration::from_millis(6);
const JUMP_BUFFER_MAX_DIGITS: usize = 20;
const SEARCH_CHUNK_LINES: usize = 4096;
const TAIL_ROW_OFFSET: usize = usize::MAX;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Plain,
    Diff,
}

pub fn run(file: Box<dyn ViewFile>, mode: ViewMode) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut cleanup = TerminalCleanup::active();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ViewerTerminal::new(backend);
    let result = run_loop(&mut terminal, file.as_ref(), mode);

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

struct ViewerTerminal<B> {
    backend: B,
    previous: Option<Buffer>,
    previous_position: Option<ViewPosition>,
}

impl<B> ViewerTerminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    fn new(backend: B) -> Self {
        Self {
            backend,
            previous: None,
            previous_position: None,
        }
    }

    fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    fn draw(
        &mut self,
        area: Rect,
        styled: Vec<Line<'static>>,
        title: String,
        footer_text: String,
        position: ViewPosition,
        scroll_hint: Option<ScrollHint>,
    ) -> io::Result<()> {
        let mut current = Buffer::empty(area);
        render_frame(&mut current, styled, title, footer_text);
        match &self.previous {
            Some(previous) if previous.area == current.area => {
                draw_diff(&mut self.backend, previous, &current, scroll_hint)?;
            }
            _ => {
                self.backend.clear()?;
                let empty = Buffer::empty(area);
                draw_cells(&mut self.backend, empty.diff(&current))?;
                self.previous_position = None;
            }
        }
        self.backend.hide_cursor()?;
        Backend::flush(&mut self.backend)?;
        self.previous = Some(current);
        self.previous_position = Some(position);
        Ok(())
    }

    fn scroll_hint(&self, position: ViewPosition) -> Option<ScrollHint> {
        let previous = self.previous_position?;
        if previous.top != position.top {
            return None;
        }

        let delta = position.row_offset.abs_diff(previous.row_offset);
        if delta == 0 || delta > 12 {
            return None;
        }
        let amount = u16::try_from(delta).ok()?;
        let direction = if position.row_offset > previous.row_offset {
            ScrollDirection::Up
        } else {
            ScrollDirection::Down
        };
        Some(ScrollHint { amount, direction })
    }
}

fn draw_diff<B>(
    backend: &mut B,
    previous: &Buffer,
    current: &Buffer,
    scroll_hint: Option<ScrollHint>,
) -> io::Result<()>
where
    B: Backend<Error = io::Error> + Write,
{
    if let Some(hint) = scroll_hint {
        let scroll = BodyScroll {
            area: scrollable_body_area(current.area),
            amount: hint.amount,
            direction: hint.direction,
        };
        if scroll.area.height > scroll.amount {
            let scrolled_updates = scroll.updates(previous, current);
            scroll.emit(backend)?;
            return draw_cells(backend, scrolled_updates);
        }
    }

    draw_cells(backend, previous.diff(current))
}

fn draw_cells<'a, B, I>(backend: &mut B, content: I) -> io::Result<()>
where
    B: Write,
    I: IntoIterator<Item = (u16, u16, &'a Cell)>,
{
    let mut output = Vec::with_capacity(16 * 1024);
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut modifier = Modifier::empty();
    let mut last_pos: Option<(u16, u16)> = None;

    for (x, y, cell) in content {
        if !matches!(last_pos, Some((last_x, last_y)) if x == last_x.saturating_add(1) && y == last_y)
        {
            write!(
                output,
                "\x1b[{};{}H",
                y.saturating_add(1),
                x.saturating_add(1)
            )?;
        }
        last_pos = Some((x, y));

        if cell.modifier != modifier {
            write!(output, "\x1b[0m")?;
            fg = Color::Reset;
            bg = Color::Reset;
            write_modifier(&mut output, cell.modifier)?;
            modifier = cell.modifier;
        }
        if cell.fg != fg {
            write_fg(&mut output, cell.fg)?;
            fg = cell.fg;
        }
        if cell.bg != bg {
            write_bg(&mut output, cell.bg)?;
            bg = cell.bg;
        }
        output.extend_from_slice(cell.symbol().as_bytes());
    }

    output.extend_from_slice(b"\x1b[0m");
    backend.write_all(&output)
}

fn write_modifier<B>(backend: &mut B, modifier: Modifier) -> io::Result<()>
where
    B: Write,
{
    if modifier.contains(Modifier::BOLD) {
        write!(backend, "\x1b[1m")?;
    }
    if modifier.contains(Modifier::DIM) {
        write!(backend, "\x1b[2m")?;
    }
    if modifier.contains(Modifier::ITALIC) {
        write!(backend, "\x1b[3m")?;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        write!(backend, "\x1b[4m")?;
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        write!(backend, "\x1b[5m")?;
    }
    if modifier.contains(Modifier::RAPID_BLINK) {
        write!(backend, "\x1b[6m")?;
    }
    if modifier.contains(Modifier::REVERSED) {
        write!(backend, "\x1b[7m")?;
    }
    if modifier.contains(Modifier::HIDDEN) {
        write!(backend, "\x1b[8m")?;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        write!(backend, "\x1b[9m")?;
    }
    Ok(())
}

fn write_fg<B>(backend: &mut B, color: Color) -> io::Result<()>
where
    B: Write,
{
    write_color(backend, 38, 39, 30, 90, color)
}

fn write_bg<B>(backend: &mut B, color: Color) -> io::Result<()>
where
    B: Write,
{
    write_color(backend, 48, 49, 40, 100, color)
}

fn write_color<B>(
    backend: &mut B,
    extended_prefix: u8,
    reset: u8,
    base: u8,
    bright_base: u8,
    color: Color,
) -> io::Result<()>
where
    B: Write,
{
    match color {
        Color::Reset => write!(backend, "\x1b[{reset}m"),
        Color::Black => write!(backend, "\x1b[{}m", base),
        Color::Red => write!(backend, "\x1b[{}m", base + 1),
        Color::Green => write!(backend, "\x1b[{}m", base + 2),
        Color::Yellow => write!(backend, "\x1b[{}m", base + 3),
        Color::Blue => write!(backend, "\x1b[{}m", base + 4),
        Color::Magenta => write!(backend, "\x1b[{}m", base + 5),
        Color::Cyan => write!(backend, "\x1b[{}m", base + 6),
        Color::Gray => write!(backend, "\x1b[{}m", base + 7),
        Color::DarkGray => write!(backend, "\x1b[{}m", bright_base),
        Color::LightRed => write!(backend, "\x1b[{}m", bright_base + 1),
        Color::LightGreen => write!(backend, "\x1b[{}m", bright_base + 2),
        Color::LightYellow => write!(backend, "\x1b[{}m", bright_base + 3),
        Color::LightBlue => write!(backend, "\x1b[{}m", bright_base + 4),
        Color::LightMagenta => write!(backend, "\x1b[{}m", bright_base + 5),
        Color::LightCyan => write!(backend, "\x1b[{}m", bright_base + 6),
        Color::White => write!(backend, "\x1b[{}m", bright_base + 7),
        Color::Indexed(index) => write!(backend, "\x1b[{extended_prefix};5;{index}m"),
        Color::Rgb(red, green, blue) => {
            write!(backend, "\x1b[{extended_prefix};2;{red};{green};{blue}m")
        }
    }
}

fn render_frame(
    buffer: &mut Buffer,
    styled: Vec<Line<'static>>,
    title: String,
    footer_text: String,
) {
    let area = buffer.area;
    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let paragraph = Paragraph::new(styled)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(gutter_style()),
        )
        .style(plain_style());
    paragraph.render(body, buffer);
    Paragraph::new(footer_text)
        .style(gutter_style())
        .render(footer, buffer);
}

#[derive(Debug, Clone, Copy)]
enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
struct ScrollHint {
    amount: u16,
    direction: ScrollDirection,
}

#[derive(Debug, Clone, Copy)]
struct BodyScroll {
    area: Rect,
    amount: u16,
    direction: ScrollDirection,
}

impl BodyScroll {
    fn emit<B>(self, backend: &mut B) -> io::Result<()>
    where
        B: Write,
    {
        if self.amount == 0 || self.area.height == 0 {
            return Ok(());
        }

        let top = self.area.y.saturating_add(1);
        let bottom = self.area.y.saturating_add(self.area.height);
        let command = match self.direction {
            ScrollDirection::Up => 'S',
            ScrollDirection::Down => 'T',
        };
        write!(
            backend,
            "\x1b[{top};{bottom}r\x1b[{}{command}\x1b[r",
            self.amount
        )
    }

    fn updates<'a>(self, previous: &Buffer, current: &'a Buffer) -> Vec<(u16, u16, &'a Cell)> {
        let mut updates = Vec::new();
        self.push_static_row_updates(previous, current, &mut updates);
        for y in self.entering_rows() {
            self.push_entering_row(current, y, &mut updates);
        }

        updates
    }

    fn entering_rows(self) -> std::ops::Range<u16> {
        match self.direction {
            ScrollDirection::Up => {
                self.area.y.saturating_add(self.area.height - self.amount)
                    ..self.area.y.saturating_add(self.area.height)
            }
            ScrollDirection::Down => self.area.y..self.area.y.saturating_add(self.amount),
        }
    }

    fn push_entering_row<'a>(
        self,
        current: &'a Buffer,
        y: u16,
        updates: &mut Vec<(u16, u16, &'a Cell)>,
    ) {
        for x in self.area.x..self.area.x.saturating_add(self.area.width) {
            let cell = &current[(x, y)];
            if !cell.skip && !is_visually_empty_cell(cell) {
                updates.push((x, y, cell));
            }
        }
    }

    fn push_static_row_updates<'a>(
        self,
        previous: &Buffer,
        current: &'a Buffer,
        updates: &mut Vec<(u16, u16, &'a Cell)>,
    ) {
        for y in 0..current.area.height {
            if y >= self.area.y && y < self.area.y.saturating_add(self.area.height) {
                continue;
            }
            for x in self.area.x..self.area.x.saturating_add(self.area.width) {
                let cell = &current[(x, y)];
                if !cell.skip && cell != &previous[(x, y)] {
                    updates.push((x, y, cell));
                }
            }
        }
    }
}

fn is_visually_empty_cell(cell: &Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset && cell.modifier == Modifier::empty()
}

fn scrollable_body_area(area: Rect) -> Rect {
    let [body, _footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    Rect {
        x: 0,
        y: body.y.saturating_add(1),
        width: body.width,
        height: body.height.saturating_sub(2),
    }
}

fn run_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
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
            dirty |= file.preload(
                LAZY_PRELOAD_LINES,
                LAZY_PRELOAD_RECORDS,
                LAZY_PRELOAD_BUDGET,
            )?;
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

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: ViewMode,
    state: &mut ViewState,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    tail_cache: &mut TailPositionCache,
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_height = usize::from(size.height.saturating_sub(3));
    let visible_width = usize::from(size.width.saturating_sub(2));
    let gutter_digits = if file.line_count_exact() {
        line_number_digits(file.line_count())
    } else {
        line_number_digits(file.line_count()).max(4)
    };
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

    let position = ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    };
    let scroll_hint = if state.wrap {
        terminal.scroll_hint(position)
    } else {
        None
    };
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
        line_count_text(file),
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
            line_count_text(file)
        )
    } else if let Some(message) = &state.search_message {
        format!(" {message} | / search | n/N | Esc clear ")
    } else {
        idle_footer_text(state)
    };

    terminal
        .draw(area, styled, title, footer_text, position, scroll_hint)
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

fn line_count_text(file: &dyn ViewFile) -> String {
    let count = file.line_count();
    if file.line_count_exact() {
        count.to_string()
    } else {
        format!("{count}+")
    }
}

fn wrap_position_text(state: &ViewState) -> Option<String> {
    if !state.wrap || state.top_row_offset == 0 {
        return None;
    }

    Some(format!("+{} rows", format_count(state.top_row_offset)))
}

#[cfg(test)]
mod tests;
