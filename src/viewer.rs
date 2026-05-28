mod breadcrumb;
mod diff_view;
mod input;
pub(crate) mod palette;
mod position;
mod render;
mod syntax_state;
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
use ratatui::{
    backend::CrosstermBackend,
    layout::{Rect, Size},
    text::Line,
};

#[cfg(test)]
use crate::load::IndexedTempFile;
use crate::load::ViewFile;
use crate::syntax::SyntaxKind;

use input::{
    StructureViewport, ViewState, drain_events, process_search_index_step, process_search_step,
    process_structure_step,
};
use position::{adjust_state_for_visible_height, resolve_targets_from_view};
#[cfg(test)]
use position::{
    resolve_search_target_position, resolve_structure_target_position, search_context_rows,
    visual_row_for_byte,
};
use render::{
    LineWindowCache, RenderContext, RenderRequest, RenderedLineCache, TailPositionCache,
    ViewPosition, ViewportRenderOptions, effective_top_row_offset, exact_top_line_tail_offset,
    format_count, line_number_digits, prewarm_render_cache, render_row_limit, render_viewport,
    viewer_progress_percent,
};
use syntax_state::MarkdownSyntaxCache;
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
        if state.structure_task.is_some() {
            dirty |= process_structure_step(file, &mut state, mode)?;
        }
        if state
            .search_index
            .as_ref()
            .is_some_and(|index| !index.exact)
        {
            dirty |= process_search_index_step(file, &mut state)?;
        }

        if dirty {
            draw_view(terminal, file, mode, &mut state, &mut caches)?;
            dirty = false;
        }

        let poll_interval = if state.search_task.is_some() || state.structure_task.is_some() {
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
        if let Some(enabled) = action.mouse_capture {
            apply_mouse_capture(terminal, enabled)?;
        }
        dirty |= action.dirty;
    }

    Ok(())
}

fn apply_mouse_capture(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    enabled: bool,
) -> Result<()> {
    if enabled {
        execute!(terminal.backend_mut(), EnableMouseCapture)
            .context("failed to enable mouse capture")?;
    } else {
        execute!(terminal.backend_mut(), DisableMouseCapture)
            .context("failed to disable mouse capture")?;
    }
    Ok(())
}

#[derive(Default)]
struct ViewerCaches {
    line: LineWindowCache,
    render: RenderedLineCache,
    markdown: MarkdownSyntaxCache,
    tail: TailPositionCache,
    breadcrumb: JsonBreadcrumbCache,
}

#[derive(Debug, Clone, Copy)]
struct DrawLayout {
    area: Rect,
    visible_width: usize,
    base_visible_height: usize,
    gutter_width: usize,
    selection_mode: bool,
    context: RenderContext,
}

