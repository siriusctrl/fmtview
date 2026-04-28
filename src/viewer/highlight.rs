use std::ops::Range;

use ratatui::{style::Style, text::Span};

use super::palette::{
    attr_style, bool_style, diff_added_style, diff_file_style, diff_hunk_style, diff_removed_style,
    error_style, escape_style, key_style, null_style, number_style, plain_style, punctuation_style,
    string_style, xml_depth_style,
};
use super::{HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES, ViewMode};

#[derive(Debug, Default)]
pub(super) struct HighlightCheckpointIndex {
    pub(super) json_value_strings: Vec<XmlHighlightCheckpoint>,
    pub(super) xml_lines: Vec<XmlHighlightCheckpoint>,
}

#[derive(Debug, Clone)]
pub(super) struct XmlHighlightCheckpoint {
    byte: usize,
    state: XmlPairState,
}

impl HighlightCheckpointIndex {
    pub(super) fn json_value_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.json_value_strings, byte)
    }

    pub(super) fn xml_line_before(&self, byte: usize) -> Option<XmlHighlightCheckpoint> {
        checkpoint_before(&self.xml_lines, byte)
    }

    pub(super) fn remember_json_value(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.json_value_strings, byte, state);
    }

    pub(super) fn remember_xml_line(&mut self, byte: usize, state: &XmlPairState) {
        remember_xml_checkpoint(&mut self.xml_lines, byte, state);
    }
}

pub(super) fn checkpoint_before(
    checkpoints: &[XmlHighlightCheckpoint],
    byte: usize,
) -> Option<XmlHighlightCheckpoint> {
    checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.byte <= byte)
        .cloned()
}

pub(super) fn remember_xml_checkpoint(
    checkpoints: &mut Vec<XmlHighlightCheckpoint>,
    byte: usize,
    state: &XmlPairState,
) {
    let next_byte = checkpoints
        .last()
        .map(|checkpoint| {
            checkpoint
                .byte
                .saturating_add(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES)
        })
        .unwrap_or(HIGHLIGHT_CHECKPOINT_INTERVAL_BYTES);
    if byte < next_byte {
        return;
    }

    match checkpoints.binary_search_by_key(&byte, |checkpoint| checkpoint.byte) {
        Ok(_) => {}
        Err(position) => checkpoints.insert(
            position,
            XmlHighlightCheckpoint {
                byte,
                state: state.clone(),
            },
        ),
    }
}

pub(super) fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    highlight_content_window(line, mode, 0, line.len())
}

pub(super) fn highlight_content_window(
    line: &str,
    mode: ViewMode,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    highlight_content_window_indexed(line, mode, window_start, window_end, None)
}

