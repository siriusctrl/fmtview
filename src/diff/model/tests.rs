use super::{
    DiffChange, DiffLayout, DiffModel, SideDiffRow, UnifiedDiffRow, inline::INLINE_DIFF_PAIR_BUDGET,
};
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
            left: Some(1),
            right: Some(2)
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
fn reads_unified_patch_from_stream() {
    let patch = "\
--- left
+++ right
@@ -1,1 +1,1 @@
-old
+new
";
    let model = DiffModel::from_unified_reader(
        "left".to_owned(),
        "right".to_owned(),
        std::io::Cursor::new(patch),
    )
    .unwrap();

    assert!(model.has_changes());
    assert_eq!(model.row_count(DiffLayout::Unified), 2);
    assert_eq!(model.changed_rows(DiffLayout::SideBySide), &[0]);
}

#[test]
fn large_inline_diff_work_is_budgeted() {
    let mut patch = String::from("--- left\n+++ right\n@@ -1,2050 +1,2050 @@\n");
    for index in 0..2050 {
        patch.push_str(&format!("-old-{index}\n"));
    }
    for index in 0..2050 {
        patch.push_str(&format!("+new-{index}\n"));
    }

    let model = DiffModel::from_unified_patch("left".to_owned(), "right".to_owned(), &patch);
    let row = &model.unified_rows()[INLINE_DIFF_PAIR_BUDGET + 1];

    assert!(matches!(
        row,
        UnifiedDiffRow::Delete {
            change: DiffChange {
                left_range: None,
                right_range: None,
                ..
            },
            ..
        }
    ));
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
