use crate::formats::indent::{following_lines, max_boundary_offset};

pub(crate) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

pub(crate) fn is_paragraph_start(line: &str, previous_line: Option<&str>) -> bool {
    !line.trim().is_empty() && previous_line.is_none_or(|previous| previous.trim().is_empty())
}
