pub(crate) fn leading_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(crate) fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}

pub(crate) fn max_observed_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    if lines.is_empty() || viewport_bottom < read_start {
        return None;
    }
    Some((viewport_bottom - read_start).min(lines.len() - 1))
}

pub(crate) fn max_boundary_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    max_observed_offset(lines, read_start, viewport_bottom.saturating_add(1))
}

pub(crate) fn following_lines(
    lines: &[String],
    start_offset: usize,
    max_offset: usize,
) -> impl Iterator<Item = (usize, &String)> {
    lines
        .iter()
        .enumerate()
        .take(max_offset + 1)
        .skip(start_offset + 1)
}

pub(crate) fn eof_block_end(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    if !line_count_exact || line_count == 0 {
        return None;
    }
    let eof_line = line_count - 1;
    let read_end = read_start.saturating_add(lines.len());
    (eof_line <= viewport_bottom && eof_line < read_end).then_some(eof_line)
}

pub(crate) fn indent_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_indent = leading_indent(lines.get(start_offset)?);
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_indent(line) <= start_indent {
            if is_same_indent_closing_line(line) {
                return Some(read_start + offset);
            }
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn is_same_indent_closing_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.starts_with("</")
        || crate::formats::jinja::structure::keyword(trimmed)
            .is_some_and(|keyword| keyword.starts_with("end"))
}
