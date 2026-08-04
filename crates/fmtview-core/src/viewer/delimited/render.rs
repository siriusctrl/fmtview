use std::ops::Range;

use ratatui::{
    layout::Size,
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    formats::{detect_markup_kind, highlight_content_window, highlight_large_content_window},
    transform::{FormatKind, format_record_to_string},
    tui::{
        palette::{
            bool_style, gutter_style, key_style, null_style, number_style, plain_style,
            search_inactive_match_bg, string_style,
        },
        text::format_count,
        wrap::{
            WrapCheckpointIndex, continuation_indent, next_wrap_end, wrap_ranges_window_indexed,
            wrapped_row_count,
        },
    },
};

use super::{DelimitedViewer, Focus, PromptKind};
use crate::viewer::file::render::apply_search_ranges_to_spans;

const MAX_FORMATTED_CELL_BYTES: usize = 1024 * 1024;

pub(super) struct RenderedDelimitedView {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) title: String,
    pub(super) footer: String,
    pub(super) total_value_rows: usize,
    pub(super) value_height: usize,
    pub(super) value_width: usize,
}

pub(super) struct CellDisplay {
    text: String,
    lines: Vec<Range<usize>>,
    mode: FormatKind,
    scalar_style: Option<Style>,
    formatted: bool,
    cached_width: usize,
    row_counts: Vec<usize>,
    wrap_indices: Vec<WrapCheckpointIndex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CellMatch {
    pub(super) line: usize,
    pub(super) start_byte: usize,
}

impl CellDisplay {
    pub(super) fn new(value: String) -> Self {
        let (text, mode, scalar_style, formatted) = display_value(value);
        let lines = line_ranges(&text);
        let count = lines.len();
        Self {
            text,
            lines,
            mode,
            scalar_style,
            formatted,
            cached_width: 0,
            row_counts: Vec::new(),
            wrap_indices: (0..count).map(|_| WrapCheckpointIndex::default()).collect(),
        }
    }

    pub(super) fn find_match(
        &self,
        query: &str,
        current: Option<CellMatch>,
        forward: bool,
    ) -> Option<CellMatch> {
        if query.is_empty() || self.lines.is_empty() {
            return None;
        }
        if forward {
            self.find_forward(query, current)
        } else {
            self.find_backward(query, current)
        }
    }

    pub(super) fn visual_offset_for_match(&self, found: CellMatch) -> usize {
        let preceding = self.row_counts.iter().take(found.line).sum::<usize>();
        let line = self.line(found.line);
        preceding.saturating_add(wrapped_row_for_byte(
            line,
            self.cached_width.max(1),
            found.start_byte,
        ))
    }

    pub(super) fn ensure_layout(&mut self, width: usize) {
        let width = width.max(1);
        if self.cached_width == width && self.row_counts.len() == self.lines.len() {
            return;
        }
        self.cached_width = width;
        self.row_counts = self
            .lines
            .iter()
            .map(|range| {
                let line = &self.text[range.clone()];
                wrapped_row_count(line, width, continuation_indent(line, width))
            })
            .collect();
        self.wrap_indices = (0..self.lines.len())
            .map(|_| WrapCheckpointIndex::default())
            .collect();
    }

    fn total_rows(&self) -> usize {
        self.row_counts.iter().sum::<usize>().max(1)
    }

    fn visible_rows(
        &mut self,
        width: usize,
        scroll: usize,
        height: usize,
        query: Option<&str>,
        active: Option<CellMatch>,
    ) -> Vec<Line<'static>> {
        self.ensure_layout(width);
        if height == 0 {
            return Vec::new();
        }
        let mut skipped = scroll.min(self.total_rows().saturating_sub(1));
        let mut logical = 0;
        while logical < self.row_counts.len() && skipped >= self.row_counts[logical] {
            skipped -= self.row_counts[logical];
            logical += 1;
        }

