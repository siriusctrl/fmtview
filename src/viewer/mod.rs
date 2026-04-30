mod breadcrumb;
mod diff_view;
mod input;
pub(crate) mod palette;
mod render;
mod terminal;

use std::{
    io::{self, Write},
    time::Duration,
};

use anyhow::{Context, Result};
use breadcrumb::JsonBreadcrumbCache;
use crossterm::{
    event,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, text::Line};

#[cfg(test)]
use crate::load::IndexedTempFile;
use crate::load::ViewFile;
use crate::syntax::SyntaxKind;

use input::{ViewState, drain_events, process_search_step, reset_top_row_offset};
use render::{
    LineWindowCache, RenderContext, RenderRequest, RenderedLineCache, TailPositionCache,
    ViewPosition, continuation_indent, effective_top_row_offset, exact_top_line_tail_offset,
    format_count, is_after_tail, last_full_logical_page_top, line_number_digits, next_wrap_end,
    prewarm_render_cache, render_row_limit, render_viewport, rendered_row_count,
    viewer_progress_percent,
};
#[cfg(test)]
use terminal::draw_cells;
use terminal::{TerminalFrame, ViewerTerminal};

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
const TERMINAL_SCROLL_HINT_MAX_ROWS: usize = 12;
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
pub fn run(file: Box<dyn ViewFile>, mode: SyntaxKind) -> Result<()> {
    run_terminal(|terminal| run_loop(terminal, file.as_ref(), mode))
}

pub(crate) fn run_diff(view: crate::diff::DiffView) -> Result<()> {
    run_terminal(|terminal| diff_view::run_loop(terminal, view))
}

fn run_terminal<F>(run_loop: F) -> Result<()>
where
    F: FnOnce(&mut ViewerTerminal<CrosstermBackend<io::Stdout>>) -> Result<()>,
{
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut cleanup = TerminalCleanup::active();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ViewerTerminal::new(backend);
    let result = run_loop(&mut terminal);

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
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: SyntaxKind,
) -> Result<()> {
    let mut state = ViewState::default();
    let mut dirty = true;
    let mut caches = ViewerCaches::default();

    loop {
        if state.search_task.is_some() {
            dirty |= process_search_step(file, &mut state)?;
        }

        if dirty {
            draw_view(terminal, file, mode, &mut state, &mut caches)?;
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
        let action = drain_events(&mut state, file.line_count(), file.line_count_exact(), page)?;
        if action.quit {
            break;
        }
        dirty |= action.dirty;
    }

    Ok(())
}

#[derive(Default)]
struct ViewerCaches {
    line: LineWindowCache,
    render: RenderedLineCache,
    tail: TailPositionCache,
    breadcrumb: JsonBreadcrumbCache,
}

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: SyntaxKind,
    state: &mut ViewState,
    caches: &mut ViewerCaches,
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_width = usize::from(size.width.saturating_sub(2));
    let base_visible_height = usize::from(size.height.saturating_sub(3));
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

    let mut sticky = Vec::new();
    let mut visible_height = base_visible_height;
    let mut tail = None;
    for _ in 0..3 {
        tail =
            adjust_state_for_visible_height(file, state, visible_height, render_context, caches)?;
        let next_sticky = sticky_lines(
            mode,
            &mut caches.breadcrumb,
            file,
            state.top,
            visible_width,
            gutter_width,
            base_visible_height,
        );
        let next_visible_height = visible_height_for_sticky(base_visible_height, next_sticky.len());
        let stable = next_visible_height == visible_height;
        sticky = next_sticky;
        visible_height = next_visible_height;
        if stable {
            break;
        }
    }

    let mut lines = caches.line.read(
        file,
        state.top,
        visible_height,
        visible_height.saturating_mul(2).max(32),
    )?;
    for _ in 0..3 {
        if !resolve_search_target_position(state, lines.lines, visible_height, render_context) {
            break;
        }
        lines = caches.line.read(
            file,
            state.top,
            visible_height,
            visible_height.saturating_mul(2).max(32),
        )?;
    }
    let final_sticky = sticky_lines(
        mode,
        &mut caches.breadcrumb,
        file,
        state.top,
        visible_width,
        gutter_width,
        base_visible_height,
    );
    if final_sticky.len() != sticky.len() {
        sticky = final_sticky;
        visible_height = visible_height_for_sticky(base_visible_height, sticky.len());
        tail =
            adjust_state_for_visible_height(file, state, visible_height, render_context, caches)?;
        lines = caches.line.read(
            file,
            state.top,
            visible_height,
            visible_height.saturating_mul(2).max(32),
        )?;
        for _ in 0..3 {
            if !resolve_search_target_position(state, lines.lines, visible_height, render_context) {
                break;
            }
            lines = caches.line.read(
                file,
                state.top,
                visible_height,
                visible_height.saturating_mul(2).max(32),
            )?;
        }
        sticky = sticky_lines(
            mode,
            &mut caches.breadcrumb,
            file,
            state.top,
            visible_width,
            gutter_width,
            base_visible_height,
        );
    } else {
        sticky = final_sticky;
    }
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
        &mut caches.render,
        active_search_query(state),
    );
    let mut max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        visible_height,
        render_context,
        &caches.render,
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
            &mut caches.render,
            active_search_query(state),
        );
    }
    max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        visible_height,
        render_context,
        &caches.render,
        tail,
    );
    if state.top_row_offset > max_top_row_offset
        && caches.render.status(state.top + 1).total_rows.is_some()
    {
        state.top_row_offset = max_top_row_offset;
        viewport = render_viewport(
            lines.lines,
            state.top + 1,
            state.top_row_offset,
            visible_height,
            render_request,
            &mut caches.render,
            active_search_query(state),
        );
        max_top_row_offset = effective_top_row_offset(
            state.top + 1,
            visible_height,
            render_context,
            &caches.render,
            tail,
        );
    }
    state.top_max_row_offset = max_top_row_offset;

    let position = ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    };
    let scroll_hint = if state.wrap {
        terminal
            .scroll_hint(position)
            .or_else(|| logical_scroll_hint(terminal, &caches.render, position))
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
        .draw(TerminalFrame {
            area,
            styled,
            sticky,
            title,
            footer_text,
            position,
            scroll_hint,
        })
        .context("failed to draw terminal frame")?;

    prewarm_render_cache(
        file,
        &mut caches.line,
        &mut caches.render,
        state.top,
        state.top_row_offset,
        visible_height,
        render_request,
    );

    Ok(())
}