struct StickyLayout {
    lines: Vec<Line<'static>>,
    visible_height: usize,
    tail: Option<ViewPosition>,
}

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: SyntaxKind,
    state: &mut ViewState,
    caches: &mut ViewerCaches,
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let layout = draw_layout(size, file, state, mode);
    let mut sticky = sync_sticky_layout(
        file,
        mode,
        state,
        &mut caches.breadcrumb,
        &mut caches.tail,
        layout,
    )?;

    sticky.tail = resolve_targets_from_view(
        file,
        state,
        &mut caches.line,
        sticky.visible_height,
        layout.context,
        &mut caches.tail,
    )?;
    let mut lines = caches.line.read(
        file,
        state.top,
        sticky.visible_height,
        sticky.visible_height.saturating_mul(2).max(32),
    )?;
    if refresh_sticky_after_position_change(
        file,
        mode,
        state,
        &mut caches.breadcrumb,
        &mut caches.tail,
        layout,
        &mut sticky,
    )? {
        sticky.tail = resolve_targets_from_view(
            file,
            state,
            &mut caches.line,
            sticky.visible_height,
            layout.context,
            &mut caches.tail,
        )?;
        lines = caches.line.read(
            file,
            state.top,
            sticky.visible_height,
            sticky.visible_height.saturating_mul(2).max(32),
        )?;
    }

    let render_request = RenderRequest {
        context: layout.context,
        row_limit: render_row_limit(sticky.visible_height),
    };
    if state.top_row_offset == TAIL_ROW_OFFSET {
        state.top_row_offset =
            exact_top_line_tail_offset(lines.lines, sticky.visible_height, layout.context);
    }
    state.wrap_bounds_stale = false;
    let line_modes = caches
        .markdown
        .line_modes(file, state.top, lines.lines, mode)?;

    let mut viewport = render_viewport(
        lines.lines,
        state.top + 1,
        state.top_row_offset,
        sticky.visible_height,
        render_request,
        &mut caches.render,
        ViewportRenderOptions {
            line_modes: line_modes.as_deref(),
            search_query: active_search_query(state),
        },
    );
    let mut max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        sticky.visible_height,
        layout.context,
        &caches.render,
        sticky.tail,
    );
    if viewport.lines.is_empty() && state.top_row_offset > 0 {
        state.top_row_offset = max_top_row_offset;
        viewport = render_viewport(
            lines.lines,
            state.top + 1,
            state.top_row_offset,
            sticky.visible_height,
            render_request,
            &mut caches.render,
            ViewportRenderOptions {
                line_modes: line_modes.as_deref(),
                search_query: active_search_query(state),
            },
        );
    }
    max_top_row_offset = effective_top_row_offset(
        state.top + 1,
        sticky.visible_height,
        layout.context,
        &caches.render,
        sticky.tail,
    );
    if state.top_row_offset > max_top_row_offset
        && caches.render.status(state.top + 1).total_rows.is_some()
    {
        state.top_row_offset = max_top_row_offset;
        viewport = render_viewport(
            lines.lines,
            state.top + 1,
            state.top_row_offset,
            sticky.visible_height,
            render_request,
            &mut caches.render,
            ViewportRenderOptions {
                line_modes: line_modes.as_deref(),
                search_query: active_search_query(state),
            },
        );
        max_top_row_offset = effective_top_row_offset(
            state.top + 1,
            sticky.visible_height,
            layout.context,
            &caches.render,
            sticky.tail,
        );
    }
    state.top_max_row_offset = max_top_row_offset;

    let position = ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    };
    let scroll_hint = if state.wrap && state.mouse_capture {
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
    state.structure_viewport = Some(StructureViewport {
        top: state.top,
        top_row_offset: state.top_row_offset,
        bottom: bottom.saturating_sub(1),
        bottom_line_end: viewport
            .bottom
            .as_ref()
            .is_none_or(|bottom| bottom.line_end),
        x: state.x,
        width: layout.context.width,
        wrap: state.wrap,
    });
    state.viewport_at_tail = file.line_count_exact()
        && file.line_count() > 0
        && bottom == file.line_count()
        && viewport
            .bottom
            .as_ref()
            .is_none_or(|bottom| bottom.line_end);
    let progress = viewer_progress_percent(file, layout.context, bottom, viewport.bottom);
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
        format!(
            " {message}{} | / search | n/N | Esc clear ",
            search_count_suffix(state)
        )
    } else {
        idle_footer_text(state)
    };

    terminal
        .draw(TerminalFrame {
            area: layout.area,
            styled,
            sticky: sticky.lines,
            selection_mode: layout.selection_mode,
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
        &mut caches.markdown,
        position,
        sticky.visible_height,
        render_request,
    );

    Ok(())
}

fn draw_layout(size: Size, file: &dyn ViewFile, state: &ViewState, mode: SyntaxKind) -> DrawLayout {
    let selection_mode = !state.mouse_capture;
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_width = if selection_mode {
        usize::from(size.width)
    } else {
        usize::from(size.width.saturating_sub(2))
    };
    let base_visible_height = if selection_mode {
        usize::from(size.height.saturating_sub(1))
    } else {
        usize::from(size.height.saturating_sub(3))
    };
    let gutter_digits = if selection_mode {
        0
    } else if file.line_count_exact() {
        line_number_digits(file.line_count())
    } else {
        line_number_digits(file.line_count()).max(4)
    };
    let gutter_width = if gutter_digits == 0 {
        0
    } else {
        gutter_digits + 3
    };
    let content_width = visible_width.saturating_sub(gutter_width);

    DrawLayout {
        area,
        visible_width,
        base_visible_height,
        gutter_width,
        selection_mode,
        context: RenderContext {
            gutter_digits,
            x: state.x,
            width: content_width,
            wrap: state.wrap,
            mode,
        },
    }
}

