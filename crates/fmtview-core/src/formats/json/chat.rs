use std::ops::Range;

use ratatui::style::{Modifier, Style};

use crate::tui::palette::{PALETTE_CYAN, PALETTE_GREEN, PALETTE_PURPLE, PALETTE_YELLOW, style_fg};

pub(crate) const CHAT_ROLE_LOOKAHEAD_LINES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChatRoleMark {
    pub(crate) role: Option<ChatRole>,
    pub(crate) label: bool,
    pub(crate) guide: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChatRoleTracker {
    containers: Vec<ChatContainer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatContainer {
    kind: ChatContainerKind,
    role: Option<ChatRole>,
    start_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatContainerKind {
    Object,
    Array,
}

#[derive(Debug, Clone, Copy)]
struct RoleDiscovery {
    role: ChatRole,
    start_line: usize,
    depth: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct RankedChatRoleMark {
    mark: ChatRoleMark,
    depth: usize,
}

impl ChatRole {
    pub(crate) fn compact_label(self) -> &'static str {
        match self {
            ChatRole::System => "S",
            ChatRole::User => "U",
            ChatRole::Assistant => "A",
            ChatRole::Tool => "T",
        }
    }

    pub(crate) fn style(self) -> Style {
        match self {
            ChatRole::System => style_fg(PALETTE_PURPLE),
            ChatRole::User => style_fg(PALETTE_CYAN),
            ChatRole::Assistant => style_fg(PALETTE_YELLOW),
            ChatRole::Tool => style_fg(PALETTE_GREEN),
        }
        .add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn parse_role(value: &str) -> Option<ChatRole> {
    match value {
        "system" => Some(ChatRole::System),
        "user" => Some(ChatRole::User),
        "assistant" => Some(ChatRole::Assistant),
        "tool" => Some(ChatRole::Tool),
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

pub(crate) fn object_direct_chat_role(lines: &[String], start_offset: usize) -> Option<ChatRole> {
    let start_line = lines.get(start_offset)?;
    let start_byte = first_object_open_byte(start_line)?;

    let mut depth = 0_usize;

    for (offset, line) in lines
        .iter()
        .enumerate()
        .skip(start_offset)
        .take(CHAT_ROLE_LOOKAHEAD_LINES)
    {
        let mut cursor = if offset == start_offset {
            start_byte
        } else {
            0
        };
        while cursor < line.len() {
            let ch = line[cursor..].chars().next()?;
            match ch {
                '"' => {
                    let end = string_end(line, cursor, line.len())?;
                    if depth == 1
                        && let Some((role, _value_end)) = role_property_at(line, cursor, end)
                    {
                        return Some(role);
                    }
                    cursor = end;
                    continue;
                }
                '{' | '[' => {
                    depth = depth.saturating_add(1);
                }
                '}' | ']' if depth > 0 => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return None;
                    }
                }
                _ => {}
            }
            cursor += ch.len_utf8();
        }
    }

    None
}

impl ChatRoleTracker {
    pub(crate) fn apply_line(&mut self, line: &str, line_number: usize) {
        self.scan_line(line, line_number);
    }

    #[cfg(test)]
    pub(crate) fn mark_lines(&mut self, lines: &[String], first_line: usize) -> Vec<ChatRoleMark> {
        self.mark_lines_with_lookahead(lines, &[], first_line)
    }

    pub(crate) fn mark_lines_with_lookahead(
        &mut self,
        visible_lines: &[String],
        lookahead_lines: &[String],
        first_line: usize,
    ) -> Vec<ChatRoleMark> {
        let mut marks = vec![RankedChatRoleMark::default(); visible_lines.len()];

        for (offset, line) in visible_lines.iter().chain(lookahead_lines).enumerate() {
            let line_number = first_line.saturating_add(offset);
            let role_before = self.active_role();
            if offset < visible_lines.len()
                && let Some((role, start_line, depth)) = role_before
            {
                marks[offset] = RankedChatRoleMark {
                    mark: ChatRoleMark {
                        role: Some(role),
                        label: line_number == start_line,
                        guide: line_number != start_line,
                    },
                    depth,
                };
            }

            let discoveries = self.scan_line(line, line_number);
            let role_after = self.active_role();
            if offset < visible_lines.len()
                && let Some((_, _, depth)) = role_before
                && role_identity(role_before) != role_identity(role_after)
                && marks[offset].depth == depth
            {
                marks[offset].mark.guide = false;
            }

            for discovery in discoveries {
                let start = discovery.start_line.max(first_line) - first_line;
                for (relative, mark) in marks
                    .iter_mut()
                    .enumerate()
                    .take(offset.saturating_add(1).min(visible_lines.len()))
                    .skip(start)
                {
                    if discovery.depth < mark.depth {
                        continue;
                    }
                    mark.mark = ChatRoleMark {
                        role: Some(discovery.role),
                        label: first_line.saturating_add(relative) == discovery.start_line,
                        guide: first_line.saturating_add(relative) != discovery.start_line,
                    };
                    mark.depth = discovery.depth;
                }
            }
        }

        marks.into_iter().map(|mark| mark.mark).collect()
    }

    fn active_role(&self) -> Option<(ChatRole, usize, usize)> {
        self.containers
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, container)| {
                container
                    .role
                    .map(|role| (role, container.start_line, index + 1))
            })
    }

    fn scan_line(&mut self, line: &str, line_number: usize) -> Vec<RoleDiscovery> {
        if starts_new_root(line) && !self.containers.is_empty() {
            self.containers.clear();
        }

        let mut discoveries = Vec::new();
        let mut cursor = 0;
        while cursor < line.len() {
            let Some(ch) = line[cursor..].chars().next() else {
                break;
            };
            match ch {
                '"' => {
                    let Some(end) = string_end(line, cursor, line.len()) else {
                        break;
                    };
                    if self
                        .containers
                        .last()
                        .is_some_and(|container| container.kind == ChatContainerKind::Object)
                        && let Some((role, value_end)) = role_property_at(line, cursor, end)
                    {
                        let depth = self.containers.len();
                        if let Some(container) = self.containers.last_mut() {
                            container.role = Some(role);
                            discoveries.push(RoleDiscovery {
                                role,
                                start_line: container.start_line,
                                depth,
                            });
                        }
                        cursor = value_end;
                        continue;
                    }
                    cursor = end;
                    continue;
                }
                '{' => self.containers.push(ChatContainer {
                    kind: ChatContainerKind::Object,
                    role: None,
                    start_line: line_number,
                }),
                '[' => self.containers.push(ChatContainer {
                    kind: ChatContainerKind::Array,
                    role: None,
                    start_line: line_number,
                }),
                '}' => self.pop_container(ChatContainerKind::Object),
                ']' => self.pop_container(ChatContainerKind::Array),
                _ => {}
            }
            cursor += ch.len_utf8();
        }
        discoveries
    }

    fn pop_container(&mut self, expected: ChatContainerKind) {
        while let Some(container) = self.containers.pop() {
            if container.kind == expected {
                break;
            }
        }
    }
}

fn role_identity(role: Option<(ChatRole, usize, usize)>) -> Option<(usize, usize)> {
    role.map(|(_, start_line, depth)| (start_line, depth))
}

fn starts_new_root(line: &str) -> bool {
    line.as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'{' | b'['))
}

fn role_property_at(line: &str, key_start: usize, key_end: usize) -> Option<(ChatRole, usize)> {
    if string_content(line, key_start, key_end) != Some("role") {
        return None;
    }
    let value_start = value_string_start(line, key_end)?;
    let value_end = string_end(line, value_start, line.len())?;
    let role = string_content(line, value_start, value_end).and_then(parse_role)?;
    Some((role, value_end))
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

        let tool = role_value_ranges(r#""role": "tool""#);
        assert_eq!(tool.len(), 1);
        assert_eq!(tool[0].1, ChatRole::Tool);
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
    fn direct_chat_role_uses_container_depth_on_the_same_line() {
        let nested_first = vec![r#"{"child":{"role":"user"},"role":"assistant"}"#.to_owned()];
        assert_eq!(
            object_direct_chat_role(&nested_first, 0),
            Some(ChatRole::Assistant)
        );

        let following_sibling = vec![r#"{"content":"unlabeled"}, {"role":"system"}"#.to_owned()];
        assert_eq!(object_direct_chat_role(&following_sibling, 0), None);

        let tool = vec![r#"{"role":"tool","content":{"result":true}}"#.to_owned()];
        assert_eq!(object_direct_chat_role(&tool, 0), Some(ChatRole::Tool));
    }

    #[test]
    fn role_marks_keep_mixed_and_unlabeled_objects_separate() {
        let lines = [
            "[",
            "  {",
            r#"    "role": "user","#,
            r#"    "content": {"#,
            r#"      "text": "hello""#,
            "    }",
            "  },",
            "  {",
            r#"    "content": "no role""#,
            "  },",
            "  {",
            r#"    "role": "system","#,
            r#"    "content": "policy""#,
            "  }",
            "]",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let mut tracker = ChatRoleTracker::default();

        let marks = tracker.mark_lines(&lines, 0);

        assert_eq!(
            marks[1],
            ChatRoleMark {
                role: Some(ChatRole::User),
                label: true,
                guide: false,
            }
        );
        assert!(marks[2].guide);
        assert_eq!(marks[5].role, Some(ChatRole::User));
        assert!(marks[5].guide);
        assert!(!marks[6].guide);
        assert_eq!(marks[7], ChatRoleMark::default());
        assert_eq!(marks[8], ChatRoleMark::default());
        assert_eq!(
            marks[10],
            ChatRoleMark {
                role: Some(ChatRole::System),
                label: true,
                guide: false,
            }
        );
        assert_eq!(marks[13].role, Some(ChatRole::System));
        assert!(!marks[13].guide);
    }

    #[test]
    fn nested_role_mark_wins_when_outer_role_appears_later() {
        let lines = [
            "{",
            r#"  "child": {"#,
            r#"    "role": "assistant","#,
            r#"    "content": "nested""#,
            "  },",
            r#"  "role": "system""#,
            "}",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let mut tracker = ChatRoleTracker::default();

        let marks = tracker.mark_lines(&lines, 0);

        assert_eq!(marks[0].role, Some(ChatRole::System));
        assert_eq!(marks[1].role, Some(ChatRole::Assistant));
        assert_eq!(marks[3].role, Some(ChatRole::Assistant));
        assert_eq!(marks[5].role, Some(ChatRole::System));
    }

    #[test]
    fn lookahead_can_label_an_object_that_starts_in_the_visible_window() {
        let visible = vec!["{".to_owned(), r#"  "content": {"#.to_owned()];
        let lookahead = vec![
            r#"    "text": "body""#.to_owned(),
            "  },".to_owned(),
            r#"  "role": "assistant""#.to_owned(),
            "}".to_owned(),
        ];
        let mut tracker = ChatRoleTracker::default();

        let marks = tracker.mark_lines_with_lookahead(&visible, &lookahead, 0);

        assert_eq!(
            marks[0],
            ChatRoleMark {
                role: Some(ChatRole::Assistant),
                label: true,
                guide: false,
            }
        );
        assert_eq!(marks[1].role, Some(ChatRole::Assistant));
        assert!(marks[1].guide);
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
