use std::ops::Range;

use ratatui::text::Span;

use super::{
    checkpoints::HighlightCheckpointIndex,
    util::{
        escaped_quoted_end, highlight_string_segment_window, push_span_window, quoted_end,
        take_while,
    },
};
use crate::viewer::palette::{
    attr_style, error_style, plain_style, punctuation_style, xml_depth_style,
};

#[cfg(test)]
pub(crate) fn highlight_xml_line(line: &str) -> Vec<Span<'static>> {
    highlight_xml_line_window(line, 0, line.len(), None)
}

pub(crate) fn highlight_xml_line_window(
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

pub(crate) fn highlight_inline_xml_window_indexed(
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

pub(crate) fn highlight_xml_tag_window(
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

pub(crate) fn apply_xml_tag_state(
    tag: &str,
    state: &mut XmlPairState,
    base_depth: usize,
) -> XmlTagState {
    let kind = xml_tag_kind(tag);
    let name = xml_tag_name_range(tag).map(|(start, end)| &tag[start..end]);
    apply_xml_tag_state_with_parts(state, kind, name, base_depth)
}

pub(crate) fn apply_xml_tag_state_with_parts(
    state: &mut XmlPairState,
    kind: XmlTagKind,
    name: Option<&str>,
    base_depth: usize,
) -> XmlTagState {
    state.apply(kind, name, base_depth)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct XmlPairState {
    stack: Vec<XmlOpenTag>,
}

#[derive(Debug, Clone)]
pub(crate) struct XmlOpenTag {
    name: String,
    depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum XmlTagKind {
    Open,
    Close,
    SelfClosing,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct XmlTagState {
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

pub(crate) fn looks_like_xml_tag(tag: &str) -> bool {
    tag.starts_with("</")
        || tag.starts_with("<?")
        || tag.starts_with("<!")
        || xml_tag_name_range(tag).is_some()
}

pub(crate) fn xml_tag_kind(tag: &str) -> XmlTagKind {
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

pub(crate) fn xml_tag_name_range(tag: &str) -> Option<(usize, usize)> {
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

pub(crate) fn xml_depth_from_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        / 2
}

pub(crate) fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}
