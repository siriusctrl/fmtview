use std::time::Duration;

use anyhow::Result;
use ratatui::layout::{Rect, Size};

use crate::{
    diff::{DiffLayout, DiffView},
    tui::palette::gutter_style,
    tui::screen::{RenderFrame, ScrollPosition},
    viewer::{InputEvent, ViewerAction},
};

mod input;
mod navigation;
mod render;

#[cfg(test)]
mod tests;

use input::{clamp_top, handle_event};
use navigation::diff_scroll_hint;
use render::render_frame;

const SIDE_BY_SIDE_MIN_WIDTH: usize = 110;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
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

/// Headless interactive diff state machine and renderer.
pub struct DiffViewer {
    view: DiffView,
    state: DiffViewState,
}

impl DiffViewer {
    pub fn new(mut view: DiffView, size: Size) -> Result<Self> {
        let state = DiffViewState::new(initial_layout(size.width));
        view.preload(LAZY_DIFF_FIRST_OPEN_RECORDS, LAZY_DIFF_FIRST_OPEN_BUDGET)?;
        Ok(Self { view, state })
    }

    pub fn poll_interval(&self) -> Duration {
        EVENT_POLL_INTERVAL
    }

    pub fn preload(&mut self) -> Result<bool> {
        self.view
            .preload(LAZY_DIFF_IDLE_RECORDS, LAZY_DIFF_IDLE_BUDGET)
    }

    pub fn handle_event(&mut self, event: InputEvent, size: Size) -> ViewerAction {
        let visible_height = diff_visible_height(size.height);
        let content_width = usize::from(size.width.saturating_sub(2));
        handle_event(
            event,
            self.view.model(),
            &mut self.state,
            visible_height,
            visible_height,
            content_width,
        )
    }

    pub fn render(&mut self, size: Size, previous_position: Option<ScrollPosition>) -> RenderFrame {
        draw_view(&self.view, &mut self.state, size, previous_position)
    }
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
    view: &DiffView,
    state: &mut DiffViewState,
    size: Size,
    previous_position: Option<ScrollPosition>,
) -> RenderFrame {
    let model = view.model();
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_height = diff_visible_height(size.height);
    let content_width = usize::from(size.width.saturating_sub(2));
    clamp_top(state, model, content_width);

    let message = state.message.take();
    let rendered = render_frame(
        model,
        view.is_complete(),
        state,
        message,
        visible_height,
        content_width,
    );
    let scroll_hint = diff_scroll_hint(previous_position, rendered.position);
    RenderFrame {
        area,
        styled: rendered.rows,
        sticky: Vec::new(),
        selection_mode: false,
        title: rendered.title,
        footer_text: rendered.footer_text,
        footer_style: gutter_style(),
        position: rendered.position,
        scroll_hint,
    }
}
