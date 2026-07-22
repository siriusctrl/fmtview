use ratatui::text::{Line, Span};

use crate::tui::{
    palette::{gutter_style, key_style},
    text::char_count,
};
use crate::{formats::json::path::JsonPathStack, load::ViewFile};

const CHECKPOINT_INTERVAL: usize = 512;
const SCAN_CHUNK_LINES: usize = 512;
const MAX_BREADCRUMB_ROWS: usize = 2;
const MIN_BREADCRUMB_WIDTH: usize = 16;

#[derive(Debug, Default)]
pub(in crate::viewer) struct JsonBreadcrumbCache {
    checkpoints: Vec<PathCheckpoint>,
}

#[derive(Debug, Clone)]
struct PathCheckpoint {
    line: usize,
    stack: JsonPathStack,
}

impl JsonBreadcrumbCache {
    pub(in crate::viewer) fn render(
        &mut self,
        file: &dyn ViewFile,
        line: usize,
        width: usize,
        indent_width: usize,
        available_rows: usize,
    ) -> Vec<Line<'static>> {
        let content_width = width.saturating_sub(indent_width);
        if content_width < MIN_BREADCRUMB_WIDTH || available_rows < 5 {
            return Vec::new();
        }

        let path = self.path_for_line(file, line);
        if path.is_empty() {
            return Vec::new();
        }

        indent_lines(
            breadcrumb_lines(
                &path,
                content_width,
                MAX_BREADCRUMB_ROWS.min(available_rows.saturating_sub(3)),
            ),
            indent_width,
        )
    }

    fn path_for_line(&mut self, file: &dyn ViewFile, line: usize) -> Vec<String> {
        if file.line_count() == 0 {
            return Vec::new();
        }

        let line = line.min(file.line_count().saturating_sub(1));
        let checkpoint = self.checkpoint_before(line);
        let mut stack = checkpoint.stack;
        let mut next_line = checkpoint.line;

        while next_line < line {
            let count = (line - next_line).min(SCAN_CHUNK_LINES);
            let Ok(lines) = file.read_window(next_line, count) else {
                return Vec::new();
            };
            if lines.is_empty() {
                break;
            }

            for text in &lines {
                stack.apply_line(text);
                next_line = next_line.saturating_add(1);
                self.remember_checkpoint(next_line, &stack);
                if next_line >= line {
                    break;
                }
            }
        }

        let Ok(target) = file.read_window(line, 1) else {
            return Vec::new();
        };
        let Some(target) = target.first() else {
            return Vec::new();
        };
        stack.current_path_for_line(target)
    }

    fn checkpoint_before(&self, line: usize) -> PathCheckpoint {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.line <= line)
            .cloned()
            .unwrap_or_else(|| PathCheckpoint {
                line: 0,
                stack: JsonPathStack::default(),
            })
    }

    fn remember_checkpoint(&mut self, line: usize, stack: &JsonPathStack) {
        if line == 0 || line % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&line, |checkpoint| checkpoint.line)
        {
            Ok(index) => self.checkpoints[index].stack = stack.clone(),
            Err(index) => self.checkpoints.insert(
                index,
                PathCheckpoint {
                    line,
                    stack: stack.clone(),
                },
            ),
        }
    }
}

