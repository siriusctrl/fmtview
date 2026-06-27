#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct JsonPathStack {
    entries: Vec<PathEntry>,
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

impl JsonPathStack {
    pub(crate) fn apply_line(&mut self, line: &str) {
        match classify_json_line(line) {
            JsonLineKind::Key { indent, key } => {
                self.pop_to_parent(indent);
                self.entries.push(PathEntry { indent, key });
            }
            JsonLineKind::Closing { indent } => {
                self.pop_to_parent(indent);
            }
            JsonLineKind::Other => {}
        }
    }

    pub(crate) fn current_path_for_line(&self, line: &str) -> Vec<String> {
        let mut stack = self.clone();
        if let JsonLineKind::Key { indent, key } = classify_json_line(line) {
            stack.pop_to_parent(indent);
            stack.entries.push(PathEntry { indent, key });
        }

        stack.entries.into_iter().map(|entry| entry.key).collect()
    }

    fn pop_to_parent(&mut self, indent: usize) {
        while self
            .entries
            .last()
            .is_some_and(|entry| entry.indent >= indent)
        {
            self.entries.pop();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_tracks_nested_keys() {
        let mut stack = JsonPathStack::default();
        stack.apply_line("{");
        stack.apply_line(r#"  "outer": {"#);
        stack.apply_line(r#"    "inner": {"#);

        assert_eq!(
            stack.current_path_for_line(r#"      "value": 1"#),
            ["outer", "inner", "value"]
        );
    }

    #[test]
    fn path_keeps_array_parent_key() {
        let mut stack = JsonPathStack::default();
        stack.apply_line("{");
        stack.apply_line(r#"  "items": ["#);
        stack.apply_line("    {");

        assert_eq!(
            stack.current_path_for_line(r#"      "name": "a""#),
            ["items", "name"]
        );
    }

    #[test]
    fn path_decodes_simple_escaped_key_text() {
        let stack = JsonPathStack::default();

        assert_eq!(stack.current_path_for_line(r#"  "a\"b": true"#), ["a\"b"]);
    }
}
