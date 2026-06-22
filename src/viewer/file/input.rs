mod events;
mod jump;
mod keys;
mod scroll;
mod search;
mod state;

pub(in crate::viewer) use events::drain_events;
pub(in crate::viewer) use scroll::reset_top_row_offset;
pub(in crate::viewer) use search::{SearchTarget, process_search_index_step, process_search_step};
pub(in crate::viewer) use state::{FooterMessageKind, ViewState};

#[cfg(test)]
pub(in crate::viewer) use events::{handle_event, handle_key_event, handle_key_event_with_count};
#[cfg(test)]
pub(in crate::viewer) use scroll::scroll_down_by;
#[cfg(test)]
pub(in crate::viewer) use search::{SearchDirection, start_search};
