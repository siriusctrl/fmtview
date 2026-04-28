use super::super::WRAP_CHECKPOINT_INTERVAL_ROWS;

#[derive(Debug, Default)]
pub(in crate::viewer) struct WrapCheckpointIndex {
    pub(in crate::viewer) checkpoints: Vec<WrapCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct WrapCheckpoint {
    pub(in crate::viewer) row: usize,
    pub(in crate::viewer) start_byte: usize,
    pub(in crate::viewer) start_char: usize,
}

impl WrapCheckpointIndex {
    pub(in crate::viewer) fn start_for(&self, row_start: usize) -> WrapCheckpoint {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.row <= row_start)
            .copied()
            .unwrap_or(WrapCheckpoint {
                row: 0,
                start_byte: 0,
                start_char: 0,
            })
    }

    pub(in crate::viewer) fn remember(&mut self, checkpoint: WrapCheckpoint) {
        if checkpoint.row == 0 || checkpoint.row % WRAP_CHECKPOINT_INTERVAL_ROWS != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&checkpoint.row, |existing| existing.row)
        {
            Ok(_) => {}
            Err(position) => self.checkpoints.insert(position, checkpoint),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct WrapRange {
    pub(in crate::viewer) start_char: usize,
    pub(in crate::viewer) end_char: usize,
    pub(in crate::viewer) start_byte: usize,
    pub(in crate::viewer) end_byte: usize,
    pub(in crate::viewer) continuation_indent: usize,
}

#[derive(Debug)]
pub(in crate::viewer) struct WrapWindow {
    pub(in crate::viewer) ranges: Vec<WrapRange>,
    pub(in crate::viewer) total_rows: Option<usize>,
}

#[cfg(test)]
pub(in crate::viewer) fn wrap_ranges(
    line: &str,
    width: usize,
    continuation_indent: usize,
    max_rows: usize,
) -> Vec<WrapRange> {
    wrap_ranges_window(line, width, continuation_indent, 0, max_rows).ranges
}

#[cfg(test)]
pub(in crate::viewer) fn wrap_ranges_window(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
) -> WrapWindow {
    wrap_ranges_window_indexed(line, width, continuation_indent, row_start, max_rows, None)
}

pub(in crate::viewer) fn wrap_ranges_window_indexed(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
    mut checkpoints: Option<&mut WrapCheckpointIndex>,
) -> WrapWindow {
    if max_rows == 0 {
        return WrapWindow {
            ranges: Vec::new(),
            total_rows: None,
        };
    }

    if line.is_empty() || width == 0 {
        return WrapWindow {
            ranges: vec![WrapRange {
                start_char: 0,
                end_char: 0,
                start_byte: 0,
                end_byte: 0,
                continuation_indent: 0,
            }],
            total_rows: Some(1),
        };
    }

    let mut ranges = Vec::new();
    let checkpoint = checkpoints
        .as_deref()
        .map(|checkpoints| checkpoints.start_for(row_start))
        .unwrap_or(WrapCheckpoint {
            row: 0,
            start_byte: 0,
            start_char: 0,
        });
    let mut start_byte = checkpoint.start_byte;
    let mut start_char = checkpoint.start_char;
    let mut row = checkpoint.row;
    let target_end = row_start.saturating_add(max_rows);
    while start_byte < line.len() {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember(WrapCheckpoint {
                row,
                start_byte,
                start_char,
            });
        }
        let continuation = row > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        if row >= row_start && row < target_end {
            ranges.push(WrapRange {
                start_char,
                end_char,
                start_byte,
                end_byte,
                continuation_indent: indent,
            });
        }
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        row = row.saturating_add(1);
        if row >= target_end && start_byte < line.len() {
            return WrapWindow {
                ranges,
                total_rows: None,
            };
        }
    }

    WrapWindow {
        ranges,
        total_rows: Some(row.max(1)),
    }
}

pub(in crate::viewer) fn next_wrap_end(
    line: &str,
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let hard_byte = start_byte.saturating_add(row_width.max(1)).min(line.len());
    if line.as_bytes()[start_byte..hard_byte].is_ascii() {
        return next_wrap_end_ascii(line.as_bytes(), start_byte, start_char, row_width);
    }

    let min_end = (row_width / 2).max(1);
    let mut consumed = 0_usize;
    let mut hard_end = None;
    let mut best_end = None;

    for (offset, ch) in line[start_byte..].char_indices() {
        if consumed >= row_width {
            break;
        }
        consumed += 1;
        let byte_end = start_byte + offset + ch.len_utf8();
        let char_end = start_char + consumed;
        hard_end = Some((byte_end, char_end));
        if consumed >= min_end && (ch.is_whitespace() || matches!(ch, ',' | '>' | '}' | ']' | ';'))
        {
            best_end = Some((byte_end, char_end));
        }
    }

    let Some(hard_end) = hard_end else {
        return (start_byte, start_char);
    };
    if hard_end.0 >= line.len() {
        return hard_end;
    }
    best_end.unwrap_or(hard_end)
}

pub(in crate::viewer) fn next_wrap_end_ascii(
    bytes: &[u8],
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let row_width = row_width.max(1);
    let hard_byte = start_byte.saturating_add(row_width).min(bytes.len());
    if hard_byte <= start_byte {
        return (start_byte, start_char);
    }
    if hard_byte >= bytes.len() {
        return (bytes.len(), start_char + (bytes.len() - start_byte));
    }

    let min_byte = start_byte + (row_width / 2).max(1).min(hard_byte - start_byte);
    for index in (min_byte..hard_byte).rev() {
        if is_ascii_wrap_boundary(bytes[index]) {
            let end_byte = index + 1;
            return (end_byte, start_char + (end_byte - start_byte));
        }
    }

    (hard_byte, start_char + (hard_byte - start_byte))
}

pub(in crate::viewer) fn is_ascii_wrap_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b',' | b'>' | b'}' | b']' | b';')
}

pub(in crate::viewer) fn continuation_indent(line: &str, width: usize) -> usize {
    if width < 8 {
        return 0;
    }

    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        + 2;
    indent.min(24).min(width / 2)
}
