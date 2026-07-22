use std::time::{Duration, Instant};

use super::search::{SearchMatchIndex, SearchTarget, SearchTask};
use crate::formats::json::tool_links::{ToolLink, ToolLinkStatus};
use crate::viewer::file::structure::{StructureTask, StructureViewport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum FooterMessageKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum FollowState {
    Following,
    Detached,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct FooterMessage {
    pub(in crate::viewer) text: String,
    pub(in crate::viewer) kind: FooterMessageKind,
    pub(in crate::viewer) expires_at: Option<Instant>,
}

pub(in crate::viewer) struct ViewState {
    pub(in crate::viewer) top: usize,
    pub(in crate::viewer) top_row_offset: usize,
    pub(in crate::viewer) top_max_row_offset: usize,
    pub(in crate::viewer) wrap_bounds_stale: bool,
    pub(in crate::viewer) x: usize,
    pub(in crate::viewer) wrap: bool,
    pub(in crate::viewer) jump_buffer: String,
    pub(in crate::viewer) search_active: bool,
    pub(in crate::viewer) search_buffer: String,
    pub(in crate::viewer) search_query: String,
    pub(in crate::viewer) footer_message: Option<FooterMessage>,
    pub(in crate::viewer) search_task: Option<SearchTask>,
    pub(in crate::viewer) search_index: Option<SearchMatchIndex>,
    pub(in crate::viewer) search_match_ordinal: Option<usize>,
    pub(in crate::viewer) search_match_target: Option<SearchTarget>,
    pub(in crate::viewer) search_target: Option<SearchTarget>,
    pub(in crate::viewer) search_cursor: Option<usize>,
    pub(in crate::viewer) structure_task: Option<StructureTask>,
    pub(in crate::viewer) structure_target: Option<SearchTarget>,
    pub(in crate::viewer) structure_cursor: Option<usize>,
    pub(in crate::viewer) structure_viewport: Option<StructureViewport>,
    pub(in crate::viewer) tool_context: Option<ToolLink>,
    pub(in crate::viewer) tool_context_line: Option<usize>,
    pub(in crate::viewer) tool_selection: Option<ToolLink>,
    pub(in crate::viewer) tool_target: Option<usize>,
    pub(in crate::viewer) viewport_at_tail: bool,
    pub(in crate::viewer) preserve_tail_on_next_draw: bool,
    pub(in crate::viewer) follow: Option<FollowState>,
    pub(in crate::viewer) follow_reattach_pending: bool,
    pub(in crate::viewer) mouse_capture: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            top: 0,
            top_row_offset: 0,
            top_max_row_offset: 0,
            wrap_bounds_stale: false,
            x: 0,
            wrap: true,
            jump_buffer: String::new(),
            search_active: false,
            search_buffer: String::new(),
            search_query: String::new(),
            footer_message: None,
            search_task: None,
            search_index: None,
            search_match_ordinal: None,
            search_match_target: None,
            search_target: None,
            search_cursor: None,
            structure_task: None,
            structure_target: None,
            structure_cursor: None,
            structure_viewport: None,
            tool_context: None,
            tool_context_line: None,
            tool_selection: None,
            tool_target: None,
            viewport_at_tail: false,
            preserve_tail_on_next_draw: false,
            follow: None,
            follow_reattach_pending: false,
            mouse_capture: true,
        }
    }
}

impl ViewState {
    pub(in crate::viewer) fn has_active_prompt(&self) -> bool {
        self.search_active || !self.jump_buffer.is_empty()
    }

    pub(in crate::viewer) fn has_search_session(&self) -> bool {
        self.search_active
            || !self.search_query.is_empty()
            || self.search_task.is_some()
            || self.search_index.is_some()
            || self.search_target.is_some()
            || self.search_match_target.is_some()
            || self.search_match_ordinal.is_some()
            || self.search_cursor.is_some()
    }

