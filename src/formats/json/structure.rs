use crate::formats::{
    StructureCandidateKind,
    shared::{leading_indent, max_observed_offset},
};

pub(crate) fn candidate_kind(line: &str) -> Option<StructureCandidateKind> {
    let indent = leading_indent(line);
    let trimmed = line.trim_start();
    let first = trimmed.as_bytes().first().copied()?;
    if matches!(first, b'{' | b'[') {
        return Some(if indent == 0 {
            StructureCandidateKind::JsonRecordStart
        } else if first == b'{' {
            StructureCandidateKind::JsonArrayItemStart
        } else {
            StructureCandidateKind::JsonRootStart
        });
    }
    let after_colon = value_after_key(trimmed)?;
    if after_colon.starts_with('{') || after_colon.starts_with('[') {
        Some(StructureCandidateKind::JsonCompositeField)
    } else {
        None
    }
}

pub(crate) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let mut depth = 0_usize;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;

    for (relative, line) in lines[start_offset..=max_offset].iter().enumerate() {
        let offset = start_offset + relative;
        let start_byte = if offset == start_offset {
            first_open_byte(line)?
        } else {
            0
        };
        for (_, ch) in line[start_byte..].char_indices() {
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
                '{' | '[' => {
                    depth = depth.saturating_add(1);
                    started = true;
                }
                '}' | ']' if started => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(read_start + offset);
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn value_after_key(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('"') {
        return None;
    }

    let mut escaped = false;
    for (index, ch) in trimmed.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                return trimmed[index + ch.len_utf8()..]
                    .trim_start()
                    .strip_prefix(':')
                    .map(str::trim_start);
            }
            _ => {}
        }
    }
    None
}

fn first_open_byte(line: &str) -> Option<usize> {
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
            '{' | '[' => return Some(index),
            _ => {}
        }
    }
    None
}
