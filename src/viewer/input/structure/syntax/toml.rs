use super::{following_lines, max_boundary_offset};

pub(super) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if is_table(line) {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

pub(super) fn is_table(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('[') && trimmed.contains(']') && !trimmed.starts_with("[]")
}
