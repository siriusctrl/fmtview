mod events;
mod jump;
mod keys;
mod scroll;
mod search;
mod state;

pub(in crate::viewer) use events::handle_event_with_count;
pub(in crate::viewer) use scroll::{reset_top_row_offset, set_file_end};
pub(in crate::viewer) use search::{
    SearchDirection, SearchTarget, process_search_index_step, process_search_step,
};
pub(in crate::viewer) use state::{FollowState, FooterMessageKind, ViewState};

#[cfg(test)]
pub(in crate::viewer) use events::{handle_event, handle_key_event, handle_key_event_with_count};
#[cfg(test)]
pub(in crate::viewer) use scroll::{scroll_down, scroll_down_by, scroll_up};
#[cfg(test)]
pub(in crate::viewer) use search::start_search;