        let mut rendered = Vec::with_capacity(height);
        while logical < self.lines.len() && rendered.len() < height {
            let line = &self.text[self.lines[logical].clone()];
            let remaining = height - rendered.len();
            let window = wrap_ranges_window_indexed(
                line,
                width,
                continuation_indent(line, width),
                skipped,
                remaining,
                self.wrap_indices.get_mut(logical),
            );
            for range in window.ranges {
                let mut spans = Vec::new();
                if range.continuation_indent > 0 {
                    spans.push(Span::styled(
                        " ".repeat(range.continuation_indent),
                        plain_style(),
                    ));
                }
                if let Some(style) = self.scalar_style {
                    spans.push(Span::styled(
                        line[range.start_byte..range.end_byte].to_owned(),
                        style,
                    ));
                } else if !self.formatted && self.text.len() > MAX_FORMATTED_CELL_BYTES {
                    spans.extend(highlight_large_content_window(
                        line,
                        self.mode,
                        range.start_byte,
                        range.end_byte,
                    ));
                } else {
                    spans.extend(highlight_content_window(
                        line,
                        self.mode,
                        range.start_byte,
                        range.end_byte,
                    ));
                }
                let (match_ranges, active_range) = row_search_ranges(
                    line,
                    query.filter(|query| !query.is_empty()),
                    logical,
                    active,
                    range.start_byte,
                    range.end_byte,
                    range.continuation_indent,
                );
                rendered.push(Line::from(apply_search_ranges_to_spans(
                    &spans,
                    &match_ranges,
                    active_range,
                )));
            }
            logical += 1;
            skipped = 0;
        }
        if rendered.is_empty() {
            rendered.push(Line::from(Span::styled("", plain_style())));
        }
        rendered
    }

    fn label(&self) -> &'static str {
        if self.scalar_style == Some(number_style()) {
            "number"
        } else if self.scalar_style == Some(bool_style()) {
            "boolean"
        } else if self.scalar_style == Some(null_style()) {
            "null"
        } else {
            match self.mode {
                FormatKind::Json if !self.formatted => "JSON raw",
                FormatKind::Xml if !self.formatted => "XML raw",
                FormatKind::Html if !self.formatted => "HTML raw",
                FormatKind::Json => "JSON",
                FormatKind::Xml => "XML",
                FormatKind::Html => "HTML",
                _ => "text",
            }
        }
    }

    fn line(&self, index: usize) -> &str {
        self.lines
            .get(index)
            .map(|range| &self.text[range.clone()])
            .unwrap_or("")
    }

    fn find_forward(&self, query: &str, current: Option<CellMatch>) -> Option<CellMatch> {
        if let Some(current) = current {
            let line = self.line(current.line);
            let after = current
                .start_byte
                .saturating_add(query.len())
                .min(line.len());
            if let Some(offset) = line[after..].find(query) {
                return Some(CellMatch {
                    line: current.line,
                    start_byte: after + offset,
                });
            }
            for line_index in current.line.saturating_add(1)..self.lines.len() {
                if let Some(found) = first_match(self.line(line_index), query, line_index) {
                    return Some(found);
                }
            }
            for line_index in 0..current.line {
                if let Some(found) = first_match(self.line(line_index), query, line_index) {
                    return Some(found);
                }
            }
            return self.line(current.line)[..current.start_byte.min(line.len())]
                .find(query)
                .map(|start_byte| CellMatch {
                    line: current.line,
                    start_byte,
                });
        }
        (0..self.lines.len())
            .find_map(|line_index| first_match(self.line(line_index), query, line_index))
    }

    fn find_backward(&self, query: &str, current: Option<CellMatch>) -> Option<CellMatch> {
        if let Some(current) = current {
            let line = self.line(current.line);
            if let Some(start_byte) = line[..current.start_byte.min(line.len())].rfind(query) {
                return Some(CellMatch {
                    line: current.line,
                    start_byte,
                });
            }
            for line_index in (0..current.line).rev() {
                if let Some(found) = last_match(self.line(line_index), query, line_index) {
                    return Some(found);
                }
            }
            for line_index in (current.line.saturating_add(1)..self.lines.len()).rev() {
                if let Some(found) = last_match(self.line(line_index), query, line_index) {
                    return Some(found);
                }
            }
            let after = current
                .start_byte
                .saturating_add(query.len())
                .min(line.len());
            return line[after..].rfind(query).map(|offset| CellMatch {
                line: current.line,
                start_byte: after + offset,
            });
        }
        (0..self.lines.len())
            .rev()
            .find_map(|line_index| last_match(self.line(line_index), query, line_index))
    }
}

