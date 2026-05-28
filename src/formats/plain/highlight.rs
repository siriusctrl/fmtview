use ratatui::text::Span;

use crate::{formats::shared::push_span_window, tui::palette::plain_style};

pub(crate) fn highlight_plain_window(
    line: &str,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    push_span_window(
        &mut spans,
        line,
        0,
        line.len(),
        plain_style(),
        window_start,
        window_end,
    );
    spans
}