    pub(in crate::viewer) fn set_notice(
        &mut self,
        message: String,
        now: Instant,
        duration: Duration,
    ) {
        self.set_timed_footer_message(message, FooterMessageKind::Error, now + duration);
    }

    pub(in crate::viewer) fn set_footer_message(
        &mut self,
        text: impl Into<String>,
        kind: FooterMessageKind,
    ) {
        self.footer_message = Some(FooterMessage {
            text: text.into(),
            kind,
            expires_at: None,
        });
    }

    pub(in crate::viewer) fn set_timed_footer_message(
        &mut self,
        text: impl Into<String>,
        kind: FooterMessageKind,
        expires_at: Instant,
    ) {
        self.footer_message = Some(FooterMessage {
            text: text.into(),
            kind,
            expires_at: Some(expires_at),
        });
    }

    pub(in crate::viewer) fn clear_footer_message(&mut self) -> bool {
        let was_active = self.footer_message.is_some();
        self.footer_message = None;
        was_active
    }

    pub(in crate::viewer) fn expire_footer_message(&mut self, now: Instant) -> bool {
        if self
            .footer_message
            .as_ref()
            .and_then(|message| message.expires_at)
            .is_some_and(|expires_at| now >= expires_at)
        {
            return self.clear_footer_message();
        }
        false
    }

    pub(in crate::viewer) fn visible_footer_message(&self) -> Option<&FooterMessage> {
        (!self.search_active && self.jump_buffer.is_empty())
            .then_some(self.footer_message.as_ref())
            .flatten()
    }

    pub(in crate::viewer) fn set_tool_context(&mut self, link: Option<(ToolLink, usize)>) {
        let (tool_context, tool_context_line) = link
            .map(|(link, line)| (Some(link), Some(line)))
            .unwrap_or((None, None));
        self.tool_context = tool_context;
        self.tool_context_line = tool_context_line;
    }

    pub(in crate::viewer) fn toggle_tool_pair(&mut self) -> bool {
        let Some(link) = self
            .tool_context
            .clone()
            .or_else(|| self.tool_selection.clone())
        else {
            self.set_footer_message(
                "no tool call/result at the current viewport",
                FooterMessageKind::Warning,
            );
            return true;
        };
        let Some(call_line) = link.call_line else {
            let message = match link.status {
                ToolLinkStatus::Ambiguous => "tool result id matches multiple earlier calls",
                ToolLinkStatus::Unmatched => "no earlier tool call matches this result id",
                ToolLinkStatus::Matched => "tool pair is incomplete",
            };
            self.set_footer_message(message, FooterMessageKind::Warning);
            return true;
        };

        let context_line = self.tool_context_line.unwrap_or(self.top);
        let at_call = context_line == call_line || self.top == call_line;
        let target = if at_call { link.result_line } else { call_line };
        self.tool_target = Some(target);
        self.tool_context_line = Some(target);
        self.tool_selection = Some(link);
        self.clear_footer_message();
        true
    }

    pub(in crate::viewer) fn clear_tool_navigation(&mut self) {
        self.tool_context = None;
        self.tool_context_line = None;
        self.tool_selection = None;
        self.tool_target = None;
    }

    pub(in crate::viewer) fn shift_for_insert(&mut self, at: usize, lines: usize) {
        if lines == 0 {
            return;
        }
        shift_index(&mut self.top, at, lines);
        shift_optional_index(&mut self.search_cursor, at, lines);
        shift_optional_index(&mut self.structure_cursor, at, lines);
        shift_optional_index(&mut self.tool_context_line, at, lines);
        shift_optional_index(&mut self.tool_target, at, lines);
        shift_target(&mut self.search_match_target, at, lines);
        shift_target(&mut self.search_target, at, lines);
        shift_target(&mut self.structure_target, at, lines);
        if let Some(task) = self.search_task.as_mut() {
            if !task.awaiting_older {
                shift_index(&mut task.next_line, at, lines);
                task.remaining = task.remaining.saturating_add(lines);
            }
        }
        if let Some(index) = self.search_index.as_mut() {
            index.counted_lines = 0;
            index.matches = 0;
            index.line_match_totals.clear();
            index.exact = false;
        }
        if let Some(task) = self.structure_task.as_mut() {
            if task.next_line != usize::MAX {
                shift_index(&mut task.next_line, at, lines);
            }
            if let Some(viewport) = task.viewport.as_mut() {
                shift_index(&mut viewport.top, at, lines);
                shift_index(&mut viewport.bottom, at, lines);
            }
        }
        if let Some(viewport) = self.structure_viewport.as_mut() {
            shift_index(&mut viewport.top, at, lines);
            shift_index(&mut viewport.bottom, at, lines);
        }
        self.search_match_ordinal = None;
        self.clear_tool_navigation();
    }

