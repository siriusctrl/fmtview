use std::sync::Arc;

use super::reader::FormattedRecord;
use crate::diff::{DiffChange, DiffModel, UnifiedDiffRow};

#[derive(Clone)]
pub(super) struct FormattedContextRecord {
    pub(super) left_start: usize,
    pub(super) right_start: usize,
    pub(super) lines: Vec<String>,
}

pub(super) fn append_context_record(
    rows: &mut Vec<UnifiedDiffRow>,
    record: FormattedContextRecord,
) {
    for (offset, line) in record.lines.into_iter().enumerate() {
        rows.push(UnifiedDiffRow::Context {
            left: record.left_start + offset,
            right: record.right_start + offset,
            content: Arc::from(line),
        });
    }
}

pub(super) fn append_context_records(
    rows: &mut Vec<UnifiedDiffRow>,
    records: impl IntoIterator<Item = FormattedContextRecord>,
) {
    for record in records {
        append_context_record(rows, record);
    }
}

pub(super) fn append_omitted_context(rows: &mut Vec<UnifiedDiffRow>, count: usize) {
    if count == 0 {
        return;
    }

    rows.push(UnifiedDiffRow::Message {
        text: format!("... {count} unchanged records omitted ..."),
    });
}

pub(super) fn scanning_model(
    left_label: &str,
    right_label: &str,
    records_read: usize,
) -> DiffModel {
    DiffModel::from_rows(
        left_label.to_owned(),
        right_label.to_owned(),
        vec![UnifiedDiffRow::Message {
            text: scanning_message(records_read),
        }],
    )
}

pub(super) fn scanning_message(records_read: usize) -> String {
    if records_read == 0 {
        "Scanning record diff...".to_owned()
    } else {
        format!("Scanning record diff... {records_read} records")
    }
}

pub(super) fn find_sync_record(
    left: &[FormattedRecord],
    right: &[FormattedRecord],
) -> Option<(usize, usize)> {
    let mut best = None;
    for (left_index, left_record) in left.iter().enumerate() {
        for (right_index, right_record) in right.iter().enumerate() {
            if left_index == 0 && right_index == 0 {
                continue;
            }
            if left_record != right_record {
                continue;
            }
            let distance = left_index + right_index;
            let replace = best
                .map(|(best_left, best_right)| distance < best_left + best_right)
                .unwrap_or(true);
            if replace {
                best = Some((left_index, right_index));
            }
        }
    }
    best
}

pub(super) fn full_line_change<'a>(lines: impl Iterator<Item = &'a str>) -> DiffChange {
    let end = lines.map(|line| line.chars().count()).max().unwrap_or(0);
    DiffChange::full_line(end)
}
