use std::ops::Range;

use ratatui::text::Span;

use super::{
    checkpoints::HighlightCheckpointIndex,
    util::{push_span_window, take_while},
    xml::highlight_inline_xml_window_indexed,
};
use crate::viewer::palette::{key_style, plain_style, punctuation_style, string_style};

pub(crate) fn highlight_jinja_line_window(
    line: &str,
    window_start: usize,
    window_end: usize,
    _index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cursor = 0_usize;
    let window = window_start..window_end;

    while cursor < line.len() && cursor < window_end {
        let Some((token_start, kind)) = next_jinja_token(line, cursor) else {
            highlight_host_window(line, cursor, line.len(), window.clone(), &mut spans);
            break;
        };

        if token_start > cursor {
            highlight_host_window(line, cursor, token_start, window.clone(), &mut spans);
        }

        let token_end = jinja_token_end(line, token_start, kind);
        highlight_jinja_token_window(
            line,
            token_start,
            token_end,
            kind,
            window.clone(),
            &mut spans,
        );
        cursor = token_end;
    }

    spans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JinjaTokenKind {
    Variable,
    Block,
    Comment,
}

fn next_jinja_token(line: &str, start: usize) -> Option<(usize, JinjaTokenKind)> {
    let mut best: Option<(usize, JinjaTokenKind)> = None;
    for (needle, kind) in [
        ("{{", JinjaTokenKind::Variable),
        ("{%", JinjaTokenKind::Block),
        ("{#", JinjaTokenKind::Comment),
    ] {
        if let Some(relative) = line[start..].find(needle) {
            let absolute = start + relative;
            if best.is_none_or(|(current, _)| absolute < current) {
                best = Some((absolute, kind));
            }
        }
    }
    best
}

fn jinja_token_end(line: &str, start: usize, kind: JinjaTokenKind) -> usize {
    let close = match kind {
        JinjaTokenKind::Variable => "}}",
        JinjaTokenKind::Block => "%}",
        JinjaTokenKind::Comment => "#}",
    };
    line[start + 2..]
        .find(close)
        .map(|relative| start + 2 + relative + close.len())
        .unwrap_or(line.len())
}

fn highlight_jinja_token_window(
    line: &str,
    start: usize,
    end: usize,
    kind: JinjaTokenKind,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
) {
    let opener_end = take_jinja_opener(line, start, kind).min(end);
    push_span_window(
        spans,
        line,
        start,
        opener_end,
        punctuation_style(),
        window.start,
        window.end,
    );

    let closer_start = jinja_closer_start(line, start, end, kind).unwrap_or(end);
    if closer_start > opener_end {
        let style = match kind {
            JinjaTokenKind::Variable | JinjaTokenKind::Block => key_style(),
            JinjaTokenKind::Comment => string_style(),
        };
        push_span_window(
            spans,
            line,
            opener_end,
            closer_start,
            style,
            window.start,
            window.end,
        );
    }

    if closer_start < end {
        push_span_window(
            spans,
            line,
            closer_start,
            end,
            punctuation_style(),
            window.start,
            window.end,
        );
    }
}

fn take_jinja_opener(line: &str, start: usize, kind: JinjaTokenKind) -> usize {
    let base = start + 2;
    if !matches!(kind, JinjaTokenKind::Comment) && line[base..].starts_with('-') {
        base + 1
    } else {
        base
    }
}

fn jinja_closer_start(line: &str, start: usize, end: usize, kind: JinjaTokenKind) -> Option<usize> {
    let close = match kind {
        JinjaTokenKind::Variable => "}}",
        JinjaTokenKind::Block => "%}",
        JinjaTokenKind::Comment => "#}",
    };
    let close_start = line[start + 2..end]
        .rfind(close)
        .map(|relative| start + 2 + relative)?;
    if close_start > start && line[..close_start].ends_with('-') {
        Some(close_start - 1)
    } else {
        Some(close_start)
    }
}

fn highlight_host_window(
    line: &str,
    start: usize,
    end: usize,
    window: Range<usize>,
    spans: &mut Vec<Span<'static>>,
) {
    if start >= end {
        return;
    }

    if line[start..end].contains('<') {
        highlight_inline_xml_window_indexed(line, start, end, 0, window, spans, None);
        return;
    }

    let whitespace_end = take_while(&line[start..end], 0, char::is_whitespace) + start;
    if whitespace_end > start {
        push_span_window(
            spans,
            line,
            start,
            whitespace_end,
            plain_style(),
            window.start,
            window.end,
        );
    }
    if whitespace_end < end {
        push_span_window(
            spans,
            line,
            whitespace_end,
            end,
            plain_style(),
            window.start,
            window.end,
        );
    }
}
