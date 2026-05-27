use super::{following_lines, max_observed_offset};

pub(super) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let trimmed = lines.get(start_offset)?.trim_start();
    let tag = start_tag_name(trimmed)?;
    if tag_is_self_contained(trimmed, &tag) {
        return Some(read_start + start_offset);
    }

    let mut depth = 1_usize;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        let line = line.as_str();
        depth = depth.saturating_add(start_tag_count(line, &tag));
        let closing = end_tag_count(line, &tag);
        depth = depth.saturating_sub(closing);
        if closing > 0 && depth == 0 {
            return Some(read_start + offset);
        }
    }
    None
}

pub(super) fn is_start_tag(trimmed: &str) -> bool {
    if trimmed.starts_with("</")
        || trimmed.starts_with("<!")
        || trimmed.starts_with("<?")
        || trimmed == "<"
    {
        return false;
    }
    trimmed
        .as_bytes()
        .get(1)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
}

fn start_tag_name(trimmed: &str) -> Option<String> {
    if !is_start_tag(trimmed) {
        return None;
    }
    let name_end = trimmed[1..]
        .find(|ch: char| !is_name_char(ch))
        .map(|index| index + 1)
        .unwrap_or(trimmed.len());
    (name_end > 1).then(|| trimmed[1..name_end].to_owned())
}

fn tag_is_self_contained(trimmed: &str, tag: &str) -> bool {
    trimmed.contains("/>") || trimmed.contains(&format!("</{tag}>"))
}

fn start_tag_count(line: &str, tag: &str) -> usize {
    let mut count = 0_usize;
    let mut rest = line;
    let needle = format!("<{tag}");
    while let Some(index) = rest.find(&needle) {
        let after = &rest[index + needle.len()..];
        if after.chars().next().is_none_or(|ch| !is_name_char(ch))
            && !after.trim_start().starts_with("/>")
        {
            count = count.saturating_add(1);
        }
        let advance = after.chars().next().map(char::len_utf8).unwrap_or(0);
        rest = &after[advance..];
    }
    count
}

fn end_tag_count(line: &str, tag: &str) -> usize {
    line.matches(&format!("</{tag}>")).count()
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}
