use std::ops::Range;

use ratatui::style::{Modifier, Style};

use crate::tui::palette::{PALETTE_CYAN, PALETTE_PURPLE, PALETTE_YELLOW, style_fg};

const CHAT_ROLE_LOOKAHEAD_LINES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub(crate) fn compact_label(self) -> &'static str {
        match self {
            ChatRole::System => "S",
            ChatRole::User => "U",
            ChatRole::Assistant => "A",
        }
    }

    pub(crate) fn style(self) -> Style {
        match self {
            ChatRole::System => style_fg(PALETTE_PURPLE),
            ChatRole::User => style_fg(PALETTE_CYAN),
            ChatRole::Assistant => style_fg(PALETTE_YELLOW),
        }
        .add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn parse_role(value: &str) -> Option<ChatRole> {
    match value {
        "system" => Some(ChatRole::System),
        "user" => Some(ChatRole::User),
        "assistant" => Some(ChatRole::Assistant),
        _ => None,
    }
}

pub(crate) fn role_value_ranges(line: &str) -> Vec<(Range<usize>, ChatRole)> {
    let mut ranges = Vec::new();
    let mut cursor = 0;

    while let Some(relative) = line[cursor..].find('"') {
        let key_start = cursor + relative;
        let Some(key_end) = string_end(line, key_start, line.len()) else {
            break;
        };

        if string_content(line, key_start, key_end) == Some("role")
            && let Some(value_start) = value_string_start(line, key_end)
            && let Some(value_end) = string_end(line, value_start, line.len())
            && let Some(role) = string_content(line, value_start, value_end).and_then(parse_role)
        {
            ranges.push((value_start..value_end, role));
            cursor = value_end;
            continue;
        }

        cursor = key_end;
    }

    ranges
}

pub(crate) fn line_declares_chat_role(line: &str) -> Option<ChatRole> {
    role_value_ranges(line)
        .into_iter()
        .map(|(_, role)| role)
        .next()
}

pub(crate) fn object_direct_chat_role(lines: &[String], start_offset: usize) -> Option<ChatRole> {
    let start_line = lines.get(start_offset)?;
    let start_byte = first_object_open_byte(start_line)?;

    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, line) in lines
        .iter()
        .enumerate()
        .skip(start_offset)
        .take(CHAT_ROLE_LOOKAHEAD_LINES)
    {
        if (offset == start_offset || depth == 1)
            && let Some(role) = line_declares_chat_role(line)
        {
            return Some(role);
        }

        let scan_start = if offset == start_offset {
            start_byte
        } else {
            0
        };
        for ch in line[scan_start..].chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' | '[' => depth = depth.saturating_add(1),
                '}' | ']' if depth > 0 => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return None;
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn value_string_start(line: &str, key_end: usize) -> Option<usize> {
    let mut cursor = skip_ws(line, key_end);
    if !line[cursor..].starts_with(':') {
        return None;
    }
    cursor += ':'.len_utf8();
    cursor = skip_ws(line, cursor);
    line[cursor..].starts_with('"').then_some(cursor)
}

fn skip_ws(line: &str, mut cursor: usize) -> usize {
    while cursor < line.len() {
        let Some(ch) = line[cursor..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn string_content(line: &str, start: usize, end: usize) -> Option<&str> {
    let content = line.get(start + '"'.len_utf8()..end.checked_sub('"'.len_utf8())?)?;
    (!content.contains('\\')).then_some(content)
}

fn string_end(line: &str, start: usize, limit: usize) -> Option<usize> {
    if start >= line.len() || !line[start..].starts_with('"') {
        return None;
    }

    let limit = limit.min(line.len());
    let mut escaped = false;
    let mut index = start + '"'.len_utf8();
    while index < limit {
        let ch = line[index..limit].chars().next()?;
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(index + ch.len_utf8());
        }
        index += ch.len_utf8();
    }

    None
}

fn first_object_open_byte(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => return Some(index),
            '[' => return None,
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_value_ranges_only_matches_supported_role_values() {
        let ranges = role_value_ranges(r#""role": "assistant", "other": "user""#);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].0, 8..19);
        assert_eq!(ranges[0].1, ChatRole::Assistant);

        assert!(role_value_ranges(r#""role": "tool""#).is_empty());
        assert!(role_value_ranges(r#""not_role": "assistant""#).is_empty());
    }

    #[test]
    fn direct_chat_role_ignores_nested_descendants() {
        let root = vec![
            "{".to_owned(),
            r#"  "messages": ["#.to_owned(),
            "    {".to_owned(),
            r#"      "role": "user""#.to_owned(),
            "    }".to_owned(),
            "  ]".to_owned(),
            "}".to_owned(),
        ];
        assert_eq!(object_direct_chat_role(&root, 0), None);
        assert_eq!(object_direct_chat_role(&root, 2), Some(ChatRole::User));

        let nested = vec![
            r#"  "message": {"#.to_owned(),
            r#"    "role": "assistant""#.to_owned(),
            "  }".to_owned(),
        ];
        assert_eq!(
            object_direct_chat_role(&nested, 0),
            Some(ChatRole::Assistant)
        );
    }

    #[test]
    fn direct_chat_role_detection_is_bounded() {
        let mut lines = vec!["{".to_owned()];
        for index in 0..CHAT_ROLE_LOOKAHEAD_LINES {
            lines.push(format!(r#"  "field_{index}": true,"#));
        }
        lines.push(r#"  "role": "user""#.to_owned());
        lines.push("}".to_owned());

        assert_eq!(object_direct_chat_role(&lines, 0), None);
    }
}
