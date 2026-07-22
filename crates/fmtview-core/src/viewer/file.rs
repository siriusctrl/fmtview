use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::layout::Size;

use crate::load::{ViewFile, ViewFileChange};
use crate::transform::FormatKind;

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
pub(in crate::viewer) const TIMELINE_PRELOAD_BYTES: usize = 4 * 1024 * 1024;
pub(in crate::viewer) const TIMELINE_OLDER_THRESHOLD_LINES: usize = 64;
pub(in crate::viewer) const JUMP_BUFFER_MAX_DIGITS: usize = 20;
pub(in crate::viewer) const SEARCH_CHUNK_LINES: usize = 4096;
pub(in crate::viewer) const TAIL_ROW_OFFSET: usize = usize::MAX;
pub(in crate::viewer) const LAST_ROW_OFFSET: usize = usize::MAX - 1;
pub(in crate::viewer) const NOTICE_DURATION: Duration = Duration::from_secs(10);

pub(in crate::viewer) mod breadcrumb;
mod cache;
pub(in crate::viewer) mod chat_roles;
pub(in crate::viewer) mod input;
pub(in crate::viewer) mod markdown_modes;
pub(in crate::viewer) mod position;
pub(in crate::viewer) mod render;
pub(in crate::viewer) mod structure;

use cache::ViewerCaches;

#[cfg(test)]
pub(super) use cache::ViewerCaches as TestViewerCaches;

use crate::tui::screen::{RenderFrame, ScrollHint, ScrollPosition};
use crate::viewer::{InputEvent, KeyCode, ViewerAction, ViewerCommand};
use input::{
    FollowState, SearchDirection, ViewState, handle_event_with_count, process_search_index_step,
    process_search_step, set_file_end,
};
use position::resolve_targets_from_view;
use render::{
    RenderRequest, RenderedLineCache, ViewPosition, ViewportRenderOptions, draw_layout,
    effective_top_row_offset, exact_top_line_scroll_limit, exact_top_line_tail_offset,
    file_footer_style, file_footer_text, file_title_text, prewarm_render_cache,
    refresh_sticky_after_position_change, render_row_limit, render_viewport, sync_sticky_layout,
    viewer_progress_percent,
};
use structure::{StructureDirection, StructureViewport, process_structure_step};

/// Headless file-viewer state machine and renderer.
pub struct FileViewer {
    file: Box<dyn ViewFile>,
    mode: FormatKind,
    state: ViewState,
    caches: ViewerCaches,
    pending_prewarm: Option<(ViewPosition, usize, RenderRequest)>,
}

impl FileViewer {
    pub fn new(file: Box<dyn ViewFile>, mode: FormatKind, notice: Option<String>) -> Self {
        let mut state = ViewState::default();
        if file.is_follow_source() {
            state.follow = Some(FollowState::Following);
            state.viewport_at_tail = true;
            set_file_end(&mut state, file.line_count());
        }
        if let Some(message) = notice {
            state.set_notice(message, Instant::now(), NOTICE_DURATION);
        }
        Self {
            file,
            mode,
            state,
            caches: ViewerCaches::default(),
            pending_prewarm: None,
        }
    }

