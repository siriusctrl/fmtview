mod checkpoints;
mod jinja;
mod json;
mod util;
mod xml;

pub(crate) use checkpoints::HighlightCheckpointIndex;
#[cfg(test)]
pub(crate) use json::highlight_json_like;
#[cfg(test)]
pub(crate) use xml::highlight_xml_line;

use ratatui::text::Span;

use crate::viewer::palette::plain_style;
use jinja::highlight_jinja_line_window;
use json::highlight_json_like_window;
use util::push_span_window;
use xml::highlight_xml_line_window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxKind {
    Plain,
    Structured,
    Jinja,
}

pub(crate) fn highlight_content(line: &str, syntax: SyntaxKind) -> Vec<Span<'static>> {
    highlight_content_window(line, syntax, 0, line.len())
}

pub(crate) fn highlight_content_window(
    line: &str,
    syntax: SyntaxKind,
    window_start: usize,
    window_end: usize,
) -> Vec<Span<'static>> {
    highlight_content_window_indexed(line, syntax, window_start, window_end, None)
}

pub(crate) fn highlight_content_window_indexed(
    line: &str,
    syntax: SyntaxKind,
    window_start: usize,
    window_end: usize,
    index: Option<&mut HighlightCheckpointIndex>,
) -> Vec<Span<'static>> {
    let window_start = window_start.min(line.len());
    let window_end = window_end.min(line.len()).max(window_start);
    match syntax {
        SyntaxKind::Plain => highlight_plain_window(line, window_start, window_end),
        SyntaxKind::Structured => {
            highlight_structured_window(line, window_start, window_end, index)
        }
        SyntaxKind::Jinja => highlight_jinja_line_window(line, window_start, window_end, index),
    }
}

fn highlight_plain_window(
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

pub(crate) fn highlight_structured_window(
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
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    #[ignore = "performance smoke; run benches/syntax-performance.sh"]
    fn perf_syntax_highlight_window() {
        let repeated = r#"<item id=\"1\"><name>visible</name></item>""#.repeat(32_768);
        let line = format!(r#"  "message": "{repeated}""#);
        let window_width = 8 * 1024;
        let mut checkpoints = HighlightCheckpointIndex::default();
        let started = Instant::now();
        let mut spans = 0_usize;

        for start in (0..line.len()).step_by(window_width) {
            let end = start.saturating_add(window_width).min(line.len());
            spans = spans.saturating_add(
                highlight_content_window_indexed(
                    &line,
                    SyntaxKind::Structured,
                    start,
                    end,
                    Some(&mut checkpoints),
                )
                .len(),
            );
        }

        let elapsed = started.elapsed();
        eprintln!(
            "syntax highlight window: {elapsed:?}, windows={}, input_bytes={}, spans={spans}",
            line.len().div_ceil(window_width),
            line.len()
        );
        assert!(spans > 0);
        assert!(
            elapsed < Duration::from_secs(5),
            "syntax highlight window took {elapsed:?}"
        );
    }

    #[test]
    fn plain_highlight_preserves_visible_text() {
        let text = "plain {{ not special }} <not-a-tag>";
        let spans = highlight_content(text, SyntaxKind::Plain);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn jinja_highlight_preserves_visible_text() {
        let text = r#"<h1>{{ title }}</h1>{% if ok %}{# comment #}{% endif %}"#;
        let spans = highlight_content(text, SyntaxKind::Jinja);
        assert_eq!(span_text(&spans), text);
    }

    #[test]
    fn jinja_highlight_handles_windowed_tokens() {
        let text = r#"<div class="item">{{ item.name }}</div>"#;
        let start = text.find("{{").unwrap();
        let spans = highlight_content_window(text, SyntaxKind::Jinja, start, text.len());
        assert_eq!(span_text(&spans), r#"{{ item.name }}</div>"#);
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
