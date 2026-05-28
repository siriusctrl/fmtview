mod cache;
mod footer;
mod layout;
mod line;
mod metrics;
mod prewarm;
mod search;
mod tail;
mod types;
mod viewport;

pub(in crate::viewer) use crate::tui::text::*;
pub(in crate::viewer) use crate::tui::wrap::{continuation_indent, next_wrap_end};
pub(in crate::viewer) use cache::*;
pub(in crate::viewer) use footer::*;
pub(in crate::viewer) use layout::*;
pub(in crate::viewer) use line::rendered_row_count;
pub(in crate::viewer) use metrics::*;
pub(in crate::viewer) use prewarm::*;
pub(in crate::viewer) use tail::*;
pub(in crate::viewer) use types::*;
pub(in crate::viewer) use viewport::*;

#[cfg(test)]
pub(in crate::viewer) use crate::tui::wrap::*;
#[cfg(test)]
pub(in crate::viewer) use line::*;
#[cfg(test)]
pub(in crate::viewer) use search::*;
