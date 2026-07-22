mod diff;
mod file;
mod input;

pub use diff::DiffViewer;
pub use file::FileViewer;
pub use input::{InputEvent, KeyCode, KeyModifiers, MouseEventKind, ViewerAction, ViewerCommand};

#[cfg(test)]
use crate::load::IndexedTempFile;
#[cfg(test)]
use crate::tui::palette::*;
#[cfg(test)]
use file::TestViewerCaches as ViewerCaches;
#[cfg(test)]
use file::breadcrumb::JsonBreadcrumbCache;
#[cfg(test)]
use file::markdown_modes::MarkdownModeCache;
#[cfg(test)]
use file::position::{adjust_state_for_visible_height, resolve_targets_from_view};
#[cfg(test)]
use file::position::{
    resolve_search_target_position, resolve_structure_target_position, search_context_rows,
    visual_row_for_byte,
};
#[cfg(test)]
use file::structure::*;
#[cfg(test)]
use file::{
    LAST_ROW_OFFSET, MOUSE_HORIZONTAL_COLUMNS, MOUSE_SCROLL_LINES, NOTICE_DURATION,
    TAIL_ROW_OFFSET, WRAP_RENDER_CHUNK_ROWS,
};

#[cfg(test)]
mod tests;
