use std::time::{Duration, Instant};

use super::search::{SearchMatchIndex, SearchTarget, SearchTask};
use crate::viewer::file::structure::{StructureTask, StructureViewport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum FooterMessageKind {
    Info,
    Warning,
    Error,
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
    pub(in crate::viewer) viewport_at_tail: bool,
    pub(in crate::viewer) preserve_tail_on_next_draw: bool,
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
            viewport_at_tail: false,
            preserve_tail_on_next_draw: false,
            mouse_capture: true,
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct EventAction {
    pub(in crate::viewer) dirty: bool,
    pub(in crate::viewer) quit: bool,
    pub(in crate::viewer) mouse_capture: Option<bool>,
}

impl EventAction {
    pub(in crate::viewer) fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
        self.mouse_capture = next.mouse_capture.or(self.mouse_capture);
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
}
