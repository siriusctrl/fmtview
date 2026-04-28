use super::search::{SearchTarget, SearchTask};

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
    pub(in crate::viewer) search_message: Option<String>,
    pub(in crate::viewer) search_task: Option<SearchTask>,
    pub(in crate::viewer) search_target: Option<SearchTarget>,
    pub(in crate::viewer) search_cursor: Option<usize>,
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
            search_message: None,
            search_task: None,
            search_target: None,
            search_cursor: None,
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct EventAction {
    pub(in crate::viewer) dirty: bool,
    pub(in crate::viewer) quit: bool,
}

impl EventAction {
    pub(in crate::viewer) fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
    }
}

impl ViewState {
    pub(in crate::viewer) fn has_active_prompt(&self) -> bool {
        self.search_active || !self.jump_buffer.is_empty()
    }
}
