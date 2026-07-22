use super::{DiffChange, DiffIntensity, DiffRange, UnifiedDiffRow};

pub(super) const INLINE_DIFF_PAIR_BUDGET: usize = 2_048;
const INLINE_DIFF_MAX_BYTES: usize = 8 * 1024;

pub(super) fn annotate_change_rows(rows: &mut [UnifiedDiffRow]) {
    let mut index = 0;
    let mut inline_budget = INLINE_DIFF_PAIR_BUDGET;
    while index < rows.len() {
        if !matches!(
            rows[index],
            UnifiedDiffRow::Delete { .. } | UnifiedDiffRow::Insert { .. }
        ) {
            index += 1;
            continue;
        }

        let start = index;
        while index < rows.len()
            && matches!(
                rows[index],
                UnifiedDiffRow::Delete { .. } | UnifiedDiffRow::Insert { .. }
            )
        {
            index += 1;
        }
        annotate_change_block(rows, start, index, &mut inline_budget);
    }
}

fn annotate_change_block(
    rows: &mut [UnifiedDiffRow],
    start: usize,
    end: usize,
    inline_budget: &mut usize,
) {
    let left = (start..end)
        .filter(|index| matches!(rows[*index], UnifiedDiffRow::Delete { .. }))
        .collect::<Vec<_>>();
    let right = (start..end)
        .filter(|index| matches!(rows[*index], UnifiedDiffRow::Insert { .. }))
        .collect::<Vec<_>>();
    let count = left.len().max(right.len());

    for pair_index in 0..count {
        let left_index = left.get(pair_index).copied();
        let right_index = right.get(pair_index).copied();
        let left_content = left_index.and_then(|index| row_content(&rows[index]));
        let right_content = right_index.and_then(|index| row_content(&rows[index]));
        let change = if *inline_budget == 0 || change_too_large(left_content, right_content) {
            line_only_change(left_content, right_content)
        } else {
            *inline_budget = inline_budget.saturating_sub(1);
            diff_change(left_content, right_content)
        };
        if let Some(index) = left_index {
            set_row_change(&mut rows[index], change);
        }
        if let Some(index) = right_index {
            set_row_change(&mut rows[index], change);
        }
    }
}

fn change_too_large(left: Option<&str>, right: Option<&str>) -> bool {
    left.map(str::len).unwrap_or(0) > INLINE_DIFF_MAX_BYTES
        || right.map(str::len).unwrap_or(0) > INLINE_DIFF_MAX_BYTES
}

fn line_only_change(left: Option<&str>, right: Option<&str>) -> DiffChange {
    let left_len = left.map(str::len).unwrap_or(0);
    let right_len = right.map(str::len).unwrap_or(0);
    let intensity = match (left, right) {
        (Some(_), Some(_)) if left_len.abs_diff(right_len) <= left_len.max(right_len) / 5 => {
            DiffIntensity::Medium
        }
        _ => DiffIntensity::High,
    };
    DiffChange::new(intensity, None, None)
}

fn row_content(row: &UnifiedDiffRow) -> Option<&str> {
    match row {
        UnifiedDiffRow::Delete { content, .. } | UnifiedDiffRow::Insert { content, .. } => {
            Some(content.as_ref())
        }
        UnifiedDiffRow::Context { .. } | UnifiedDiffRow::Message { .. } => None,
    }
}

fn set_row_change(row: &mut UnifiedDiffRow, change: DiffChange) {
    match row {
        UnifiedDiffRow::Delete { change: target, .. }
        | UnifiedDiffRow::Insert { change: target, .. } => *target = change,
        UnifiedDiffRow::Context { .. } | UnifiedDiffRow::Message { .. } => {}
    }
}

fn diff_change(left: Option<&str>, right: Option<&str>) -> DiffChange {
    let left_len = left.map(char_len).unwrap_or(0);
    let right_len = right.map(char_len).unwrap_or(0);
    let max_len = left_len.max(right_len).max(1);

    let (left_range, right_range) = match (left, right) {
        (Some(left), Some(right)) => {
            let prefix = common_prefix_chars(left, right);
            let suffix = common_suffix_chars(left, right, prefix);
            (
                range_from_shared_edges(left_len, prefix, suffix),
                range_from_shared_edges(right_len, prefix, suffix),
            )
        }
        (Some(_), None) => (Some(DiffRange::full(left_len)), None),
        (None, Some(_)) => (None, Some(DiffRange::full(right_len))),
        (None, None) => (None, None),
    };
    let changed = range_len(left_range).max(range_len(right_range));
    let ratio = changed.saturating_mul(100) / max_len;

    DiffChange::new(
        match ratio {
            0..=20 => DiffIntensity::Low,
            21..=60 => DiffIntensity::Medium,
            _ => DiffIntensity::High,
        },
        left_range,
        right_range,
    )
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn common_prefix_chars(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_chars(left: &str, right: &str, prefix: usize) -> usize {
    let left_len = char_len(left);
    let right_len = char_len(right);
    let max_suffix = left_len.min(right_len).saturating_sub(prefix);
    left.chars()
        .rev()
        .zip(right.chars().rev())
        .take(max_suffix)
        .take_while(|(left, right)| left == right)
        .count()
}

fn range_from_shared_edges(len: usize, prefix: usize, suffix: usize) -> Option<DiffRange> {
    let end = len.saturating_sub(suffix);
    (prefix < end).then_some(DiffRange::new(prefix, end))
}

fn range_len(range: Option<DiffRange>) -> usize {
    range
        .map(|range| range.end.saturating_sub(range.start))
        .unwrap_or(0)
}
