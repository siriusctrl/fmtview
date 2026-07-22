use std::io::{self, BufRead};

mod inline;
mod parse;
mod rows;
mod side_by_side;

#[cfg(test)]
mod tests;

pub(crate) use rows::{
    DiffChange, DiffIntensity, DiffLayout, DiffRange, NumberedDiffLine, SideDiffRow, UnifiedDiffRow,
};

use inline::annotate_change_rows;
use parse::parse_unified_rows;
use side_by_side::{build_side_rows, line_number_digits};

#[derive(Debug)]
pub(crate) struct DiffModel {
    left_label: String,
    right_label: String,
    unified_rows: Vec<UnifiedDiffRow>,
    side_rows: Vec<SideDiffRow>,
    unified_changes: Vec<usize>,
    side_changes: Vec<usize>,
    left_digits: usize,
    right_digits: usize,
    has_changes: bool,
}

impl DiffModel {
    pub(crate) fn from_rows(
        left_label: String,
        right_label: String,
        mut unified_rows: Vec<UnifiedDiffRow>,
    ) -> Self {
        Self::from_unified_rows(left_label, right_label, &mut unified_rows)
    }

    #[cfg(test)]
    pub(crate) fn from_unified_patch(left_label: String, right_label: String, patch: &str) -> Self {
        let mut unified_rows = parse_unified_rows(patch.lines().map(Ok::<_, io::Error>))
            .expect("parsing string lines cannot fail");
        Self::from_unified_rows(left_label, right_label, &mut unified_rows)
    }

    pub(crate) fn from_unified_reader<R: BufRead>(
        left_label: String,
        right_label: String,
        reader: R,
    ) -> io::Result<Self> {
        let mut unified_rows = parse_unified_rows(reader.lines())?;
        Ok(Self::from_unified_rows(
            left_label,
            right_label,
            &mut unified_rows,
        ))
    }

    fn from_unified_rows(
        left_label: String,
        right_label: String,
        unified_rows: &mut Vec<UnifiedDiffRow>,
    ) -> Self {
        let has_changes = unified_rows.iter().any(|row| {
            matches!(
                row,
                UnifiedDiffRow::Delete { .. } | UnifiedDiffRow::Insert { .. }
            )
        });
        if unified_rows.is_empty() {
            unified_rows.push(UnifiedDiffRow::Message {
                text: "No differences".to_owned(),
            });
        } else {
            annotate_change_rows(unified_rows);
        }

        let side_rows = build_side_rows(unified_rows);
        let unified_changes = unified_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                matches!(
                    row,
                    UnifiedDiffRow::Delete { .. } | UnifiedDiffRow::Insert { .. }
                )
                .then_some(index)
            })
            .collect();
        let side_changes = side_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, SideDiffRow::Change { .. }).then_some(index))
            .collect();
        let (left_digits, right_digits) = line_number_digits(unified_rows);

        Self {
            left_label,
            right_label,
            unified_rows: std::mem::take(unified_rows),
            side_rows,
            unified_changes,
            side_changes,
            left_digits,
            right_digits,
            has_changes,
        }
    }

    pub(crate) fn left_label(&self) -> &str {
        &self.left_label
    }

    pub(crate) fn right_label(&self) -> &str {
        &self.right_label
    }

    pub(crate) fn unified_rows(&self) -> &[UnifiedDiffRow] {
        &self.unified_rows
    }

    pub(crate) fn side_rows(&self) -> &[SideDiffRow] {
        &self.side_rows
    }

    pub(crate) fn row_count(&self, layout: DiffLayout) -> usize {
        match layout {
            DiffLayout::Unified => self.unified_rows.len(),
            DiffLayout::SideBySide => self.side_rows.len(),
        }
    }

    pub(crate) fn changed_rows(&self, layout: DiffLayout) -> &[usize] {
        match layout {
            DiffLayout::Unified => &self.unified_changes,
            DiffLayout::SideBySide => &self.side_changes,
        }
    }

    pub(crate) fn left_digits(&self) -> usize {
        self.left_digits
    }

    pub(crate) fn right_digits(&self) -> usize {
        self.right_digits
    }

    pub(crate) fn has_changes(&self) -> bool {
        self.has_changes
    }
}