    pub fn advance(&mut self, now: Instant) -> Result<bool> {
        let mut dirty = false;
        if self.file.has_older_records()
            && self
                .state
                .structure_task
                .as_ref()
                .is_some_and(|task| task.direction == StructureDirection::Backward)
        {
            let change = self
                .file
                .load_older_records(LAZY_PRELOAD_RECORDS, TIMELINE_PRELOAD_BYTES)?;
            dirty |= self.apply_file_change(change);
        }
        if self.file.has_older_records()
            && self
                .state
                .search_task
                .as_ref()
                .is_some_and(|task| task.direction == SearchDirection::Backward)
        {
            let change = self
                .file
                .load_older_records(LAZY_PRELOAD_RECORDS, TIMELINE_PRELOAD_BYTES)?;
            dirty |= self.apply_file_change(change);
        }

        let file = self.file.as_ref();
        dirty |= absorb_file_notice(file, &mut self.state, now);
        dirty |= self.state.expire_footer_message(now);
        if self.state.search_task.is_some() {
            dirty |= process_search_step(file, &mut self.state)?;
        }
        if self.state.structure_task.is_some() {
            dirty |= process_structure_step(file, &mut self.state, self.mode)?;
        }
        if self
            .state
            .search_index
            .as_ref()
            .is_some_and(|index| !index.exact)
            && !self
                .state
                .search_task
                .as_ref()
                .is_some_and(|task| task.awaiting_older)
        {
            dirty |= process_search_index_step(file, &mut self.state)?;
        }
        if self.file.has_older_records() && self.state.search_task.is_some() {
            let change = self
                .file
                .load_older_records(LAZY_PRELOAD_RECORDS, TIMELINE_PRELOAD_BYTES)?;
            dirty |= self.apply_file_change(change);
        }
        Ok(dirty)
    }

    /// Whether the engine has a search or navigation task ready to advance.
    pub fn needs_immediate_advance(&self) -> bool {
        self.state.search_task.is_some() || self.state.structure_task.is_some()
    }

    pub fn preload(&mut self) -> Result<bool> {
        if !self.file.is_follow_source() {
            return self.file.preload(
                LAZY_PRELOAD_LINES,
                LAZY_PRELOAD_RECORDS,
                LAZY_PRELOAD_BUDGET,
            );
        }

        let refresh = self
            .file
            .refresh_records(LAZY_PRELOAD_RECORDS, TIMELINE_PRELOAD_BYTES)?;
        let mut dirty = self.apply_file_change(refresh);
        if self.state.top <= TIMELINE_OLDER_THRESHOLD_LINES && self.file.has_older_records() {
            let older = self
                .file
                .load_older_records(LAZY_PRELOAD_RECORDS, TIMELINE_PRELOAD_BYTES)?;
            dirty |= self.apply_file_change(older);
        }
        Ok(dirty)
    }

    pub fn page_for_size(size: Size) -> usize {
        usize::from(size.height.saturating_sub(4)).max(1)
    }

    pub fn handle_event(&mut self, event: InputEvent, page: usize) -> ViewerAction {
        let had_active_prompt = self.state.has_active_prompt();
        if self.file.is_follow_source() {
            if matches!(event, InputEvent::Command(ViewerCommand::FollowTail)) {
                return self.enable_follow_tail();
            }
            if matches!(event, InputEvent::Command(ViewerCommand::ToggleFollowTail))
                || (!self.state.has_active_prompt()
                    && matches!(
                        event,
                        InputEvent::Key {
                            code: KeyCode::Char('f'),
                            modifiers
                        } if modifiers.is_empty()
                    ))
            {
                return self.toggle_follow_tail();
            }
        }

        let mut action = handle_event_with_count(
            event,
            &mut self.state,
            self.file.line_count(),
            self.file.at_newer_boundary(),
            page,
        );
        if self.file.is_follow_source() && !had_active_prompt {
            if action.dirty && event_moves_away_from_tail(event) {
                if self.state.follow == Some(FollowState::Following) {
                    self.state.follow = Some(FollowState::Detached);
                }
                self.state.viewport_at_tail = false;
                self.state.follow_reattach_pending = false;
            } else if event_forces_tail(event)
                && (action.dirty
                    || matches!(
                        event,
                        InputEvent::Key {
                            code: KeyCode::End | KeyCode::Char('G'),
                            ..
                        }
                    ))
            {
                self.state.follow = Some(FollowState::Following);
                self.state.viewport_at_tail = true;
                self.state.follow_reattach_pending = false;
            } else if event_moves_toward_tail(event)
                && self.state.follow == Some(FollowState::Detached)
            {
                if !action.dirty || self.state.top >= self.file.line_count().saturating_sub(1) {
                    self.state.follow = Some(FollowState::Following);
                    self.state.viewport_at_tail = true;
                    self.state.follow_reattach_pending = false;
                    set_file_end(&mut self.state, self.file.line_count());
                    action.dirty = true;
                } else {
                    self.state.follow_reattach_pending = true;
                }
            }
        }
        action
    }

