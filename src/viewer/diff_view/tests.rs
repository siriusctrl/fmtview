use super::*;
use std::{hint::black_box, time::Instant};

use ratatui::text::Line;

use crate::diff::{DiffLayout, DiffModel};

use super::super::palette::{diff_removed_inline_bg, diff_removed_line_bg};
use super::super::render::char_count;
use super::{input::*, render::*};

fn sample_model() -> DiffModel {
    DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1,4 +1,4 @@
 {
-  \"a\": 1,
+  \"a\": 2,
   \"b\": true
 }
",
    )
}

fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn line_width(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| char_count(span.content.as_ref()))
        .sum()
}

#[test]
fn renders_unified_diff_rows_with_line_numbers() {
    let lines = render_rows(&sample_model(), DiffLayout::Unified, 0, 3, 80, 0);
    let text = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>();

    assert_eq!(text[0], "1 1   {");
    assert_eq!(text[1], "2   -   \"a\": 1,");
    assert_eq!(text[2], "  2 +   \"a\": 2,");
}

#[test]
fn renders_side_by_side_change_pairs() {
    let lines = render_rows(&sample_model(), DiffLayout::SideBySide, 1, 1, 80, 0);
    let text = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(text.contains("-   \"a\": 1,"));
    assert!(text.contains("+   \"a\": 2,"));
}

#[test]
fn interactive_diff_hides_patch_control_rows() {
    let lines = render_rows(&sample_model(), DiffLayout::Unified, 0, 10, 80, 0);
    let text = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(!text.contains("@@"));
    assert!(!text.contains("---"));
    assert!(!text.contains("+++"));
}

#[test]
fn changed_rows_use_line_and_inline_backgrounds() {
    let lines = render_rows(&sample_model(), DiffLayout::Unified, 1, 1, 80, 0);
    let removed = &lines[0];

    assert!(removed.spans.iter().any(|span| {
        span.style.bg == Some(diff_removed_line_bg(crate::diff::DiffIntensity::Low))
    }));
    assert!(removed.spans.iter().any(|span| {
        span.content.as_ref().contains('1')
            && span.style.bg == Some(diff_removed_inline_bg(crate::diff::DiffIntensity::Low))
    }));
}

#[test]
fn change_jump_skips_adjacent_rows_in_same_block() {
    let model = DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1,4 +1,4 @@
 a
-old
+new
 b
 c
@@ -20,3 +20,3 @@
 x
-old2
+new2
 y
",
    );
    let targets = change_block_starts(model.changed_rows(DiffLayout::Unified));
    let mut state = DiffViewState::new(DiffLayout::Unified);

    assert_eq!(targets, vec![1, 6]);
    jump_change(&model, &mut state, DiffJump::Next, 9);
    assert_eq!(state.change_cursor, Some(1));

    assert!(jump_change(&model, &mut state, DiffJump::Next, 9));
    assert_eq!(state.change_cursor, Some(6));
    assert_eq!(state.top, 3);
}

#[test]
fn diff_scroll_clamps_to_last_full_page() {
    let model = sample_model();
    let mut state = DiffViewState::new(DiffLayout::Unified);

    assert!(scroll_by(&mut state, &model, 3, 80, 99));

    assert_eq!(state.top, model.row_count(DiffLayout::Unified) - 3);
}

#[test]
fn side_by_side_scroll_uses_longer_display_side() {
    let model = DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1,3 +1,5 @@
 a
-old
+new1
+new2
+new3
 z
",
    );
    let mut state = DiffViewState::new(DiffLayout::SideBySide);

    assert!(scroll_by(&mut state, &model, 3, 80, 99));
    assert_eq!(state.top, 2);

    let text = render_rows(&model, DiffLayout::SideBySide, state.top, 3, 80, 0)
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(text.contains("new3"));
}

#[test]
fn unified_diff_wraps_and_scrolls_inside_long_rows() {
    let model = DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1,1 +1,1 @@
-abcdefghijklmnopqrstuvwxyz
+abcxyzdefghijklmnopqrstuvwxyz
",
    );
    let mut state = DiffViewState::new(DiffLayout::Unified);
    state.top = 0;

    let lines = render_rows_with_status(&model, DiffLayout::Unified, 0, 0, 3, 18, 0, true)
        .rows
        .iter()
        .map(line_text)
        .collect::<Vec<_>>();

    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("- abcdefghij"));
    assert!(lines[1].contains("mnopqrst"));
    assert!(!lines[1].contains("1  "));

    assert!(scroll_by(&mut state, &model, 3, 18, 1));
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, 1);
}

#[test]
fn side_by_side_wrap_uses_longer_cell() {
    let model = DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1,1 +1,1 @@
-short
+abcdefghijklmnopqrstuvwxyz0123456789
",
    );

    let lines = render_rows_with_status(&model, DiffLayout::SideBySide, 0, 0, 4, 32, 0, true).rows;
    let text = lines.iter().map(line_text).collect::<Vec<_>>();

    assert!(text.len() > 1);
    assert!(text.iter().any(|line| line.contains("short")));
    assert!(text.iter().any(|line| line.contains("lmnopqrst")));
    assert!(text.iter().any(|line| line.contains("uvwxyz")));
}

#[test]
fn side_by_side_wrapped_rows_fit_content_width() {
    let model = DiffModel::from_unified_patch(
        "left".to_owned(),
        "right".to_owned(),
        "\
--- left
+++ right
@@ -1000,1 +1000,1 @@
-abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz
+01234567890123456789012345678901234567890123456789
",
    );

    for width in [24, 32, 80, 118] {
        let rows =
            render_rows_with_status(&model, DiffLayout::SideBySide, 0, 0, 16, width, 0, true).rows;

        assert!(!rows.is_empty());
        assert!(
            rows.iter().all(|line| line_width(line) <= width),
            "wrapped side-by-side rows exceeded width {width}: {:?}",
            rows.iter().map(line_text).collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "performance smoke; run benches/diff-performance.sh"]
fn perf_diff_view_render() {
    let patch = generated_patch(2_048, 3);
    let model = DiffModel::from_unified_patch("left".to_owned(), "right".to_owned(), &patch);
    let mut rendered_rows = 0_usize;
    let started = Instant::now();
    for index in 0..2_000 {
        let top = index
            % model
                .row_count(DiffLayout::Unified)
                .saturating_sub(32)
                .max(1);
        let unified = render_rows(&model, DiffLayout::Unified, top, 28, 120, index % 8);
        rendered_rows = rendered_rows.saturating_add(unified.len());
        black_box(unified);

        let side_top = index
            % model
                .row_count(DiffLayout::SideBySide)
                .saturating_sub(32)
                .max(1);
        let side = render_rows(&model, DiffLayout::SideBySide, side_top, 28, 160, index % 8);
        rendered_rows = rendered_rows.saturating_add(side.len());
        black_box(side);
    }
    let elapsed = started.elapsed();
    eprintln!(
        "diff view render: {elapsed:?}, rows={} changes={} rendered_rows={} patch_bytes={}",
        model.row_count(DiffLayout::Unified),
        model.changed_rows(DiffLayout::Unified).len(),
        rendered_rows,
        patch.len()
    );
}

fn generated_patch(hunks: usize, changes_per_hunk: usize) -> String {
    let mut patch = String::from("--- left\n+++ right\n");
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