fn sync_sticky_layout(
    file: &dyn ViewFile,
    mode: SyntaxKind,
    state: &mut ViewState,
    breadcrumb: &mut JsonBreadcrumbCache,
    tail_cache: &mut TailPositionCache,
    layout: DrawLayout,
) -> Result<StickyLayout> {
    let mut lines = Vec::new();
    let mut visible_height = layout.base_visible_height;
    let mut tail = None;
    let preserve_tail = state.preserve_tail_on_next_draw;
    let preserved_tail_position = preserve_tail.then_some(ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    });
    state.preserve_tail_on_next_draw = false;

    for _ in 0..3 {
        tail = adjust_state_for_visible_height(
            file,
            state,
            visible_height,
            layout.context,
            tail_cache,
        )?;
        if preserve_tail {
            pin_state_to_tail(state, tail);
            keep_preserved_tail_position(state, preserved_tail_position);
        }
        let next_lines = sticky_lines(
            mode,
            breadcrumb,
            file,
            state.top,
            layout.visible_width,
            layout.gutter_width,
            layout.base_visible_height,
        );
        let next_visible_height =
            visible_height_for_sticky(layout.base_visible_height, next_lines.len());
        let stable = next_visible_height == visible_height;
        lines = next_lines;
        visible_height = next_visible_height;
        if stable {
            break;
        }
    }

    Ok(StickyLayout {
        lines,
        visible_height,
        tail,
    })
}

fn pin_state_to_tail(state: &mut ViewState, tail: Option<ViewPosition>) {
    let Some(tail) = tail else {
        return;
    };
    if state.top == tail.top && state.top_row_offset == tail.row_offset {
        return;
    }

    state.top = tail.top;
    state.top_row_offset = tail.row_offset;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
}

fn keep_preserved_tail_position(state: &mut ViewState, position: Option<ViewPosition>) {
    let Some(position) = position else {
        return;
    };
    // Sticky breadcrumbs can change the computed tail while rendering a status
    // message; keep an already-tail viewport from moving upward.
    if state.top > position.top
        || (state.top == position.top && state.top_row_offset >= position.row_offset)
    {
        return;
    }

    state.top = position.top;
    state.top_row_offset = position.row_offset;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
}

fn refresh_sticky_after_position_change(
    file: &dyn ViewFile,
    mode: SyntaxKind,
    state: &mut ViewState,
    breadcrumb: &mut JsonBreadcrumbCache,
    tail_cache: &mut TailPositionCache,
    layout: DrawLayout,
    sticky: &mut StickyLayout,
) -> Result<bool> {
    let final_lines = sticky_lines(
        mode,
        breadcrumb,
        file,
        state.top,
        layout.visible_width,
        layout.gutter_width,
        layout.base_visible_height,
    );
    if final_lines.len() == sticky.lines.len() {
        sticky.lines = final_lines;
        return Ok(false);
    }

    sticky.lines = final_lines;
    sticky.visible_height =
        visible_height_for_sticky(layout.base_visible_height, sticky.lines.len());
    sticky.tail = adjust_state_for_visible_height(
        file,
        state,
        sticky.visible_height,
        layout.context,
        tail_cache,
    )?;
    sticky.lines = sticky_lines(
        mode,
        breadcrumb,
        file,
        state.top,
        layout.visible_width,
        layout.gutter_width,
        layout.base_visible_height,
    );
    Ok(true)
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
    let mouse_hint = if state.mouse_capture {
        "m select"
    } else {
        "m mouse"
    };
    let position = wrap_position_text(state)
        .map(|position| format!("{position} | "))
        .unwrap_or_default();
    let search = search_count_text(state)
        .map(|count| format!("{count} | "))
        .unwrap_or_default();
    format!(
        " {position}{search}{wrap_hint} | {mouse_hint} | / search n/N | ]/[ structure | 123 Enter jump to line | Space/f,b "
    )
}

fn search_count_suffix(state: &ViewState) -> String {
    search_count_text(state)
        .map(|count| format!(" | {count}"))
        .unwrap_or_default()
}

fn search_count_text(state: &ViewState) -> Option<String> {
    let index = state.search_index.as_ref()?;
    if index.query != state.search_query {
        return None;
    }

    let suffix = if index.exact { "" } else { "+" };
    let matches = state
        .search_match_ordinal
        .map(|ordinal| index.matches.max(ordinal))
        .unwrap_or(index.matches);
    let noun = if matches == 1 { "match" } else { "matches" };
    if let Some(ordinal) = state.search_match_ordinal {
        return Some(format!(
            "{}/{}{suffix} {noun}",
            format_count(ordinal),
            format_count(matches)
        ));
    }

    Some(format!("{}{suffix} {noun}", format_count(matches)))
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