pub(super) fn render(viewer: &mut DelimitedViewer, size: Size) -> RenderedDelimitedView {
    let content_width = usize::from(size.width.saturating_sub(2));
    let body_height = usize::from(size.height.saturating_sub(3));
    let sidebar_width = sidebar_width(content_width);
    let value_width = content_width
        .saturating_sub(sidebar_width)
        .saturating_sub(1);
    let value_height = body_height.saturating_sub(1).max(1);

    viewer.cell.ensure_layout(value_width);
    let total_value_rows = viewer.cell.total_rows();
    let max_scroll = total_value_rows.saturating_sub(value_height);
    viewer.value_scroll = viewer.value_scroll.min(max_scroll);

    let visible_fields = viewer.visible_fields(body_height);
    let value_rows = viewer.cell.visible_rows(
        value_width,
        viewer.value_scroll,
        value_height,
        (viewer.focus == Focus::Value).then_some(viewer.value_query.as_str()),
        (viewer.focus == Focus::Value)
            .then_some(viewer.value_search_match)
            .flatten(),
    );

    let mut lines = Vec::with_capacity(body_height);
    for row in 0..body_height {
        let sidebar = sidebar_line(viewer, visible_fields.get(row).copied(), sidebar_width);
        let mut spans = sidebar.spans;
        spans.push(Span::styled("│", gutter_style()));
        if row == 0 {
            let field_name = viewer
                .dataset
                .headers()
                .get(viewer.field)
                .map(String::as_str)
                .unwrap_or("no fields");
            spans.push(Span::styled(
                fit_text(
                    &format!(" {field_name}  · {}", viewer.cell.label()),
                    value_width,
                ),
                key_style().add_modifier(Modifier::BOLD),
            ));
        } else if let Some(value) = value_rows.get(row - 1) {
            spans.extend(value.spans.clone());
        }
        lines.push(Line::from(spans));
    }

    RenderedDelimitedView {
        lines,
        title: title(viewer),
        footer: footer(viewer),
        total_value_rows,
        value_height,
        value_width,
    }
}

fn title(viewer: &DelimitedViewer) -> String {
    let records = format_count(viewer.dataset.record_count());
    let suffix = if viewer.dataset.record_count_exact() {
        ""
    } else {
        "+"
    };
    let record = viewer
        .dataset
        .current_index()
        .map(|index| format_count(index + 1))
        .unwrap_or_else(|| "0".to_owned());
    let field = if viewer.dataset.headers().is_empty() {
        0
    } else {
        viewer.field + 1
    };
    let focus = if viewer.focus == Focus::Fields {
        "fields"
    } else {
        "value"
    };
    format!(
        " {} | {} {} | record {record}/{records}{suffix} | field {field}/{} | {focus} ",
        viewer.dataset.label(),
        format_kind_label(viewer.dataset.kind()),
        viewer.dataset.dialect().display(),
        viewer.dataset.headers().len(),
    )
}

fn footer(viewer: &DelimitedViewer) -> String {
    if let Some(prompt) = viewer.prompt.as_ref() {
        let label = if prompt.kind == PromptKind::Field {
            "find field"
        } else {
            "search value"
        };
        return format!(
            " {label}: {} | Enter accept | Backspace edit | Esc cancel ",
            prompt.buffer
        );
    }
    if let Some(notice) = viewer.notice.as_deref() {
        return format!(" {notice} | Esc/q close ");
    }
    if viewer.focus == Focus::Value {
        let search = if viewer.value_query.is_empty() {
            String::new()
        } else {
            format!("search: {} | n/N | ", viewer.value_query)
        };
        return format!(
            " value {}/{} | {search}↑/↓ scroll | Space/Page | / search | Esc fields | q quit ",
            viewer.value_scroll.saturating_add(1),
            viewer.last_total_rows,
        );
    }
    " ↑/↓ field | ←/→ record | / find field | Enter value | Page records | q quit ".to_owned()
}

