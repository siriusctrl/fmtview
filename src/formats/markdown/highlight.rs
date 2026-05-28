use ratatui::text::Span;

use crate::formats::{
    HighlightCheckpointIndex,
    shared::{push_span_window, take_while},
};
use crate::transform::FormatKind;
use crate::tui::palette::{key_style, plain_style, punctuation_style, string_style};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MarkdownFenceState {
    fence: Option<MarkdownFence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkdownFence {
    marker: char,
    marker_len: usize,
    format: FormatKind,
}

impl MarkdownFenceState {
    pub(crate) fn line_format(self, line: &str) -> FormatKind {
        match (self.fence, fence_line(line)) {
            (Some(fence), Some(line_fence)) if line_fence.closes(fence) => FormatKind::Markdown,
            (Some(fence), _) => fence.format,
            (None, _) => FormatKind::Markdown,
        }
    }

    pub(crate) fn advance(&mut self, line: &str) {
        let Some(line_fence) = fence_line(line) else {
            return;
        };

        match self.fence {
            Some(fence) if line_fence.closes(fence) => self.fence = None,
            Some(_) => {}
            None => {
                self.fence = Some(MarkdownFence {
                    marker: line_fence.marker,
                    marker_len: line_fence.marker_len,
                    format: format_for_fence_info(&line[line_fence.marker_end..]),
                });
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn markdown_line_formats(
    lines: &[String],
    mut state: MarkdownFenceState,
) -> Vec<FormatKind> {
    lines
        .iter()
        .map(|line| {
            let format = state.line_format(line);
            state.advance(line);
            format
        })
        .collect()
}

pub(crate) fn highlight_markdown_line_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    _index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let first = take_while(line, 0, char::is_whitespace);

    if first > 0 {
        push_plain(line, 0, first, window_start, window_end, &mut spans);
    }

    if first >= line.len() {
        return spans;
    }

    if let Some(marker_end) = fence_marker_end(line, first) {
        push_punctuation(
            line,
            first,
            marker_end,
            window_start,
            window_end,
            &mut spans,
        );
        if marker_end < line.len() {
            push_span_window(
                &mut spans,
                line,
                marker_end,
                line.len(),
                string_style(),
                window_start,
                window_end,
            );
        }
        return spans;
    }

    if let Some(marker_end) = heading_marker_end(line, first) {
        push_punctuation(
            line,
            first,
            marker_end,
            window_start,
            window_end,
            &mut spans,
        );
        highlight_inline(
            line,
            marker_end,
            line.len(),
            key_style(),
            window_start,
            window_end,
            &mut spans,
        );
        return spans;
    }

    if line[first..].starts_with('>') {
        push_punctuation(line, first, first + 1, window_start, window_end, &mut spans);
        highlight_inline(
            line,
            first + 1,
            line.len(),
            plain_style(),
            window_start,
            window_end,
            &mut spans,
        );
        return spans;
    }

    if let Some(marker_end) = list_marker_end(line, first) {
        push_punctuation(
            line,
            first,
            marker_end,
            window_start,
            window_end,
            &mut spans,
        );
        highlight_inline(
            line,
            marker_end,
            line.len(),
            plain_style(),
            window_start,
            window_end,
            &mut spans,
        );
        return spans;
    }

    highlight_inline(
        line,
        first,
        line.len(),
        plain_style(),
        window_start,
        window_end,
        &mut spans,
    );
    spans
}

fn highlight_inline(
    line: &str,
    start: usize,
    end: usize,
    base_style: ratatui::style::Style,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let mut cursor = start;
    let mut plain_start = start;

    while cursor < end {
        let Some((token_start, token)) = next_inline_token(line, cursor, end) else {
            break;
        };

        push_span_window(
            spans,
            line,
            plain_start,
            token_start,
            base_style,
            window_start,
            window_end,
        );

        let token_end = match token {
            InlineToken::Code => {
                highlight_inline_code(line, token_start, end, window_start, window_end, spans)
            }
            InlineToken::Link => {
                highlight_link(line, token_start, end, window_start, window_end, spans)
            }
            InlineToken::Emphasis(marker) => highlight_emphasis(
                line,
                token_start,
                end,
                marker,
                window_start,
                window_end,
                spans,
            ),
        };
        cursor = token_end;
        plain_start = cursor;
    }

    push_span_window(
        spans,
        line,
        plain_start,
        end,
        base_style,
        window_start,
        window_end,
    );
}

#[derive(Debug, Clone, Copy)]
enum InlineToken {
    Code,
    Link,
    Emphasis(char),
}

fn next_inline_token(line: &str, start: usize, end: usize) -> Option<(usize, InlineToken)> {
    line[start..end]
        .char_indices()
        .filter_map(|(relative, ch)| {
            let index = start + relative;
            match ch {
                '`' => Some((index, InlineToken::Code)),
                '[' => Some((index, InlineToken::Link)),
                '*' | '_' => Some((index, InlineToken::Emphasis(ch))),
                _ => None,
            }
        })
        .next()
}

fn highlight_inline_code(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) -> usize {
    let Some(close) = line[start + 1..end]
        .find('`')
        .map(|relative| start + 1 + relative)
    else {
        push_plain(line, start, start + 1, window_start, window_end, spans);
        return start + 1;
    };
    push_punctuation(line, start, start + 1, window_start, window_end, spans);
    push_span_window(
        spans,
        line,
        start + 1,
        close,
        string_style(),
        window_start,
        window_end,
    );
    push_punctuation(line, close, close + 1, window_start, window_end, spans);
    close + 1
}

fn highlight_link(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) -> usize {
    let Some(label_close) = line[start + 1..end]
        .find(']')
        .map(|relative| start + 1 + relative)
    else {
        push_plain(line, start, start + 1, window_start, window_end, spans);
        return start + 1;
    };
    let url_open = label_close + 1;
    if !line[url_open..end].starts_with('(') {
        push_plain(line, start, start + 1, window_start, window_end, spans);
        return start + 1;
    }
    let Some(url_close) = line[url_open + 1..end]
        .find(')')
        .map(|relative| url_open + 1 + relative)
    else {
        push_plain(line, start, start + 1, window_start, window_end, spans);
        return start + 1;
    };

    push_punctuation(line, start, start + 1, window_start, window_end, spans);
    push_span_window(
        spans,
        line,
        start + 1,
        label_close,
        key_style(),
        window_start,
        window_end,
    );
    push_punctuation(
        line,
        label_close,
        url_open + 1,
        window_start,
        window_end,
        spans,
    );
    push_span_window(
        spans,
        line,
        url_open + 1,
        url_close,
        string_style(),
        window_start,
        window_end,
    );
    push_punctuation(
        line,
        url_close,
        url_close + 1,
        window_start,
        window_end,
        spans,
    );
    url_close + 1
}

fn highlight_emphasis(
    line: &str,
    start: usize,
    end: usize,
    marker: char,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) -> usize {
    let marker_len = if line[start + marker.len_utf8()..end].starts_with(marker) {
        marker.len_utf8() * 2
    } else {
        marker.len_utf8()
    };
    let close_marker = marker.to_string().repeat(marker_len / marker.len_utf8());
    let content_start = start + marker_len;
    let Some(content_end) = line[content_start..end]
        .find(&close_marker)
        .map(|relative| content_start + relative)
    else {
        push_plain(
            line,
            start,
            start + marker.len_utf8(),
            window_start,
            window_end,
            spans,
        );
        return start + marker.len_utf8();
    };

    push_punctuation(line, start, content_start, window_start, window_end, spans);
    push_span_window(
        spans,
        line,
        content_start,
        content_end,
        key_style(),
        window_start,
        window_end,
    );
    push_punctuation(
        line,
        content_end,
        content_end + marker_len,
        window_start,
        window_end,
        spans,
    );
    content_end + marker_len
}

fn heading_marker_end(line: &str, start: usize) -> Option<usize> {
    let count = line[start..].chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&count) {
        return None;
    }
    let end = start + count;
    if end == line.len() || line[end..].starts_with(char::is_whitespace) {
        Some(end)
    } else {
        None
    }
}

fn fence_marker_end(line: &str, start: usize) -> Option<usize> {
    fence_line_from_start(line, start).map(|fence| fence.marker_end)
}

#[derive(Debug, Clone, Copy)]
struct MarkdownFenceLine {
    marker: char,
    marker_len: usize,
    marker_end: usize,
    trailing_is_blank: bool,
}

impl MarkdownFenceLine {
    fn closes(self, fence: MarkdownFence) -> bool {
        self.marker == fence.marker && self.marker_len >= fence.marker_len && self.trailing_is_blank
    }
}

fn fence_line(line: &str) -> Option<MarkdownFenceLine> {
    let start = markdown_marker_start(line)?;
    fence_line_from_start(line, start)
}

fn markdown_marker_start(line: &str) -> Option<usize> {
    let mut spaces = 0_usize;
    for (index, ch) in line.char_indices() {
        match ch {
            ' ' if spaces < 3 => spaces += 1,
            ' ' => return None,
            _ => return Some(index),
        }
    }
    None
}

fn fence_line_from_start(line: &str, start: usize) -> Option<MarkdownFenceLine> {
    let marker = line[start..].chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = line[start..].chars().take_while(|ch| *ch == marker).count();
    (count >= 3).then_some(MarkdownFenceLine {
        marker,
        marker_len: count,
        marker_end: start + marker.len_utf8() * count,
        trailing_is_blank: line[start + marker.len_utf8() * count..].trim().is_empty(),
    })
}

fn format_for_fence_info(info: &str) -> FormatKind {
    let language = normalized_fence_language(info);
    match language.as_deref() {
        Some("json" | "jsonc" | "jsonl" | "ndjson") => FormatKind::Json,
        Some("xml" | "html" | "xhtml" | "svg") => FormatKind::Xml,
        Some("toml") => FormatKind::Toml,
        Some("jinja" | "jinja2" | "j2" | "html+jinja") => FormatKind::Jinja,
        Some("md" | "markdown") => FormatKind::Markdown,
        _ => FormatKind::Plain,
    }
}

fn normalized_fence_language(info: &str) -> Option<String> {
    let raw = info.split_whitespace().next()?;
    let language = raw
        .trim_matches(|ch| matches!(ch, '{' | '}'))
        .trim_start_matches('.')
        .trim_end_matches(',');
    (!language.is_empty()).then(|| language.to_ascii_lowercase())
}

fn list_marker_end(line: &str, start: usize) -> Option<usize> {
    if line[start..].starts_with("- ")
        || line[start..].starts_with("* ")
        || line[start..].starts_with("+ ")
    {
        return Some(start + 2);
    }

    let digit_end = take_while(line, start, |ch| ch.is_ascii_digit());
    if digit_end == start || digit_end >= line.len() {
        return None;
    }
    let marker = line[digit_end..].chars().next()?;
    let marker_end = digit_end + marker.len_utf8();
    if (marker == '.' || marker == ')')
        && line[marker_end..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    {
        Some(marker_end + 1)
    } else {
        None
    }
}

fn push_plain(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    push_span_window(
        spans,
        line,
        start,
        end,
        plain_style(),
        window_start,
        window_end,
    );
}

fn push_punctuation(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    push_span_window(
        spans,
        line,
        start,
        end,
        punctuation_style(),
        window_start,
        window_end,
    );
}
