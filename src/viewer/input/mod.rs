mod events;
mod jump;
mod keys;
mod scroll;
mod search;
mod state;
mod structure;

pub(super) use events::drain_events;
pub(super) use scroll::reset_top_row_offset;
pub(super) use search::{SearchTarget, process_search_index_step, process_search_step};
pub(super) use state::ViewState;
pub(super) use structure::{StructureViewport, process_structure_step};

#[cfg(test)]
pub(super) use events::{handle_event, handle_key_event, handle_key_event_with_count};
#[cfg(test)]
pub(super) use scroll::scroll_down_by;
#[cfg(test)]
pub(super) use search::{SearchDirection, start_search};
#[cfg(test)]
pub(super) use structure::{StructureDirection, is_structure_point, start_structure_navigation};
