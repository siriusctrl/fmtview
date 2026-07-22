use anyhow::Result;
use ratatui::{
    layout::{Rect, Size},
    text::Line,
};

use crate::{load::ViewFile, transform::FormatKind};

use super::super::{
    breadcrumb::{JsonBreadcrumbCache, MAX_BREADCRUMB_ROWS},
    input::{FollowState, ViewState},
    position::adjust_state_for_visible_height,
};
use super::{
    GutterLayout, RenderContext, TailPositionCache, ViewPosition, last_full_logical_page_top,
};

#[derive(Debug, Clone, Copy)]
pub(in crate::viewer) struct DrawLayout {
    pub(in crate::viewer) area: Rect,
    pub(in crate::viewer) visible_width: usize,
    pub(in crate::viewer) base_visible_height: usize,
    pub(in crate::viewer) gutter_width: usize,
    pub(in crate::viewer) selection_mode: bool,
    pub(in crate::viewer) context: RenderContext,
}

pub(in crate::viewer) struct StickyLayout {
    pub(in crate::viewer) lines: Vec<Line<'static>>,
    pub(in crate::viewer) visible_height: usize,
    pub(in crate::viewer) tail: Option<ViewPosition>,
}

pub(in crate::viewer) fn draw_layout(
    size: Size,
    file: &dyn ViewFile,
    state: &ViewState,
    mode: FormatKind,
) -> DrawLayout {
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
    let gutter = GutterLayout::for_view(file, selection_mode, mode, visible_width);
    let gutter_width = gutter.width();
    let content_width = visible_width.saturating_sub(gutter_width);

    DrawLayout {
        area,
        visible_width,
        base_visible_height,
        gutter_width,
        selection_mode,
        context: RenderContext {
            gutter,
            x: state.x,
            width: content_width,
            wrap: state.wrap,
            mode,
        },
    }
}

pub(in crate::viewer) fn sync_sticky_layout(
    file: &dyn ViewFile,
    mode: FormatKind,
    state: &mut ViewState,
    breadcrumb: &mut JsonBreadcrumbCache,
    tail_cache: &mut TailPositionCache,
    layout: DrawLayout,
) -> Result<StickyLayout> {
    let mut lines = Vec::new();
    let mut visible_height = layout.base_visible_height;
    let mut tail = None;
    let preserve_tail = state.preserve_tail_on_next_draw;
    let pin_follow_tail = state.follow == Some(FollowState::Following);
    let pin_requested_tail = state.follow == Some(FollowState::Detached)
        && state.follow_reattach_pending
        && file.at_newer_boundary()
        && state.top >= last_full_logical_page_top(file.line_count(), layout.base_visible_height);
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
        if tail.is_none()
            && (pin_follow_tail || pin_requested_tail)
            && file.at_newer_boundary()
            && state.top.saturating_add(MAX_BREADCRUMB_ROWS)
                >= last_full_logical_page_top(file.line_count(), visible_height)
        {
            tail = Some(tail_cache.position(file, visible_height, layout.context)?);
        }
        if preserve_tail || pin_follow_tail || pin_requested_tail {
            pin_state_to_tail(state, tail);
        }
        if preserve_tail {
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

pub(in crate::viewer) fn refresh_sticky_after_position_change(
    file: &dyn ViewFile,
    mode: FormatKind,
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

pub(in crate::viewer) fn visible_height_for_sticky(
    base_visible_height: usize,
    sticky_rows: usize,
) -> usize {
    base_visible_height.saturating_sub(sticky_rows).max(1)
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

fn sticky_lines(
    mode: FormatKind,
    breadcrumb: &mut JsonBreadcrumbCache,
    file: &dyn ViewFile,
    top: usize,
    width: usize,
    gutter_width: usize,
    base_visible_height: usize,
) -> Vec<Line<'static>> {
    if matches!(mode, FormatKind::Json | FormatKind::Jsonl) {
        breadcrumb.render(file, top, width, gutter_width, base_visible_height)
    } else {
        Vec::new()
    }
}