    pub fn needs_layout(&self) -> bool {
        self.state.wrap_bounds_stale
    }

    pub fn render(
        &mut self,
        size: Size,
        previous_position: Option<ScrollPosition>,
    ) -> Result<RenderFrame> {
        let (frame, pending_prewarm) = draw_view(
            self.file.as_ref(),
            self.mode,
            &mut self.state,
            &mut self.caches,
            size,
            previous_position,
        )?;
        self.pending_prewarm = Some(pending_prewarm);
        Ok(frame)
    }

    pub fn prewarm(&mut self) {
        let Some((position, visible_height, request)) = self.pending_prewarm.take() else {
            return;
        };
        prewarm_render_cache(
            self.file.as_ref(),
            &mut self.caches.line,
            &mut self.caches.render,
            &mut self.caches.markdown,
            position,
            visible_height,
            request,
        );
    }

    fn apply_file_change(&mut self, change: ViewFileChange) -> bool {
        if change.inserted_lines > 0 {
            self.state
                .shift_for_insert(change.inserted_at, change.inserted_lines);
        }
        if change.appended_lines > 0 && self.state.follow == Some(FollowState::Following) {
            set_file_end(&mut self.state, self.file.line_count());
        }
        if change.changed() {
            self.caches = ViewerCaches::default();
            self.pending_prewarm = None;
        }
        change.changed()
    }

    fn enable_follow_tail(&mut self) -> ViewerAction {
        self.state.follow = Some(FollowState::Following);
        self.state.viewport_at_tail = true;
        self.state.follow_reattach_pending = false;
        set_file_end(&mut self.state, self.file.line_count());
        ViewerAction {
            dirty: true,
            ..ViewerAction::default()
        }
    }

    fn toggle_follow_tail(&mut self) -> ViewerAction {
        if self.state.follow == Some(FollowState::Paused) {
            self.enable_follow_tail()
        } else {
            self.state.preserve_tail_on_next_draw = self.state.viewport_at_tail;
            self.state.follow = Some(FollowState::Paused);
            self.state.follow_reattach_pending = false;
            ViewerAction {
                dirty: true,
                ..ViewerAction::default()
            }
        }
    }
}

fn event_moves_away_from_tail(event: InputEvent) -> bool {
    matches!(
        event,
        InputEvent::Key {
            code: KeyCode::Up | KeyCode::PageUp | KeyCode::Home | KeyCode::Char('k' | 'b' | 'g'),
            ..
        } | InputEvent::Mouse {
            kind: crate::viewer::MouseEventKind::ScrollUp,
            ..
        }
    )
}

fn event_forces_tail(event: InputEvent) -> bool {
    matches!(
        event,
        InputEvent::Key {
            code: KeyCode::End | KeyCode::Char('G'),
            ..
        }
    )
}

fn event_moves_toward_tail(event: InputEvent) -> bool {
    matches!(
        event,
        InputEvent::Key {
            code: KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j' | ' '),
            ..
        } | InputEvent::Mouse {
            kind: crate::viewer::MouseEventKind::ScrollDown,
            ..
        }
    )
}

