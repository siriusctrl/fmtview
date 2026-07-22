use super::{SideDiffRow, UnifiedDiffRow};

pub(super) fn build_side_rows(unified_rows: &[UnifiedDiffRow]) -> Vec<SideDiffRow> {
    let mut rows = Vec::with_capacity(unified_rows.len());
    let mut left_changes = Vec::new();
    let mut right_changes = Vec::new();

    for (index, row) in unified_rows.iter().enumerate() {
        match row {
            UnifiedDiffRow::Delete { .. } => {
                if !right_changes.is_empty() && left_changes.is_empty() {
                    flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                }
                left_changes.push(index);
            }
            UnifiedDiffRow::Insert { .. } => right_changes.push(index),
            UnifiedDiffRow::Context { .. } => {
                flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                rows.push(SideDiffRow::Context { unified: index });
            }
            UnifiedDiffRow::Message { .. } => {
                flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                rows.push(SideDiffRow::Message { unified: index });
            }
        }
    }

    flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
    rows
}

fn flush_change_rows(
    rows: &mut Vec<SideDiffRow>,
    left_changes: &mut Vec<usize>,
    right_changes: &mut Vec<usize>,
) {
    let count = left_changes.len().max(right_changes.len());
    for index in 0..count {
        rows.push(SideDiffRow::Change {
            left: left_changes.get(index).copied(),
            right: right_changes.get(index).copied(),
        });
    }
    left_changes.clear();
    right_changes.clear();
}

pub(super) fn line_number_digits(rows: &[UnifiedDiffRow]) -> (usize, usize) {
    let mut left_max = 0_usize;
    let mut right_max = 0_usize;
    for row in rows {
        match row {
            UnifiedDiffRow::Context { left, right, .. } => {
                left_max = left_max.max(*left);
                right_max = right_max.max(*right);
            }
            UnifiedDiffRow::Delete { left, .. } => {
                left_max = left_max.max(*left);
            }
            UnifiedDiffRow::Insert { right, .. } => {
                right_max = right_max.max(*right);
            }
            UnifiedDiffRow::Message { .. } => {}
        }
    }

    (digits(left_max), digits(right_max))
}

fn digits(value: usize) -> usize {
    value.max(1).to_string().len()
}
