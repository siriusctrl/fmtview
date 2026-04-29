use ratatui::text::{Line, Span};

use crate::load::ViewFile;

use super::{
    palette::{gutter_style, key_style},
    render::char_count,
};

const CHECKPOINT_INTERVAL: usize = 512;
const SCAN_CHUNK_LINES: usize = 512;
const MAX_BREADCRUMB_ROWS: usize = 2;
const MIN_BREADCRUMB_WIDTH: usize = 16;

#[derive(Debug, Default)]
pub(super) struct JsonBreadcrumbCache {
    checkpoints: Vec<PathCheckpoint>,
}

#[derive(Debug, Clone)]
struct PathCheckpoint {
    line: usize,
    stack: Vec<PathEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathEntry {
    indent: usize,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonLineKind {
    Key { indent: usize, key: String },
    Closing { indent: usize },
    Other,
}

impl JsonBreadcrumbCache {
    pub(super) fn render(
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
                apply_line_to_stack(&mut stack, classify_json_line(text));
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
        path_for_current_line(&stack, classify_json_line(target))
    }

    fn checkpoint_before(&self, line: usize) -> PathCheckpoint {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.line <= line)
            .cloned()
            .unwrap_or_else(|| PathCheckpoint {
                line: 0,
                stack: Vec::new(),
            })
    }

    fn remember_checkpoint(&mut self, line: usize, stack: &[PathEntry]) {
        if line == 0 || line % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&line, |checkpoint| checkpoint.line)
        {
            Ok(index) => self.checkpoints[index].stack = stack.to_vec(),
            Err(index) => self.checkpoints.insert(
                index,
                PathCheckpoint {
                    line,
                    stack: stack.to_vec(),
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

fn path_for_current_line(stack: &[PathEntry], line: JsonLineKind) -> Vec<String> {
    let mut stack = stack.to_vec();
    match line {
        JsonLineKind::Key { indent, key } => {
            pop_to_parent(&mut stack, indent);
            stack.push(PathEntry { indent, key });
        }
        JsonLineKind::Closing { .. } | JsonLineKind::Other => {}
    }

    stack.into_iter().map(|entry| entry.key).collect()
}

fn apply_line_to_stack(stack: &mut Vec<PathEntry>, line: JsonLineKind) {
    match line {
        JsonLineKind::Key { indent, key } => {
            pop_to_parent(stack, indent);
            stack.push(PathEntry { indent, key });
        }
        JsonLineKind::Closing { indent } => {
            pop_to_parent(stack, indent);
        }
        JsonLineKind::Other => {}
    }
}

fn pop_to_parent(stack: &mut Vec<PathEntry>, indent: usize) {
    while stack.last().is_some_and(|entry| entry.indent >= indent) {
        stack.pop();
    }
}

fn classify_json_line(line: &str) -> JsonLineKind {
    let indent = line.len().saturating_sub(line.trim_start().len());
    let trimmed = line.trim_start();
    if trimmed.starts_with('}') || trimmed.starts_with(']') {
        return JsonLineKind::Closing { indent };
    }

    parse_json_key(trimmed)
        .map(|key| JsonLineKind::Key { indent, key })
        .unwrap_or(JsonLineKind::Other)
}

fn parse_json_key(trimmed: &str) -> Option<String> {
    if !trimmed.starts_with('"') {
        return None;
    }

    let end = json_string_end(trimmed)?;
    if !trimmed[end..].trim_start().starts_with(':') {
        return None;
    }

    Some(decode_json_string_content(&trimmed[1..end - 1]))
}

fn json_string_end(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (offset, ch) in text[1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Some(1 + offset + ch.len_utf8());
        }
    }

    None
}

fn decode_json_string_content(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut chars = content.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('"') => output.push('"'),
            Some('\\') => output.push('\\'),
            Some('/') => output.push('/'),
            Some('b') => output.push_str("\\b"),
            Some('f') => output.push_str("\\f"),
            Some('n') => output.push_str("\\n"),
            Some('r') => output.push_str("\\r"),
            Some('t') => output.push_str("\\t"),
            Some('u') => {
                output.push_str("\\u");
                for _ in 0..4 {
                    if let Some(hex) = chars.next() {
                        output.push(hex);
                    }
                }
            }
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
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
