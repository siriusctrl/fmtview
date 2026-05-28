use std::{
    io,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use ratatui::backend::CrosstermBackend;

use crate::load::ViewFile;
use crate::transform::FormatKind;

pub(in crate::viewer) const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
pub(in crate::viewer) const EVENT_DRAIN_BUDGET: Duration = Duration::from_millis(8);
pub(in crate::viewer) const EVENT_DRAIN_LIMIT: usize = 512;
pub(in crate::viewer) const MOUSE_SCROLL_LINES: usize = 1;
pub(in crate::viewer) const MOUSE_HORIZONTAL_COLUMNS: usize = 4;
pub(in crate::viewer) const RENDER_CACHE_MAX_LINES: usize = 512;
pub(in crate::viewer) const RENDER_CACHE_MAX_ROWS_PER_LINE: usize = 256;
pub(in crate::viewer) const WRAP_RENDER_CHUNK_ROWS: usize = 64;
pub(in crate::viewer) const WRAP_RENDER_CHUNKS_PER_LINE: usize = 64;
pub(in crate::viewer) const TERMINAL_SCROLL_HINT_MAX_ROWS: usize = 12;
pub(in crate::viewer) const WRAP_PREWARM_LOGICAL_LINES: usize = 4;
pub(in crate::viewer) const PREWARM_PAGES: usize = 2;
pub(in crate::viewer) const PREWARM_MAX_LINES: usize = 192;
pub(in crate::viewer) const PREWARM_MAX_LINE_BYTES: usize = 16 * 1024;
pub(in crate::viewer) const PREWARM_BUDGET: Duration = Duration::from_millis(4);
pub(in crate::viewer) const LAZY_PRELOAD_LINES: usize = 4096;
pub(in crate::viewer) const LAZY_PRELOAD_RECORDS: usize = 64;
pub(in crate::viewer) const LAZY_PRELOAD_BUDGET: Duration = Duration::from_millis(6);
pub(in crate::viewer) const JUMP_BUFFER_MAX_DIGITS: usize = 20;
pub(in crate::viewer) const SEARCH_CHUNK_LINES: usize = 4096;
pub(in crate::viewer) const TAIL_ROW_OFFSET: usize = usize::MAX;
pub(in crate::viewer) const NOTICE_DURATION: Duration = Duration::from_secs(10);

pub(in crate::viewer) mod breadcrumb;
mod cache;
pub(in crate::viewer) mod input;
pub(in crate::viewer) mod markdown_modes;
pub(in crate::viewer) mod position;
pub(in crate::viewer) mod render;
pub(in crate::viewer) mod structure;

use cache::ViewerCaches;

#[cfg(test)]
pub(super) use cache::ViewerCaches as TestViewerCaches;

use crate::tui::screen::{ScrollHint, TerminalFrame, ViewerTerminal};
use input::{ViewState, drain_events, process_search_index_step, process_search_step};
use position::resolve_targets_from_view;
use render::{
    RenderRequest, RenderedLineCache, ViewPosition, ViewportRenderOptions, draw_layout,
    effective_top_row_offset, exact_top_line_tail_offset, file_footer_style, file_footer_text,
    file_title_text, prewarm_render_cache, refresh_sticky_after_position_change, render_row_limit,
    render_viewport, sync_sticky_layout, viewer_progress_percent,
};
use structure::{StructureViewport, process_structure_step};

pub(super) fn run_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: FormatKind,
    notice: Option<String>,
) -> Result<()> {
    let mut state = ViewState::default();
    if let Some(message) = notice {
        state.set_notice(message, Instant::now(), NOTICE_DURATION);
    }
    let mut dirty = true;
    let mut caches = ViewerCaches::default();

    loop {
        dirty |= state.expire_notice(Instant::now());
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

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    file: &dyn ViewFile,
    mode: FormatKind,
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
    let title = file_title_text(file, state, current, bottom, progress);
    let footer_text = file_footer_text(file, state);
    let footer_style = file_footer_style(state);

    terminal
        .draw(TerminalFrame {
            area: layout.area,
            styled,
            sticky: sticky.lines,
            selection_mode: layout.selection_mode,
            title,
            footer_text,
            footer_style,
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

fn active_search_query(state: &ViewState) -> Option<&str> {
    (!state.search_query.is_empty()).then_some(state.search_query.as_str())
}

fn logical_scroll_hint(
    terminal: &ViewerTerminal<CrosstermBackend<io::Stdout>>,
    render_cache: &RenderedLineCache,
    position: ViewPosition,
) -> Option<ScrollHint> {
    let previous = terminal.previous_position()?;
    if previous.row_offset != 0 || position.row_offset != 0 {
        return None;
    }

    if position.top == previous.top.saturating_add(1) {
        return known_line_rows(render_cache, previous.top).map(ScrollHint::up);
    }
    if previous.top == position.top.saturating_add(1) {
        return known_line_rows(render_cache, position.top).map(ScrollHint::down);
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
