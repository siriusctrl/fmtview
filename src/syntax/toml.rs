use ratatui::text::Span;

use super::{
    checkpoints::HighlightCheckpointIndex,
    util::{highlight_string_segment_window, push_span_window, take_while},
};
use crate::viewer::palette::{
    bool_style, key_style, number_style, plain_style, punctuation_style, string_style,
};

pub(crate) fn highlight_toml_line_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    _index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let comment_start = comment_start(line).unwrap_or(line.len());
    let code = &line[..comment_start];
    let first = take_while(code, 0, char::is_whitespace);

    if first > 0 {
        push_plain(line, 0, first, window_start, window_end, &mut spans);
    }

    if first < comment_start && code[first..].starts_with('[') {
        highlight_section(
            line,
            first,
            comment_start,
            window_start,
            window_end,
            &mut spans,
        );
    } else if let Some(equal) = equal_start(code) {
        highlight_key_value(
            line,
            first,
            equal,
            comment_start,
            window_start,
            window_end,
            &mut spans,
        );
    } else if first < comment_start {
        push_plain(
            line,
            first,
            comment_start,
            window_start,
            window_end,
            &mut spans,
        );
    }

    if comment_start < line.len() {
        push_span_window(
            &mut spans,
            line,
            comment_start,
            line.len(),
            string_style(),
            window_start,
            window_end,
        );
    }

    spans
}

fn highlight_section(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let close = line[start..end]
        .rfind(']')
        .map(|relative| start + relative + 1)
        .unwrap_or(end);
    push_span_window(
        spans,
        line,
        start,
        start + 1,
        punctuation_style(),
        window_start,
        window_end,
    );
    if close > start + 1 {
        push_span_window(
            spans,
            line,
            start + 1,
            close.saturating_sub(1),
            key_style(),
            window_start,
            window_end,
        );
        push_span_window(
            spans,
            line,
            close.saturating_sub(1),
            close,
            punctuation_style(),
            window_start,
            window_end,
        );
    }
    if close < end {
        push_plain(line, close, end, window_start, window_end, spans);
    }
}

fn highlight_key_value(
    line: &str,
    first: usize,
    equal: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let key_end = trim_end_ascii_ws(line, first, equal);
    if first < key_end {
        push_span_window(
            spans,
            line,
            first,
            key_end,
            key_style(),
            window_start,
            window_end,
        );
    }
    if key_end < equal {
        push_plain(line, key_end, equal, window_start, window_end, spans);
    }
    push_span_window(
        spans,
        line,
        equal,
        equal + 1,
        punctuation_style(),
        window_start,
        window_end,
    );

    let value_start = take_while(line, equal + 1, char::is_whitespace).min(end);
    if equal + 1 < value_start {
        push_plain(
            line,
            equal + 1,
            value_start,
            window_start,
            window_end,
            spans,
        );
    }
    highlight_value(line, value_start, end, window_start, window_end, spans);
}

fn highlight_value(
    line: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if start >= end {
        return;
    }

    let style = match line[start..end].chars().next() {
        Some('"' | '\'') => {
            highlight_string_segment_window(line, start, end, window_start, window_end, spans);
            return;
        }
        Some('[' | ']' | '{' | '}') => punctuation_style(),
        _ if is_toml_bool(&line[start..end]) => bool_style(),
        _ if is_toml_number_like(&line[start..end]) => number_style(),
        _ => plain_style(),
    };

    push_span_window(spans, line, start, end, style, window_start, window_end);
}

fn comment_start(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '#' => return Some(index),
                _ => {}
            },
        }
    }

    None
}

fn equal_start(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                }
            }
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '=' => return Some(index),
                _ => {}
            },
        }
    }

    None
}

fn trim_end_ascii_ws(line: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let Some((previous, ch)) = line[..end].char_indices().next_back() else {
            break;
        };
        if !ch.is_ascii_whitespace() {
            break;
        }
        end = previous;
    }
    end
}

fn is_toml_bool(value: &str) -> bool {
    let value = value.trim_ascii_end();
    value == "true" || value == "false"
}

fn is_toml_number_like(value: &str) -> bool {
    let value = value.trim_ascii_end();
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '+')
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