fn sidebar_line(viewer: &DelimitedViewer, field: Option<usize>, width: usize) -> Line<'static> {
    let Some(field) = field else {
        return Line::from(Span::styled(" ".repeat(width), gutter_style()));
    };
    let name = viewer
        .dataset
        .headers()
        .get(field)
        .map(String::as_str)
        .unwrap_or("");
    let selected = field == viewer.field;
    let marker = if selected { '›' } else { ' ' };
    let text = fit_text(&format!("{marker} {:>4} {name}", field + 1), width);
    let style = if selected {
        key_style()
            .bg(search_inactive_match_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        gutter_style()
    };
    Line::from(Span::styled(text, style))
}

fn sidebar_width(content_width: usize) -> usize {
    if content_width < 32 {
        return content_width.saturating_div(3).max(8);
    }
    content_width.saturating_div(3).clamp(18, 36)
}

fn fit_text(text: &str, width: usize) -> String {
    let mut output = String::new();
    let mut used = 0_usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if used.saturating_add(ch_width) > width {
            break;
        }
        output.push(ch);
        used += ch_width;
    }
    output.push_str(&" ".repeat(width.saturating_sub(used)));
    output
}

fn display_value(value: String) -> (String, FormatKind, Option<Style>, bool) {
    let trimmed = value.trim();
    if value.len() <= MAX_FORMATTED_CELL_BYTES
        && matches!(trimmed.as_bytes().first(), Some(b'{' | b'['))
        && let Ok(formatted) = format_record_to_string(trimmed.as_bytes(), FormatKind::Json, 2)
    {
        return (formatted, FormatKind::Json, None, true);
    }
    if trimmed.starts_with('<') {
        let kind = detect_markup_kind(trimmed.as_bytes());
        if value.len() <= MAX_FORMATTED_CELL_BYTES
            && let Ok(formatted) = format_record_to_string(trimmed.as_bytes(), kind, 2)
        {
            return (formatted, kind, None, true);
        }
        return (value, kind, None, false);
    }
    if matches!(trimmed.as_bytes().first(), Some(b'{' | b'[')) {
        return (value, FormatKind::Json, None, false);
    }
    let scalar_style = if trimmed.len() > 128 {
        Some(string_style())
    } else if trimmed.is_empty() {
        Some(gutter_style())
    } else {
        let lower = trimmed.to_ascii_lowercase();
        if lower == "true" || lower == "false" {
            Some(bool_style())
        } else if matches!(lower.as_str(), "null" | "nil" | "none" | "na" | "n/a") {
            Some(null_style())
        } else if trimmed.parse::<f64>().is_ok() {
            Some(number_style())
        } else {
            Some(string_style())
        }
    };
    (value, FormatKind::Plain, scalar_style, false)
}

fn line_ranges(value: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        let end = if index > start && value.as_bytes()[index - 1] == b'\r' {
            index - 1
        } else {
            index
        };
        lines.push(start..end);
        start = index + 1;
    }
    lines.push(start..value.len());
    lines
}

fn first_match(line: &str, query: &str, line_index: usize) -> Option<CellMatch> {
    line.find(query).map(|start_byte| CellMatch {
        line: line_index,
        start_byte,
    })
}

fn last_match(line: &str, query: &str, line_index: usize) -> Option<CellMatch> {
    line.rfind(query).map(|start_byte| CellMatch {
        line: line_index,
        start_byte,
    })
}

fn wrapped_row_for_byte(line: &str, width: usize, byte: usize) -> usize {
    if line.is_empty() {
        return 0;
    }
    let target = byte.min(line.len().saturating_sub(1));
    let indent = continuation_indent(line, width);
    let mut row = 0;
    let mut start_byte = 0;
    let mut start_char = 0;
    while start_byte < line.len() {
        let row_width = if row == 0 {
            width.max(1)
        } else {
            width
                .saturating_sub(indent.min(width.saturating_sub(1)))
                .max(1)
        };
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        if target < end_byte || end_byte >= line.len() {
            return row;
        }
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        row += 1;
    }
    row
}

