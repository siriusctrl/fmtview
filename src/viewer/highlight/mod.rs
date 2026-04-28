mod checkpoints;
mod json;
mod util;
mod xml;

pub(super) use checkpoints::HighlightCheckpointIndex;
#[cfg(test)]
pub(super) use json::highlight_json_like;
#[cfg(test)]
pub(super) use xml::highlight_xml_line;

use ratatui::{style::Style, text::Span};

use super::{
    ViewMode,
    palette::{diff_added_style, diff_file_style, diff_hunk_style, diff_removed_style},
};

use json::highlight_json_like_window;
use util::push_span_window;
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
        ViewMode::Diff if line.starts_with("@@") => {
            let mut spans = Vec::new();
            push_span_window(
                &mut spans,
                line,
                0,
                line.len(),
                diff_hunk_style(),
                window_start,
                window_end,
            );
            spans
        }
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            let mut spans = Vec::new();
            push_span_window(
                &mut spans,
                line,
                0,
                line.len(),
                diff_file_style(),
                window_start,
                window_end,
            );
            spans
        }
        ViewMode::Diff if line.starts_with('+') => {
            highlight_diff_payload_window(line, diff_added_style(), window_start, window_end)
        }
        ViewMode::Diff if line.starts_with('-') => {
            highlight_diff_payload_window(line, diff_removed_style(), window_start, window_end)
        }
        ViewMode::Diff => highlight_structured_window(line, window_start, window_end, index),
    }
}

pub(in crate::viewer) fn highlight_diff_payload_window(
    line: &str,
    marker_style: Style,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    push_span_window(
        &mut spans,
        line,
        0,
        1,
        marker_style,
        window_start,
        window_end,
    );
    spans.extend(highlight_structured_window(
        &line[1..],
        window_start.saturating_sub(1),
        window_end.saturating_sub(1),
        None,
    ));
    spans
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
