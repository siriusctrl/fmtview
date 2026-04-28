use std::sync::Arc;

use super::{DiffChange, UnifiedDiffRow};

pub(super) fn parse_unified_rows<I, S, E>(lines: I) -> Result<Vec<UnifiedDiffRow>, E>
where
    I: IntoIterator<Item = Result<S, E>>,
    S: AsRef<str>,
{
    let mut rows = Vec::new();
    let mut left_line = 0_usize;
    let mut right_line = 0_usize;
    let mut in_hunk = false;

    for line in lines {
        let line = line?;
        let line = line.as_ref();
        if let Some((left_start, right_start)) = parse_hunk_start(line) {
            left_line = left_start;
            right_line = right_start;
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            continue;
        }

        let Some(marker) = line.as_bytes().first().copied() else {
            continue;
        };
        let content = Arc::<str>::from(line.get(1..).unwrap_or_default());
        match marker {
            b' ' => {
                rows.push(UnifiedDiffRow::Context {
                    left: left_line,
                    right: right_line,
                    content,
                });
                left_line = left_line.saturating_add(1);
                right_line = right_line.saturating_add(1);
            }
            b'-' => {
                rows.push(UnifiedDiffRow::Delete {
                    left: left_line,
                    content,
                    change: DiffChange::default(),
                });
                left_line = left_line.saturating_add(1);
            }
            b'+' => {
                rows.push(UnifiedDiffRow::Insert {
                    right: right_line,
                    content,
                    change: DiffChange::default(),
                });
                right_line = right_line.saturating_add(1);
            }
            b'\\' => {}
            _ => {}
        }
    }

    Ok(rows)
}

fn parse_hunk_start(line: &str) -> Option<(usize, usize)> {
    if !line.starts_with("@@ ") {
        return None;
    }

    let mut parts = line.split_whitespace();
    parts.next()?;
    let left = parse_range_start(parts.next()?)?;
    let right = parse_range_start(parts.next()?)?;
    Some((left, right))
}

fn parse_range_start(token: &str) -> Option<usize> {
    token
        .trim_start_matches(['-', '+'])
        .split(',')
        .next()?
        .parse()
        .ok()
}