    pub(in crate::viewer) fn shift_for_overlap_removal(&mut self, at: usize, lines: usize) {
        if lines == 0 {
            return;
        }
        shift_index_for_removal(&mut self.top, at, lines);
        shift_optional_index_for_removal(&mut self.search_cursor, at, lines);
        shift_optional_index_for_removal(&mut self.structure_cursor, at, lines);
        shift_optional_index_for_removal(&mut self.tool_context_line, at, lines);
        shift_optional_index_for_removal(&mut self.tool_target, at, lines);
        shift_target_for_removal(&mut self.search_match_target, at, lines);
        shift_target_for_removal(&mut self.search_target, at, lines);
        shift_target_for_removal(&mut self.structure_target, at, lines);
        if let Some(task) = self.search_task.as_mut() {
            shift_index_for_removal(&mut task.next_line, at, lines);
            task.remaining = task.remaining.saturating_sub(lines);
        }
        if let Some(index) = self.search_index.as_mut() {
            index.counted_lines = 0;
            index.matches = 0;
            index.line_match_totals.clear();
            index.exact = false;
        }
        if let Some(task) = self.structure_task.as_mut() {
            if task.next_line != usize::MAX {
                shift_index_for_removal(&mut task.next_line, at, lines);
            }
            if let Some(viewport) = task.viewport.as_mut() {
                shift_index_for_removal(&mut viewport.top, at, lines);
                shift_index_for_removal(&mut viewport.bottom, at, lines);
            }
        }
        if let Some(viewport) = self.structure_viewport.as_mut() {
            shift_index_for_removal(&mut viewport.top, at, lines);
            shift_index_for_removal(&mut viewport.bottom, at, lines);
        }
        self.search_match_ordinal = None;
        self.clear_tool_navigation();
    }

    pub(in crate::viewer) fn extend_for_append(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        if let Some(task) = self.search_task.as_mut() {
            task.remaining = task.remaining.saturating_add(lines);
        }
        if let Some(index) = self.search_index.as_mut() {
            index.exact = false;
        }
    }
}

fn shift_index(value: &mut usize, at: usize, lines: usize) {
    if *value >= at {
        *value = value.saturating_add(lines);
    }
}

fn shift_optional_index(value: &mut Option<usize>, at: usize, lines: usize) {
    if let Some(value) = value.as_mut() {
        shift_index(value, at, lines);
    }
}

fn shift_target(target: &mut Option<super::search::SearchTarget>, at: usize, lines: usize) {
    if let Some(target) = target.as_mut() {
        shift_index(&mut target.line, at, lines);
    }
}

fn shift_index_for_removal(value: &mut usize, at: usize, lines: usize) {
    if *value >= at {
        *value = value.saturating_sub(lines);
    }
}

fn shift_optional_index_for_removal(value: &mut Option<usize>, at: usize, lines: usize) {
    if let Some(value) = value.as_mut() {
        shift_index_for_removal(value, at, lines);
    }
}

fn shift_target_for_removal(
    target: &mut Option<super::search::SearchTarget>,
    at: usize,
    lines: usize,
) {
    if let Some(target) = target.as_mut() {
        shift_index_for_removal(&mut target.line, at, lines);
    }
}