fn adjust_state_for_visible_height(
    file: &dyn ViewFile,
    state: &mut ViewState,
    visible_height: usize,
    render_context: RenderContext,
    caches: &mut ViewerCaches,
) -> Result<Option<ViewPosition>> {
    let logical_tail_top = last_full_logical_page_top(file.line_count(), visible_height);
    let tail = if file.line_count_exact() && (!state.wrap || state.top >= logical_tail_top) {
        Some(caches.tail.position(file, visible_height, render_context)?)
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

fn sticky_lines(
    mode: SyntaxKind,
    breadcrumb: &mut JsonBreadcrumbCache,
    file: &dyn ViewFile,
    top: usize,
    width: usize,
    gutter_width: usize,
    base_visible_height: usize,
) -> Vec<Line<'static>> {
    if mode == SyntaxKind::Structured {
        breadcrumb.render(file, top, width, gutter_width, base_visible_height)
    } else {
        Vec::new()
    }
}

fn visible_height_for_sticky(base_visible_height: usize, sticky_rows: usize) -> usize {
    base_visible_height.saturating_sub(sticky_rows).max(1)
}

fn active_search_query(state: &ViewState) -> Option<&str> {
    (!state.search_query.is_empty()).then_some(state.search_query.as_str())
}

fn resolve_search_target_position(
    state: &mut ViewState,
    lines: &[String],
    visible_height: usize,
    context: RenderContext,
) -> bool {
    let Some(target) = state.search_target else {
        return false;
    };

    let context_rows = search_context_rows(visible_height);
    match target_visual_position_in_window(lines, state.top, target, context) {
        Some(position)
            if visual_row_is_visible(position.row, state.top_row_offset, visible_height) =>
        {
            state.search_target = None;
            false
        }
        Some(position) if target.line == state.top => {
            state.top_row_offset = position.row_in_line.saturating_sub(context_rows);
            state.top_max_row_offset = 0;
            state.search_target = None;
            false
        }
        Some(position) if position.row_in_line > context_rows => {
            position_search_target_visual_line(
                state,
                target.line,
                position.row_in_line,
                context_rows,
            )
        }
        Some(position) => {
            if position_search_target_logical_line(state, target.line, visible_height) {
                true
            } else {
                position_search_target_visual_line(
                    state,
                    target.line,
                    position.row_in_line,
                    context_rows,
                )
            }
        }
        None => position_search_target_logical_line(state, target.line, visible_height),
    }
}

fn position_search_target_visual_line(
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
    state.search_target = None;
    state.top != old_top
}

fn position_search_target_logical_line(
    state: &mut ViewState,
    target_line: usize,
    visible_height: usize,
) -> bool {
    let next_top = target_line.saturating_sub(search_context_rows(visible_height));
    if state.top == next_top && state.top_row_offset == 0 {
        state.search_target = None;
        return false;
    }

    state.top = next_top;
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    true
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

fn search_context_rows(visible_height: usize) -> usize {
    if visible_height < 4 {
        return 0;
    }

    (visible_height / 3)
        .clamp(2, 8)
        .min(visible_height.saturating_sub(1))
}

fn visual_row_for_byte(line: &str, byte_index: usize, context: RenderContext) -> usize {
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

fn logical_scroll_hint(
    terminal: &ViewerTerminal<CrosstermBackend<io::Stdout>>,
    render_cache: &RenderedLineCache,
    position: ViewPosition,
) -> Option<terminal::ScrollHint> {
    let previous = terminal.previous_position()?;
    if previous.row_offset != 0 || position.row_offset != 0 {
        return None;
    }

    if position.top == previous.top.saturating_add(1) {
        return known_line_rows(render_cache, previous.top).map(terminal::ScrollHint::up);
    }
    if previous.top == position.top.saturating_add(1) {
        return known_line_rows(render_cache, position.top).map(terminal::ScrollHint::down);
    }

    None
}

fn known_line_rows(render_cache: &RenderedLineCache, zero_based_line: usize) -> Option<u16> {
    let rows = render_cache.status(zero_based_line + 1).total_rows?;
    if rows == 0 || rows > TERMINAL_SCROLL_HINT_MAX_ROWS {
        return None;
    }
    u16::try_from(rows).ok()
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
