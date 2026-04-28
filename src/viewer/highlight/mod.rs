mod checkpoints;
mod json;
mod util;
mod xml;

pub(super) use checkpoints::HighlightCheckpointIndex;
#[cfg(test)]
pub(super) use json::highlight_json_like;
#[cfg(test)]
pub(super) use xml::highlight_xml_line;

use ratatui::text::Span;

use super::ViewMode;

use json::highlight_json_like_window;
use xml::highlight_xml_line_window;

pub(in crate::viewer) fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    highlight_content_window(line, mode, 0, line.len())
}

pub(in crate::viewer) fn highlight_content_window(
    line: &str,
    mode: ViewMode,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    highlight_content_window_indexed(line, mode, window_start, window_end, None)
}

pub(in crate::viewer) fn highlight_content_window_indexed(
    line: &str,
    mode: ViewMode,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let window_start = window_start.min(line.len());
    let window_end = window_end.min(line.len()).max(window_start);
    match mode {
        ViewMode::Plain => highlight_structured_window(line, window_start, window_end, index),
    }
}

pub(in crate::viewer) fn highlight_structured_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        highlight_xml_line_window(line, window_start, window_end, index)
    } else {
        highlight_json_like_window(line, window_start, window_end, index)
    }
}