pub(super) fn highlight_content_window_indexed(
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

pub(super) fn highlight_diff_payload_window(
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

pub(super) fn highlight_structured_window(
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

#[cfg(test)]
pub(super) fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    highlight_json_like_window(line, 0, line.len(), None)
}

pub(super) fn highlight_json_like_window(
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

pub(super) fn highlight_json_string_value_window(
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

pub(super) fn highlight_json_value_string_continue_window(
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

pub(super) fn highlight_string_segment_window(
    source: &str,
    start: usize,
    end: usize,
    window_start: usize,
    window_end: usize,
    spans: &mut Vec<Span<'static>>,
) {
    if end <= window_start {
        return;
    }

    let mut index = start;
    let mut plain_start = start;

    while index < end {
        if let Some(escape_end) =
            escape_token_end(source, index).filter(|escape_end| *escape_end <= end)
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

        let Some(ch) = source[index..end].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    push_span_window(
        spans,
        source,
        plain_start,
        end,
        string_style(),
        window_start,
        window_end,
    );
}

#[cfg(test)]
pub(super) fn highlight_xml_line(line: &str) -> Vec<Span<'static>> {
    highlight_xml_line_window(line, 0, line.len(), None)
}

pub(super) fn highlight_xml_line_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let base_depth = xml_depth_from_indent(line);
    let mut spans = Vec::new();
    highlight_inline_xml_window_indexed(
        line,
        0,
        line.len(),
        base_depth,
        window_start..window_end,
        &mut spans,
        index,
    );
    spans
}

pub(super) fn highlight_inline_xml_window_indexed(
    source: &str,
    start: usize,
    end: usize,
    base_depth: usize,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
    mut checkpoints: Option<&mut HighlightCheckpointIndex>,
) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = start;
    let mut state = XmlPairState::default();
    if let Some(checkpoints) = checkpoints.as_deref_mut()
        && let Some(checkpoint) = checkpoints.xml_line_before(window_start)
    {
        index = checkpoint.byte;
        state = checkpoint.state;
    }

    while index < end && index < window_end {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember_xml_line(index, &state);
        }

        let rest = &source[index..end];
        if rest.starts_with('<') {
            let end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(end);
            let tag = &source[index..end];
            if looks_like_xml_tag(tag) {
                if end <= window_start {
                    apply_xml_tag_state(tag, &mut state, base_depth);
                } else {
                    highlight_xml_tag_window(
                        source,
                        index,
                        end,
                        &mut state,
                        base_depth,
                        window_start..window_end,
                        spans,
                    );
                }
            } else if end > window_start {
                highlight_string_segment_window(
                    source,
                    index,
                    end,
                    window_start,
                    window_end,
                    spans,
                );
            }
            index = end;
        } else {
            let end = rest
                .find('<')
                .map(|position| index + position)
                .unwrap_or(end);
            if end > window_start {
                highlight_string_segment_window(
                    source,
                    index,
                    end,
                    window_start,
                    window_end,
                    spans,
                );
            }
            index = end;
        }
    }
}

pub(super) fn highlight_xml_tag_window(
    source: &str,
    tag_start: usize,
    end: usize,
    state: &mut XmlPairState,
    base_depth: usize,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
) {
    let window_start = window.start;
    let window_end = window.end;
    let mut index = 0;
    let tag = &source[tag_start..end];
    let kind = xml_tag_kind(tag);
    let name_range = xml_tag_name_range(tag);
    let name = name_range.map(|(start, end)| &tag[start..end]);
    let tag_state = apply_xml_tag_state_with_parts(state, kind, name, base_depth);

    while index < tag.len() {
        let rest = &tag[index..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if let Some((name_start, name_end)) = name_range
            && index == name_start
        {
            let style = if tag_state.matched {
                xml_depth_style(tag_state.depth)
            } else {
                error_style()
            };
            push_span_window(
                spans,
                source,
                tag_start + name_start,
                tag_start + name_end,
                style,
                window_start,
                window_end,
            );
            index = name_end;
            continue;
        }

        if ch.is_whitespace() {
            let end = take_while(tag, index, char::is_whitespace);
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + end,
                plain_style(),
                window_start,
                window_end,
            );
            index = end;
            continue;
        }

        if rest.starts_with("\\\"") || rest.starts_with("\\'") {
            let quote = if rest.starts_with("\\\"") { '"' } else { '\'' };
            let end = escaped_quoted_end(tag, index, quote);
            highlight_string_segment_window(
                source,
                tag_start + index,
                tag_start + end,
                window_start,
                window_end,
                spans,
            );
            index = end;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let end = quoted_end(tag, index, ch);
            highlight_string_segment_window(
                source,
                tag_start + index,
                tag_start + end,
                window_start,
                window_end,
                spans,
            );
            index = end;
            continue;
        }

        if "<>/=?!".contains(ch) {
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + index + ch.len_utf8(),
                punctuation_style(),
                window_start,
                window_end,
            );
            index += ch.len_utf8();
            continue;
        }

        if is_xml_name_char(ch) {
            let end = take_while(tag, index, is_xml_name_char);
            push_span_window(
                spans,
                source,
                tag_start + index,
                tag_start + end,
                attr_style(),
                window_start,
                window_end,
            );
            index = end;
            continue;
        }

        push_span_window(
            spans,
            source,
            tag_start + index,
            tag_start + index + ch.len_utf8(),
            plain_style(),
            window_start,
            window_end,
        );
        index += ch.len_utf8();
    }
}

pub(super) fn apply_xml_tag_state(
    tag: &str,
    state: &mut XmlPairState,
    base_depth: usize,
) -> XmlTagState {
    let kind = xml_tag_kind(tag);
    let name = xml_tag_name_range(tag).map(|(start, end)| &tag[start..end]);
    apply_xml_tag_state_with_parts(state, kind, name, base_depth)
}

pub(super) fn apply_xml_tag_state_with_parts(
    state: &mut XmlPairState,
    kind: XmlTagKind,
    name: Option<&str>,
    base_depth: usize,
) -> XmlTagState {
    state.apply(kind, name, base_depth)
}

#[derive(Debug, Clone, Default)]
pub(super) struct XmlPairState {
    stack: Vec<XmlOpenTag>,
}

#[derive(Debug, Clone)]
pub(super) struct XmlOpenTag {
    name: String,
    depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XmlTagKind {
    Open,
    Close,
    SelfClosing,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct XmlTagState {
    depth: usize,
    matched: bool,
}

impl XmlPairState {
    fn apply(&mut self, kind: XmlTagKind, name: Option<&str>, base_depth: usize) -> XmlTagState {
        match (kind, name) {
            (XmlTagKind::Open, Some(name)) => {
                let depth = base_depth + self.stack.len();
                self.stack.push(XmlOpenTag {
                    name: name.to_owned(),
                    depth,
                });
                XmlTagState {
                    depth,
                    matched: true,
                }
            }
            (XmlTagKind::SelfClosing, Some(_)) => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
            (XmlTagKind::Close, Some(name)) => match self.stack.pop() {
                Some(open) if open.name == name => XmlTagState {
                    depth: open.depth,
                    matched: true,
                },
                Some(open) => {
                    self.stack.push(open);
                    XmlTagState {
                        depth: base_depth + self.stack.len() - 1,
                        matched: false,
                    }
                }
                None => XmlTagState {
                    depth: base_depth,
                    matched: false,
                },
            },
            _ => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
        }
    }
}

pub(super) fn looks_like_xml_tag(tag: &str) -> bool {
    tag.starts_with("</")
        || tag.starts_with("<?")
        || tag.starts_with("<!")
        || xml_tag_name_range(tag).is_some()
}

pub(super) fn xml_tag_kind(tag: &str) -> XmlTagKind {
    if tag.starts_with("</") {
        XmlTagKind::Close
    } else if tag.starts_with("<?") || tag.starts_with("<!") {
        XmlTagKind::Other
    } else if tag.trim_end_matches('>').trim_end().ends_with('/') {
        XmlTagKind::SelfClosing
    } else {
        XmlTagKind::Open
    }
}

pub(super) fn xml_tag_name_range(tag: &str) -> Option<(usize, usize)> {
    let mut index = if tag.starts_with("</") { 2 } else { 1 };
    while index < tag.len() {
        let ch = tag[index..].chars().next()?;
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }

    let start = index;
    let end = take_while(tag, start, is_xml_name_char);
    (end > start).then_some((start, end))
}

pub(super) fn xml_depth_from_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        / 2
}

pub(super) fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

pub(super) fn json_string_end_until(line: &str, start: usize, limit: usize) -> (usize, bool) {
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

pub(super) fn json_quote_starts_value(line: &str, quote_start: usize) -> bool {
    line[..quote_start]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| matches!(ch, ':' | '['))
}

pub(super) fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..].trim_start().starts_with(':')
}