fn draw_view(
    file: &dyn ViewFile,
    mode: FormatKind,
    state: &mut ViewState,
    caches: &mut ViewerCaches,
    size: Size,
    previous_position: Option<ScrollPosition>,
) -> Result<(RenderFrame, (ViewPosition, usize, RenderRequest))> {
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
    if state.top_row_offset == LAST_ROW_OFFSET {
        state.top_row_offset = exact_top_line_scroll_limit(lines.lines, layout.context);
    } else if state.top_row_offset == TAIL_ROW_OFFSET {
        state.top_row_offset =
            exact_top_line_tail_offset(lines.lines, sticky.visible_height, layout.context);
    }
    state.wrap_bounds_stale = false;
    let line_modes = caches
        .markdown
        .line_modes(file, state.top, lines.lines, mode)?;
    let conversation_marks = if matches!(mode, FormatKind::Json | FormatKind::Jsonl) {
        Some(caches.chat_roles.marks_for_view(
            file,
            state.top,
            lines.lines,
            layout.context.gutter.chat_role_enabled(),
        )?)
    } else {
        None
    };

    let mut viewport = render_viewport(
        lines.lines,
        state.top + 1,
        state.top_row_offset,
        sticky.visible_height,
        render_request,
        &mut caches.render,
        ViewportRenderOptions {
            line_modes: line_modes.as_deref(),
            chat_role_marks: conversation_marks
                .as_ref()
                .map(|marks| marks.roles.as_slice()),
            tool_relation_marks: conversation_marks
                .as_ref()
                .map(|marks| marks.tools.as_slice()),
            search_query: active_search_query(state),
            active_search_match: state.search_match_target,
        },
    );
    let mut max_top_row_offset =
        effective_top_row_offset(state.top + 1, layout.context, &caches.render, sticky.tail);
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
                chat_role_marks: conversation_marks
                    .as_ref()
                    .map(|marks| marks.roles.as_slice()),
                tool_relation_marks: conversation_marks
                    .as_ref()
                    .map(|marks| marks.tools.as_slice()),
                search_query: active_search_query(state),
                active_search_match: state.search_match_target,
            },
        );
    }
    max_top_row_offset =
        effective_top_row_offset(state.top + 1, layout.context, &caches.render, sticky.tail);
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
                chat_role_marks: conversation_marks
                    .as_ref()
                    .map(|marks| marks.roles.as_slice()),
                tool_relation_marks: conversation_marks
                    .as_ref()
                    .map(|marks| marks.tools.as_slice()),
                search_query: active_search_query(state),
                active_search_match: state.search_match_target,
            },
        );
        max_top_row_offset =
            effective_top_row_offset(state.top + 1, layout.context, &caches.render, sticky.tail);
    }
    state.top_max_row_offset = max_top_row_offset;

    let position = ViewPosition {
        top: state.top,
        row_offset: state.top_row_offset,
    };
    let scroll_hint = if state.wrap && state.mouse_capture {
        position_scroll_hint(previous_position, position)
            .or_else(|| logical_scroll_hint(previous_position, &caches.render, position))
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
    let tool_context = conversation_marks
        .as_ref()
        .and_then(|marks| visible_tool_context(marks, state, bottom));
    state.set_tool_context(tool_context);
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
    let position_is_tail = sticky
        .tail
        .is_some_and(|tail| state.top == tail.top && state.top_row_offset == tail.row_offset);
    state.viewport_at_tail = file.at_newer_boundary()
        && file.line_count() > 0
        && (position_is_tail
            || (bottom == file.line_count()
                && viewport
                    .bottom
                    .as_ref()
                    .is_none_or(|bottom| bottom.line_end)));
    if state.follow == Some(FollowState::Following)
        && file.at_newer_boundary()
        && file.line_count() > 0
        && !state.viewport_at_tail
    {
        state.follow = Some(FollowState::Detached);
    }
    if state.follow == Some(FollowState::Detached) && state.follow_reattach_pending {
        if state.viewport_at_tail {
            state.follow = Some(FollowState::Following);
        }
        state.follow_reattach_pending = false;
    }
    let progress = viewer_progress_percent(file, layout.context, bottom, viewport.bottom);
    let styled = viewport.lines;
    absorb_file_notice(file, state, Instant::now());
    let title = file_title_text(file, state, current, bottom, progress);
    let footer_text = file_footer_text(file, state);
    let footer_style = file_footer_style(state);

    let frame = RenderFrame {
        area: layout.area,
        styled,
        sticky: sticky.lines,
        selection_mode: layout.selection_mode,
        title,
        footer_text,
        footer_style,
        position,
        scroll_hint,
    };

    Ok((frame, (position, sticky.visible_height, render_request)))
}

