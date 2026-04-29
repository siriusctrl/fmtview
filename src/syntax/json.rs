use std::ops::Range;

use ratatui::{style::Style, text::Span};

use super::{
    checkpoints::HighlightCheckpointIndex,
    util::{escape_token_end, floor_char_boundary, push_span_window, take_while},
    xml::{XmlPairState, apply_xml_tag_state, highlight_xml_tag_window, looks_like_xml_tag},
};
use crate::viewer::palette::{
    bool_style, escape_style, key_style, null_style, number_style, plain_style, punctuation_style,
    string_style,
};

#[cfg(test)]
pub(crate) fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    highlight_json_like_window(line, 0, line.len(), None)
}

pub(crate) fn highlight_json_like_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    mut index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    let mut value_string_state = None;
    if let Some(checkpoint_index) = index.as_deref_mut()
        && let Some(checkpoint) = checkpoint_index.json_value_before(window_start)
    {
        cursor = checkpoint.byte;
        value_string_state = Some(checkpoint.state);
    }

    while cursor < line.len() && cursor < window_end {
        if let Some(state) = value_string_state.take() {
            let (end, closed) = highlight_json_value_string_continue_window(
                line,
                cursor,
                window_end,
                state,
                window_start..window_end,
                &mut spans,
                index.as_deref_mut(),
            );
            cursor = end;
            if !closed {
                break;
            }
            continue;
        }

        let rest = &line[cursor..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            let end = take_while(line, cursor, char::is_whitespace);
            push_span_window(
                &mut spans,
                line,
                cursor,
                end,
                plain_style(),
                window_start,
                window_end,
            );
            cursor = end;
            continue;
        }

        if ch == '"' {
            if json_quote_starts_value(line, cursor) {
                let (end, closed) = highlight_json_string_value_window(
                    line,
                    cursor,
                    window_end,
                    window_start,
                    window_end,
                    &mut spans,
                    index.as_deref_mut(),
                );
                cursor = end;
                if !closed {
                    break;
                }
                continue;
            }

            let (end, closed) = json_string_end_until(line, cursor, window_end);
            if closed && json_string_is_key(line, end) {
                push_span_window(
                    &mut spans,
                    line,
                    cursor,
                    end,
                    key_style(),
                    window_start,
                    window_end,
                );
            } else {
                let (end, closed) = highlight_json_string_value_window(
                    line,
                    cursor,
                    window_end,
                    window_start,
                    window_end,
                    &mut spans,
                    index.as_deref_mut(),
                );
                cursor = end;
                if !closed {
                    break;
                }
                continue;
            }
            cursor = end;
            continue;
        }

        if ch == '-' || ch.is_ascii_digit() {
            let end = take_while(line, cursor, is_json_number_char);
            push_span_window(
                &mut spans,
                line,
                cursor,
                end,
                number_style(),
                window_start,
                window_end,
            );
            cursor = end;
            continue;
        }

        if let Some((word, style)) = json_keyword(rest) {
            push_span_window(
                &mut spans,
                line,
                cursor,
                cursor + word.len(),
                style,
                window_start,
                window_end,
            );
            cursor += word.len();
            continue;
        }

        if "{}[]:,".contains(ch) {
            push_span_window(
                &mut spans,
                line,
                cursor,
                cursor + ch.len_utf8(),
                punctuation_style(),
                window_start,
                window_end,
            );
            cursor += ch.len_utf8();
            continue;
        }

        push_span_window(
            &mut spans,
            line,
            cursor,
            cursor + ch.len_utf8(),
            plain_style(),
            window_start,
            window_end,
        );
        cursor += ch.len_utf8();
    }

    spans
}

pub(crate) fn highlight_json_string_value_window(
    source: &str,
    start: usize,
    limit: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
    checkpoints: Option<&mut HighlightCheckpointIndex>,
) -> (usize, bool) {
    let inner_start = start + '"'.len_utf8();
    push_span_window(
        spans,
        source,
        start,
        inner_start,
        string_style(),
        window_start,
        window_end,
    );
    highlight_json_value_string_continue_window(
        source,
        inner_start,
        limit,
        XmlPairState::default(),
        window_start..window_end,
        spans,
        checkpoints,
    )
}

pub(crate) fn highlight_json_value_string_continue_window(
    source: &str,
    start: usize,
    limit: usize,
    mut state: XmlPairState,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
    mut checkpoints: Option<&mut HighlightCheckpointIndex>,
) -> (usize, bool) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = start;
    let mut plain_start = start;
    let limit = limit.min(source.len());

    while index < limit {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember_json_value(index, &state);
        }

        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= limit)
        {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            push_span_window(
                spans,
                source,
                index,
                escape_end,
                escape_style(),
                window_start,
                window_end,
            );
            index = escape_end;
            plain_start = index;
            continue;
        }

        let Some(ch) = source[index..limit].chars().next() else {
            break;
        };

        if ch == '"' {
            push_span_window(
                spans,
                source,
                plain_start,
                index,
                string_style(),
                window_start,
                window_end,
            );
            let end = index + ch.len_utf8();
            push_span_window(
                spans,
                source,
                index,
                end,
                string_style(),
                window_start,
                window_end,
            );
            return (end, true);
        }

        if ch == '<' {
            let rest = &source[index..limit];
            let tag_end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(limit);
            let tag = &source[index..tag_end];
            if looks_like_xml_tag(tag) {
                push_span_window(
                    spans,
                    source,
                    plain_start,
                    index,
                    string_style(),
                    window_start,
                    window_end,
                );
                if tag_end <= window_start {
                    apply_xml_tag_state(tag, &mut state, 0);
                } else {
                    highlight_xml_tag_window(
                        source,
                        index,
                        tag_end,
                        &mut state,
                        0,
                        window_start..window_end,
                        spans,
                    );
                }
                index = tag_end;
                plain_start = index;
                continue;
            }
        }

        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        limit,
        string_style(),
        window_start,
        window_end,
    );
    (limit, false)
}

pub(crate) fn json_string_end_until(line: &str, start: usize, limit: usize) -> (usize, bool) {
    if start >= line.len() {
        return (line.len(), false);
    }

    let limit = floor_char_boundary(line, limit.min(line.len()));
    let mut escaped = false;
    let mut index = (start + 1).min(limit);
    while index < limit {
        let Some(ch) = line[index..limit].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return (index + ch.len_utf8(), true);
        }

        index += ch.len_utf8();
    }

    (limit, false)
}

pub(crate) fn json_quote_starts_value(line: &str, quote_start: usize) -> bool {
    line[..quote_start]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, ':' | '['))
}

pub(crate) fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..].trim_start().starts_with(':')
}

pub(crate) fn is_json_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')
}

pub(crate) fn json_keyword(rest: &str) -> Option<(&str, Style)> {
    for keyword in ["true", "false"] {
        if rest.starts_with(keyword) && keyword_boundary(rest, keyword.len()) {
            return Some((keyword, bool_style()));
        }
    }

    if rest.starts_with("null") && keyword_boundary(rest, "null".len()) {
        Some(("null", null_style()))
    } else {
        None
    }
}

pub(crate) fn keyword_boundary(rest: &str, end: usize) -> bool {
    rest[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}