pub(super) fn is_json_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')
}

pub(super) fn json_keyword(rest: &str) -> Option<(&str, Style)> {
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

pub(super) fn keyword_boundary(rest: &str, end: usize) -> bool {
    rest[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

pub(super) fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

pub(super) fn escaped_quoted_end(text: &str, start: usize, quote: char) -> usize {
    let pattern = if quote == '"' { "\\\"" } else { "\\'" };
    text[start + pattern.len()..]
        .find(pattern)
        .map(|offset| start + pattern.len() + offset + pattern.len())
        .unwrap_or(text.len())
}

pub(super) fn escape_token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    if !rest.starts_with('\\') {
        return None;
    }

    let mut chars = rest.chars();
    chars.next()?;
    let escaped = chars.next()?;
    let escaped_start = start + '\\'.len_utf8();
    let escaped_end = escaped_start + escaped.len_utf8();

    if escaped == 'u' {
        let unicode_end = escaped_end + 4;
        if text
            .get(escaped_end..unicode_end)
            .is_some_and(|digits| digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        {
            return Some(unicode_end);
        }
    }

    Some(escaped_end)
}

pub(super) fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}

pub(super) fn push_span_window(
    spans: &mut Vec<Span<'static>>,
    source: &str,
    start: usize,
    end: usize,
    style: Style,
    window_start: usize,
    window_end: usize,
) {
    let start = floor_char_boundary(source, start.min(source.len()));
    let end = floor_char_boundary(source, end.min(source.len()));
    let window_start = floor_char_boundary(source, window_start.min(source.len()));
    let window_end = floor_char_boundary(source, window_end.min(source.len()));
    let overlap_start = start.max(window_start);
    let overlap_end = end.min(window_end);
    if overlap_start < overlap_end {
        let style = normalize_span_style(style);
        push_text_span(spans, source[overlap_start..overlap_end].to_owned(), style);
    }
}

pub(super) fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn normalize_span_style(style: Style) -> Style {
    if style == plain_style() {
        Style::default()
    } else {
        style
    }
}

fn push_text_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if text.is_empty() {
        return;
    }

    if style == Style::default()
        && let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push_str(&text);
        return;
    }

    spans.push(Span::styled(text, style));
}