fn indent_lines(lines: Vec<Line<'static>>, indent_width: usize) -> Vec<Line<'static>> {
    if indent_width == 0 {
        return lines;
    }

    let indent = " ".repeat(indent_width);
    lines
        .into_iter()
        .map(|mut line| {
            line.spans
                .insert(0, Span::styled(indent.clone(), gutter_style()));
            line
        })
        .collect()
}

fn breadcrumb_lines(path: &[String], width: usize, max_rows: usize) -> Vec<Line<'static>> {
    if max_rows == 0 {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut used = 0_usize;
    let visible_path = fit_path_tail(path, width, max_rows);

    for (index, key) in visible_path.iter().enumerate() {
        let key_width = char_count(key);
        let separator_width = usize::from(index > 0) * 3;
        if !current.is_empty() && used + separator_width + key_width > width {
            rows.push(Line::from(current));
            current = Vec::new();
            used = 0;
            if rows.len() >= max_rows {
                return rows;
            }
        }

        if !current.is_empty() {
            push_separator(&mut current);
            used += 3;
        }
        let available = width.saturating_sub(used);
        let text = truncate_key(key, available);
        used += char_count(&text);
        current.push(Span::styled(text, key_style()));
    }

    if !current.is_empty() && rows.len() < max_rows {
        rows.push(Line::from(current));
    }
    rows
}

fn fit_path_tail(path: &[String], width: usize, max_rows: usize) -> Vec<String> {
    let budget = width.saturating_mul(max_rows);
    let mut selected = Vec::new();
    let mut used = 0_usize;

    for key in path.iter().rev() {
        let key_width = char_count(key);
        let next_width = key_width + if selected.is_empty() { 0 } else { 3 };
        if !selected.is_empty() && used + next_width > budget.saturating_sub(2) {
            selected.push("...".to_owned());
            break;
        }
        selected.push(key.clone());
        used += next_width;
    }

    selected.reverse();
    selected
}

fn push_separator(spans: &mut Vec<Span<'static>>) {
    spans.push(Span::styled(" ", gutter_style()));
    spans.push(Span::styled("›", gutter_style()));
    spans.push(Span::styled(" ", gutter_style()));
}

fn truncate_key(key: &str, width: usize) -> String {
    if char_count(key) <= width {
        return key.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }

    let take = width.saturating_sub(1);
    let mut output = key.chars().take(take).collect::<String>();
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    struct TestFile {
        lines: Vec<String>,
    }

    impl ViewFile for TestFile {
        fn label(&self) -> &str {
            "test"
        }

        fn line_count(&self) -> usize {
            self.lines.len()
        }

        fn line_count_exact(&self) -> bool {
            true
        }

        fn byte_len(&self) -> u64 {
            self.lines.iter().map(|line| line.len() as u64 + 1).sum()
        }

        fn byte_offset_for_line(&self, _line: usize) -> u64 {
            0
        }

        fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
            Ok(self.lines.iter().skip(start).take(count).cloned().collect())
        }
    }

    #[test]
    fn breadcrumb_tracks_nested_keys() {
        let file = TestFile {
            lines: [
                "{",
                "  \"outer\": {",
                "    \"inner\": {",
                "      \"value\": 1",
                "    }",
                "  }",
                "}",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        };
        let mut cache = JsonBreadcrumbCache::default();
        let lines = cache.render(&file, 3, 80, 0, 12);
        let text = span_text(&lines[0]);

        assert_eq!(text, "outer › inner › value");
    }

    #[test]
    fn breadcrumb_keeps_array_parent_key() {
        let file = TestFile {
            lines: [
                "{",
                "  \"items\": [",
                "    {",
                "      \"name\": \"a\"",
                "    },",
                "    {",
                "      \"name\": \"b\"",
                "    }",
                "  ]",
                "}",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        };
        let mut cache = JsonBreadcrumbCache::default();
        let lines = cache.render(&file, 6, 80, 0, 12);

        assert_eq!(span_text(&lines[0]), "items › name");
    }

    #[test]
    fn breadcrumb_offsets_to_content_column() {
        let file = TestFile {
            lines: [
                "{",
                "  \"outer\": {",
                "    \"inner\": {",
                "      \"value\": 1",
                "    }",
                "  }",
                "}",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        };
        let mut cache = JsonBreadcrumbCache::default();
        let lines = cache.render(&file, 3, 80, 5, 12);

        assert_eq!(span_text(&lines[0]), "     outer › inner › value");
    }

    #[test]
    fn breadcrumb_wraps_to_limited_rows() {
        let path = vec![
            "alpha".to_owned(),
            "beta".to_owned(),
            "gamma".to_owned(),
            "delta".to_owned(),
        ];
        let lines = breadcrumb_lines(&path, 16, 2);

        assert_eq!(lines.len(), 2);
    }

    fn span_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
