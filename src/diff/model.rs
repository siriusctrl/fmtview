#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLayout {
    Unified,
    SideBySide,
}

impl DiffLayout {
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Unified => Self::SideBySide,
            Self::SideBySide => Self::Unified,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unified => "single",
            Self::SideBySide => "side-by-side",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NumberedDiffLine {
    pub(crate) number: usize,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangedDiffLine {
    pub(crate) line: NumberedDiffLine,
    pub(crate) change: DiffChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffChange {
    pub(crate) intensity: DiffIntensity,
    pub(crate) left_range: Option<DiffRange>,
    pub(crate) right_range: Option<DiffRange>,
}

impl Default for DiffChange {
    fn default() -> Self {
        Self {
            intensity: DiffIntensity::Low,
            left_range: None,
            right_range: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DiffIntensity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifiedDiffRow {
    Context {
        left: usize,
        right: usize,
        content: String,
    },
    Delete {
        left: usize,
        content: String,
        change: DiffChange,
    },
    Insert {
        right: usize,
        content: String,
        change: DiffChange,
    },
    Message {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SideDiffRow {
    Context {
        left: usize,
        right: usize,
        content: String,
    },
    Change {
        left: Option<ChangedDiffLine>,
        right: Option<ChangedDiffLine>,
    },
    Message {
        text: String,
    },
}

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
    pub(crate) fn from_unified_patch(left_label: String, right_label: String, patch: &str) -> Self {
        let mut unified_rows = parse_unified_rows(patch);
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
            annotate_change_rows(&mut unified_rows);
        }

        let side_rows = build_side_rows(&unified_rows);
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
        let (left_digits, right_digits) = line_number_digits(&unified_rows);

        Self {
            left_label,
            right_label,
            unified_rows,
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

fn parse_unified_rows(patch: &str) -> Vec<UnifiedDiffRow> {
    let mut rows = Vec::new();
    let mut left_line = 0_usize;
    let mut right_line = 0_usize;
    let mut in_hunk = false;

    for line in patch.lines() {
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
        let content = line.get(1..).unwrap_or_default().to_owned();
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

    rows
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

fn annotate_change_rows(rows: &mut [UnifiedDiffRow]) {
    let mut index = 0;
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
        annotate_change_block(rows, start, index);
    }
}

fn annotate_change_block(rows: &mut [UnifiedDiffRow], start: usize, end: usize) {
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
        let change = diff_change(left_content, right_content);
        if let Some(index) = left_index {
            set_row_change(&mut rows[index], change);
        }
        if let Some(index) = right_index {
            set_row_change(&mut rows[index], change);
        }
    }
}

fn row_content(row: &UnifiedDiffRow) -> Option<&str> {
    match row {
        UnifiedDiffRow::Delete { content, .. } | UnifiedDiffRow::Insert { content, .. } => {
            Some(content)
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
        (Some(_), None) => (
            Some(DiffRange {
                start: 0,
                end: left_len,
            }),
            None,
        ),
        (None, Some(_)) => (
            None,
            Some(DiffRange {
                start: 0,
                end: right_len,
            }),
        ),
        (None, None) => (None, None),
    };
    let changed = range_len(left_range).max(range_len(right_range));
    let ratio = changed.saturating_mul(100) / max_len;

    DiffChange {
        intensity: match ratio {
            0..=20 => DiffIntensity::Low,
            21..=60 => DiffIntensity::Medium,
            _ => DiffIntensity::High,
        },
        left_range,
        right_range,
    }
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
    (prefix < end).then_some(DiffRange { start: prefix, end })
}

fn range_len(range: Option<DiffRange>) -> usize {
    range
        .map(|range| range.end.saturating_sub(range.start))
        .unwrap_or(0)
}

fn build_side_rows(unified_rows: &[UnifiedDiffRow]) -> Vec<SideDiffRow> {
    let mut rows = Vec::with_capacity(unified_rows.len());
    let mut left_changes = Vec::new();
    let mut right_changes = Vec::new();

    for row in unified_rows {
        match row {
            UnifiedDiffRow::Delete {
                left,
                content,
                change,
            } => {
                if !right_changes.is_empty() && left_changes.is_empty() {
                    flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                }
                left_changes.push(ChangedDiffLine {
                    line: NumberedDiffLine {
                        number: *left,
                        content: content.clone(),
                    },
                    change: *change,
                });
            }
            UnifiedDiffRow::Insert {
                right,
                content,
                change,
            } => {
                right_changes.push(ChangedDiffLine {
                    line: NumberedDiffLine {
                        number: *right,
                        content: content.clone(),
                    },
                    change: *change,
                });
            }
            UnifiedDiffRow::Context {
                left,
                right,
                content,
            } => {
                flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                rows.push(SideDiffRow::Context {
                    left: *left,
                    right: *right,
                    content: content.clone(),
                });
            }
            UnifiedDiffRow::Message { text } => {
                flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
                rows.push(SideDiffRow::Message { text: text.clone() });
            }
        }
    }

    flush_change_rows(&mut rows, &mut left_changes, &mut right_changes);
    rows
}

fn flush_change_rows(
    rows: &mut Vec<SideDiffRow>,
    left_changes: &mut Vec<ChangedDiffLine>,
    right_changes: &mut Vec<ChangedDiffLine>,
) {
    let count = left_changes.len().max(right_changes.len());
    for index in 0..count {
        rows.push(SideDiffRow::Change {
            left: left_changes.get(index).cloned(),
            right: right_changes.get(index).cloned(),
        });
    }
    left_changes.clear();
    right_changes.clear();
}

fn line_number_digits(rows: &[UnifiedDiffRow]) -> (usize, usize) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{hint::black_box, time::Instant};

    #[test]
    fn parses_unified_diff_line_numbers() {
        let patch = "\
--- left
+++ right
@@ -2,3 +2,4 @@
 keep
-old
+new
+added
 tail
";
        let model = DiffModel::from_unified_patch("left".to_owned(), "right".to_owned(), patch);

        assert!(model.has_changes());
        assert_eq!(model.changed_rows(DiffLayout::Unified), &[1, 2, 3]);
        assert_eq!(model.changed_rows(DiffLayout::SideBySide), &[1, 2]);
        assert!(matches!(
            &model.side_rows()[1],
            SideDiffRow::Change {
                left: Some(ChangedDiffLine {
                    line: NumberedDiffLine { number: 3, .. },
                    ..
                }),
                right: Some(ChangedDiffLine {
                    line: NumberedDiffLine { number: 3, .. },
                    ..
                })
            }
        ));
    }

    #[test]
    fn empty_patch_becomes_equal_message() {
        let model = DiffModel::from_unified_patch("left".to_owned(), "right".to_owned(), "");

        assert!(!model.has_changes());
        assert_eq!(model.row_count(DiffLayout::Unified), 1);
        assert!(model.changed_rows(DiffLayout::Unified).is_empty());
    }

    #[test]
    #[ignore = "performance smoke; run benches/diff-performance.sh"]
    fn perf_diff_model_build() {
        let patch = generated_patch(2_048, 3);
        let started = Instant::now();
        let mut rows = 0;
        let mut changes = 0;
        for _ in 0..16 {
            let model = DiffModel::from_unified_patch(
                "left.json".to_owned(),
                "right.json".to_owned(),
                black_box(&patch),
            );
            rows = model.row_count(DiffLayout::Unified);
            changes = model.changed_rows(DiffLayout::Unified).len();
            black_box(model);
        }
        let elapsed = started.elapsed();
        eprintln!(
            "diff model build: {elapsed:?}, rows={rows} changes={changes} patch_bytes={}",
            patch.len()
        );
    }

    fn generated_patch(hunks: usize, changes_per_hunk: usize) -> String {
        let mut patch = String::from("--- left.json\n+++ right.json\n");
        for hunk in 0..hunks {
            let start = hunk.saturating_mul(16).saturating_add(1);
            patch.push_str(&format!("@@ -{start},10 +{start},10 @@\n"));
            patch.push_str(" {\n");
            patch.push_str(&format!("   \"id\": {hunk},\n"));
            for change in 0..changes_per_hunk {
                patch.push_str(&format!("-  \"old_{change}\": \"{}\",\n", "x".repeat(48)));
            }
            for change in 0..changes_per_hunk {
                patch.push_str(&format!("+  \"new_{change}\": \"{}\",\n", "y".repeat(48)));
            }
            patch.push_str("   \"ok\": true\n");
            patch.push_str(" }\n");
        }
        patch
    }
}