fn row_search_ranges(
    line: &str,
    query: Option<&str>,
    logical_line: usize,
    active: Option<CellMatch>,
    row_start_byte: usize,
    row_end_byte: usize,
    prefix_chars: usize,
) -> (Vec<Range<usize>>, Option<Range<usize>>) {
    let Some(query) = query else {
        return (Vec::new(), None);
    };
    let overlap = query.len().saturating_sub(1);
    let mut scan_start = row_start_byte.saturating_sub(overlap);
    while scan_start > 0 && !line.is_char_boundary(scan_start) {
        scan_start -= 1;
    }
    let mut scan_end = row_end_byte.saturating_add(overlap).min(line.len());
    while scan_end < line.len() && !line.is_char_boundary(scan_end) {
        scan_end += 1;
    }
    let ranges = line[scan_start..scan_end]
        .match_indices(query)
        .filter_map(|(offset, _)| {
            let match_start = scan_start + offset;
            let match_end = match_start + query.len();
            let start = match_start.max(row_start_byte);
            let end = match_end.min(row_end_byte);
            (start < end).then(|| {
                let local_start = line[row_start_byte..start].chars().count();
                let local_end = local_start + line[start..end].chars().count();
                prefix_chars + local_start..prefix_chars + local_end
            })
        })
        .collect::<Vec<_>>();
    let active = active
        .filter(|active| active.line == logical_line)
        .and_then(|active| {
            let match_start = active.start_byte.min(line.len());
            let match_end = match_start.saturating_add(query.len()).min(line.len());
            let start = match_start.max(row_start_byte);
            let end = match_end.min(row_end_byte);
            (start < end).then(|| {
                let local_start = line[row_start_byte..start].chars().count();
                let local_end = local_start + line[start..end].chars().count();
                prefix_chars + local_start..prefix_chars + local_end
            })
        });
    (ranges, active)
}

fn format_kind_label(kind: FormatKind) -> &'static str {
    match kind {
        FormatKind::Csv => "CSV",
        FormatKind::Tsv => "TSV",
        FormatKind::Xsv => "XSV",
        _ => "delimited",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_cells_reuse_existing_highlighters() {
        let display = CellDisplay::new(r#"{"items":[1,true]}"#.to_owned());
        assert_eq!(display.mode, FormatKind::Json);
        assert!(display.text.contains("\"items\""));
    }

    #[test]
    fn fit_text_uses_display_width_without_ellipsis() {
        assert_eq!(fit_text("你好 payload", 8), "你好 pay");
    }

    #[test]
    fn search_match_maps_to_its_actual_wrapped_row_and_repeats_within_a_line() {
        let mut display = CellDisplay::new(format!(
            "{}needle{}needle",
            "x".repeat(5000),
            "y".repeat(80)
        ));
        display.ensure_layout(40);

        let first = display.find_match("needle", None, true).unwrap();
        let second = display.find_match("needle", Some(first), true).unwrap();

        assert!(display.visual_offset_for_match(first) >= 124);
        assert!(display.visual_offset_for_match(second) > display.visual_offset_for_match(first));
    }

    #[test]
    fn search_highlight_survives_a_soft_wrap_boundary() {
        let mut display = CellDisplay::new("xxxxneedle".to_owned());
        let found = display.find_match("needle", None, true).unwrap();
        let rows = display.visible_rows(5, 0, 3, Some("needle"), Some(found));
        let highlighted_rows = rows
            .iter()
            .filter(|row| {
                row.spans
                    .iter()
                    .any(|span| span.style.bg == Some(crate::tui::palette::search_match_bg()))
            })
            .count();

        assert_eq!(highlighted_rows, 2);
    }

    #[test]
    fn oversized_structured_cell_stays_raw_to_bound_formatting_memory() {
        let display = CellDisplay::new(format!(
            "{{\"payload\":\"{}\"}}",
            "x".repeat(MAX_FORMATTED_CELL_BYTES)
        ));

        assert_eq!(display.mode, FormatKind::Json);
        assert!(!display.formatted);
        assert_eq!(display.label(), "JSON raw");
    }
}