fn absorb_file_notice(file: &dyn ViewFile, state: &mut ViewState, now: Instant) -> bool {
    if let Some(message) = file.take_notice() {
        state.set_notice(message, now, NOTICE_DURATION);
        true
    } else {
        false
    }
}

fn active_search_query(state: &ViewState) -> Option<&str> {
    (!state.search_query.is_empty()).then_some(state.search_query.as_str())
}

fn visible_tool_context(
    marks: &chat_roles::ConversationViewMarks,
    state: &ViewState,
    bottom_line_number: usize,
) -> Option<(crate::formats::json::tool_links::ToolLink, usize)> {
    let visible_len = bottom_line_number.saturating_sub(state.top);
    let preferred_line = if state.tool_selection.is_some() {
        state.tool_context_line
    } else {
        state
            .search_match_target
            .map(|target| target.line)
            .or(state.tool_context_line)
    };

    preferred_line
        .filter(|line| *line >= state.top && *line < bottom_line_number)
        .and_then(|line| {
            marks
                .tools
                .get(line.checked_sub(state.top)?)
                .and_then(|mark| mark.link.clone().map(|link| (link, line)))
        })
        .or_else(|| {
            marks
                .tools
                .iter()
                .take(visible_len)
                .enumerate()
                .find_map(|(offset, mark)| {
                    mark.link
                        .clone()
                        .map(|link| (link, state.top.saturating_add(offset)))
                })
        })
}

fn logical_scroll_hint(
    previous: Option<ScrollPosition>,
    render_cache: &RenderedLineCache,
    position: ViewPosition,
) -> Option<ScrollHint> {
    let previous = previous?;
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

fn position_scroll_hint(
    previous: Option<ScrollPosition>,
    position: ScrollPosition,
) -> Option<ScrollHint> {
    let previous = previous?;
    if previous.top != position.top {
        return None;
    }

    let delta = position.row_offset.abs_diff(previous.row_offset);
    if delta == 0 || delta > TERMINAL_SCROLL_HINT_MAX_ROWS {
        return None;
    }
    let amount = u16::try_from(delta).ok()?;
    if position.row_offset > previous.row_offset {
        Some(ScrollHint::up(amount))
    } else {
        Some(ScrollHint::down(amount))
    }
}

fn known_line_rows(render_cache: &RenderedLineCache, zero_based_line: usize) -> Option<u16> {
    let rows = render_cache.status(zero_based_line + 1).total_rows?;
    if rows == 0 || rows > TERMINAL_SCROLL_HINT_MAX_ROWS {
        return None;
    }
    u16::try_from(rows).ok()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::formats::json::tool_links::{
        ToolLineMark, ToolLink, ToolLinkStatus, ToolRelationMark,
    };

    #[test]
    fn tool_context_ignores_prefetched_lines_below_the_rendered_viewport() {
        let link = ToolLink {
            id: Arc::from("call_7"),
            call_line: Some(2),
            result_line: 15,
            status: ToolLinkStatus::Matched,
        };
        let mut marks = chat_roles::ConversationViewMarks {
            roles: Vec::new(),
            tools: vec![ToolLineMark::default(); 6],
        };
        marks.tools[5] = ToolLineMark {
            relation: ToolRelationMark::MatchedResult,
            link: Some(link.clone()),
        };
        let state = ViewState {
            top: 10,
            ..ViewState::default()
        };

        assert_eq!(visible_tool_context(&marks, &state, 13), None);
        assert_eq!(visible_tool_context(&marks, &state, 16), Some((link, 15)));
    }
}
