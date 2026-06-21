use crate::formats::indent::{following_lines, max_boundary_offset};

pub(crate) fn block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_level = heading_level(lines.get(start_offset)?)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if heading_level(line).is_some_and(|level| level <= start_level) {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

pub(crate) fn is_heading(line: &str) -> bool {
    heading_level(line).is_some()
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    ((1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace))
    .then_some(hashes)
}
