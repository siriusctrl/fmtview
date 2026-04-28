use std::{io, time::Duration};

use anyhow::{Context, Result};
use crossterm::event;
use ratatui::{backend::CrosstermBackend, layout::Rect};

use crate::diff::{DiffLayout, DiffView};

use super::{
    EVENT_POLL_INTERVAL,
    render::{ViewPosition, format_count, progress_percent},
    terminal::{TerminalFrame, ViewerTerminal},
};

mod input;
mod render;

#[cfg(test)]
mod tests;

use input::{clamp_top, diff_scroll_hint, drain_events};
use render::render_rows_with_status;

const SIDE_BY_SIDE_MIN_WIDTH: usize = 110;
const DIFF_SCROLL_HINT_MAX_ROWS: usize = 12;
const LAZY_DIFF_FIRST_OPEN_RECORDS: usize = 256;
const LAZY_DIFF_IDLE_RECORDS: usize = 256;
const LAZY_DIFF_FIRST_OPEN_BUDGET: Duration = Duration::from_millis(30);
const LAZY_DIFF_IDLE_BUDGET: Duration = Duration::from_millis(8);

#[derive(Debug)]
struct DiffViewState {
    top: usize,
    top_row_offset: usize,
    x: usize,
    wrap: bool,
    layout: DiffLayout,
    message: Option<String>,
    change_cursor: Option<usize>,
}

impl DiffViewState {
    fn new(layout: DiffLayout) -> Self {
        Self {
            top: 0,
            top_row_offset: 0,
            x: 0,
            wrap: true,
            layout,
            message: None,
            change_cursor: None,
        }
    }
}

pub(super) fn run_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    mut view: DiffView,
) -> Result<()> {
    let initial_layout = terminal
        .size()
        .map(|size| initial_layout(size.width))
        .unwrap_or(DiffLayout::Unified);
    let mut state = DiffViewState::new(initial_layout);
    view.preload(LAZY_DIFF_FIRST_OPEN_RECORDS, LAZY_DIFF_FIRST_OPEN_BUDGET)?;
    let mut dirty = true;

    loop {
        if dirty {
            draw_view(terminal, &view, &mut state)?;
            dirty = false;
        }

        if !event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal event")? {
            dirty |= view.preload(LAZY_DIFF_IDLE_RECORDS, LAZY_DIFF_IDLE_BUDGET)?;
            continue;
        }

        let (page, visible_height) = terminal
            .size()
            .map(|size| {
                let visible_height = diff_visible_height(size.height);
                (visible_height, visible_height)
            })
            .unwrap_or((20, 20));
        let content_width = terminal
            .size()
            .map(|size| usize::from(size.width.saturating_sub(2)))
            .unwrap_or(80);
        let action = drain_events(
            view.model(),
            &mut state,
            page,
            visible_height,
            content_width,
        )?;
        if action.quit {
            break;
        }
        dirty |= action.dirty;
        if !dirty {
            dirty |= view.preload(LAZY_DIFF_IDLE_RECORDS, LAZY_DIFF_IDLE_BUDGET)?;
        }
    }

    Ok(())
}

fn initial_layout(width: u16) -> DiffLayout {
    if usize::from(width) >= SIDE_BY_SIDE_MIN_WIDTH {
        DiffLayout::SideBySide
    } else {
        DiffLayout::Unified
    }
}

fn diff_visible_height(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(3)).max(1)
}

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    view: &DiffView,
    state: &mut DiffViewState,
) -> Result<()> {
    let model = view.model();
    let size = terminal.size().context("failed to read terminal size")?;
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_height = diff_visible_height(size.height);
    let content_width = usize::from(size.width.saturating_sub(2));
    clamp_top(state, model, content_width);

    let rendered = render_rows_with_status(
        model,
        state.layout,
        state.top,
        state.top_row_offset,
        visible_height,
        content_width,
        state.x,
        state.wrap,
    );
    let styled = rendered.rows;
    let row_count = model.row_count(state.layout);
    let current = if row_count == 0 { 0 } else { state.top + 1 };
    let bottom = rendered
        .bottom_row
        .saturating_add(1)
        .min(row_count)
        .max(current);
    let lazy_scanning = !view.is_complete();
    let progress = if lazy_scanning {
        0
    } else {
        progress_percent(bottom, row_count)
    };
    let change_text = if lazy_scanning && model.has_changes() {
        format!(
            "{} changes, scanning",
            format_count(model.changed_rows(state.layout).len())
        )
    } else if lazy_scanning {
        "scanning".to_owned()
    } else if model.has_changes() {
        format!(
            "{} changes",
            format_count(model.changed_rows(state.layout).len())
        )
    } else {
        "no changes".to_owned()
    };
    let title = format!(
        " {} <-> {} | {} rows | {} | {}-{} | {:>3}% | diff {} ",
        model.left_label(),
        model.right_label(),
        format_count(row_count),
        change_text,
        current,
        bottom,
        progress,
        state.layout.label()
    );
    let footer_text = state.message.take().unwrap_or_else(|| {
        let wrap_hint = if state.wrap { "w unwrap" } else { "w wrap" };
        let horizontal = if state.wrap { "" } else { " | h/l" };
        format!(
            " q/Esc quit | s single/split | {wrap_hint} | ]/[ next/prev block | j/k wheel | Space/b{horizontal} "
        )
    });
    let position = ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    };
    let scroll_hint = diff_scroll_hint(terminal, position);
    terminal
        .draw(TerminalFrame {
            area,
            styled,
            sticky: Vec::new(),
            title,
            footer_text,
            position,
            scroll_hint,
        })
        .context("failed to draw terminal frame")?;
    Ok(())
}
