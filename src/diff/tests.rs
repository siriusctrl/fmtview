use super::*;
use std::{
    fs,
    hint::black_box,
    io::Write,
    time::{Duration, Instant},
};

use tempfile::Builder as TempFileBuilder;

use crate::{
    input::InputSource,
    transform::{FormatKind, FormatOptions},
};

#[test]
fn record_stream_diff_view_is_lazy_for_different_files() {
    let mut left = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    let mut right = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    writeln!(left, r#"{{"id":1,"value":"same"}}"#).unwrap();
    writeln!(left, r#"{{"id":2,"value":"left"}}"#).unwrap();
    writeln!(left, r#"{{"id":3,"value":"tail"}}"#).unwrap();
    writeln!(right, r#"{{"id":1,"value":"same"}}"#).unwrap();
    writeln!(right, r#"{{"id":2,"value":"right"}}"#).unwrap();
    writeln!(right, r#"{{"id":3,"value":"tail"}}"#).unwrap();
    left.flush().unwrap();
    right.flush().unwrap();
    let left = InputSource::from_arg(left.path().to_str().unwrap(), None).unwrap();
    let right = InputSource::from_arg(right.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let mut view = diff_view(&left, &right, &options).unwrap();
    assert!(matches!(view, DiffView::Lazy(_)));

    view.preload(16, Duration::from_secs(1)).unwrap();
    let model = view.model();

    assert!(model.has_changes());
    assert!(!model.changed_rows(DiffLayout::Unified).is_empty());
    assert!(model.unified_rows().iter().any(|row| {
        matches!(
            row,
            UnifiedDiffRow::Insert { content, .. } if content.contains("\"right\"")
        )
    }));
}

#[test]
fn lazy_record_diff_resyncs_after_inserted_record() {
    let mut left = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    let mut right = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    writeln!(left, r#"{{"id":1,"value":"same"}}"#).unwrap();
    writeln!(left, r#"{{"id":3,"value":"tail"}}"#).unwrap();
    writeln!(right, r#"{{"id":1,"value":"same"}}"#).unwrap();
    writeln!(right, r#"{{"id":2,"value":"inserted"}}"#).unwrap();
    writeln!(right, r#"{{"id":3,"value":"tail"}}"#).unwrap();
    left.flush().unwrap();
    right.flush().unwrap();
    let left = InputSource::from_arg(left.path().to_str().unwrap(), None).unwrap();
    let right = InputSource::from_arg(right.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let mut view = diff_view(&left, &right, &options).unwrap();
    view.preload(16, Duration::from_secs(1)).unwrap();
    let model = view.model();

    assert!(model.unified_rows().iter().any(|row| {
        matches!(
            row,
            UnifiedDiffRow::Insert { content, .. } if content.contains("\"inserted\"")
        )
    }));
    assert!(model.unified_rows().iter().any(|row| {
        matches!(
            row,
            UnifiedDiffRow::Context { content, .. } if content.contains("\"tail\"")
        )
    }));
    assert!(!model.unified_rows().iter().any(|row| {
        matches!(
            row,
            UnifiedDiffRow::Delete { content, .. } | UnifiedDiffRow::Insert { content, .. }
                if content.contains("\"tail\"")
        )
    }));
}

#[test]
fn lazy_record_diff_ignores_formatting_only_record_differences() {
    let mut left = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    let mut right = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    writeln!(left, r#"{{"a":1,"b":[true,false]}}"#).unwrap();
    writeln!(left, r#"{{"c":2}}"#).unwrap();
    writeln!(right, r#"{{ "a" : 1, "b" : [ true, false ] }}"#).unwrap();
    writeln!(right, r#"{{ "c" : 2 }}"#).unwrap();
    left.flush().unwrap();
    right.flush().unwrap();
    let left = InputSource::from_arg(left.path().to_str().unwrap(), None).unwrap();
    let right = InputSource::from_arg(right.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let mut view = diff_view(&left, &right, &options).unwrap();
    assert!(matches!(view, DiffView::Lazy(_)));
    view.preload(16, Duration::from_secs(1)).unwrap();

    assert!(view.is_complete());
    assert!(!view.model().has_changes());
    assert_eq!(view.model().row_count(DiffLayout::Unified), 1);
}

#[test]
fn lazy_record_diff_keeps_unchanged_tail_bounded() {
    let mut left = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    let mut right = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    writeln!(left, r#"{{"id":0,"value":"left"}}"#).unwrap();
    writeln!(right, r#"{{"id":0,"value":"right"}}"#).unwrap();
    for index in 1..=1_000 {
        writeln!(left, r#"{{"id":{index},"value":"same"}}"#).unwrap();
        writeln!(right, r#"{{"id":{index},"value":"same"}}"#).unwrap();
    }
    left.flush().unwrap();
    right.flush().unwrap();
    let left = InputSource::from_arg(left.path().to_str().unwrap(), None).unwrap();
    let right = InputSource::from_arg(right.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let mut view = diff_view(&left, &right, &options).unwrap();
    view.preload(2_000, Duration::from_secs(1)).unwrap();
    let model = view.model();

    assert!(view.is_complete());
    assert!(model.has_changes());
    assert!(model.row_count(DiffLayout::Unified) < 64);
    assert!(model.unified_rows().iter().any(|row| {
        matches!(
            row,
            UnifiedDiffRow::Message { text } if text.contains("unchanged records omitted")
        )
    }));
}

#[test]
fn same_file_diff_stdout_is_empty_for_valid_inputs() {
    let mut file = TempFileBuilder::new().suffix(".json").tempfile().unwrap();
    writeln!(file, r#"{{"ok":true}}"#).unwrap();
    file.flush().unwrap();
    let source = InputSource::from_arg(file.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };

    let output = diff_sources(&source, &source, &options, false).unwrap();

    assert_eq!(fs::read_to_string(output.path()).unwrap(), "");
}

#[test]
#[ignore = "performance smoke; run benches/diff-performance.sh"]
fn perf_lazy_record_diff_view_open() {
    let mut left = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    let mut right = TempFileBuilder::new().suffix(".jsonl").tempfile().unwrap();
    for index in 0..200_000 {
        let left_payload = "x".repeat(96);
        let right_payload = if index % 50_000 == 0 {
            "y".repeat(96)
        } else {
            left_payload.clone()
        };
        writeln!(
            left,
            "{{\"index\":{index},\"payload\":{{\"message\":\"{}\"}}}}",
            left_payload
        )
        .unwrap();
        writeln!(
            right,
            "{{\"index\":{index},\"payload\":{{\"message\":\"{}\"}}}}",
            right_payload
        )
        .unwrap();
    }
    left.flush().unwrap();
    right.flush().unwrap();
    let bytes =
        left.as_file().metadata().unwrap().len() + right.as_file().metadata().unwrap().len();
    let left = InputSource::from_arg(left.path().to_str().unwrap(), None).unwrap();
    let right = InputSource::from_arg(right.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let started = Instant::now();
    let mut view = diff_view(&left, &right, &options).unwrap();
    view.preload(512, Duration::from_millis(30)).unwrap();
    let elapsed = started.elapsed();
    let model = view.model();

    eprintln!(
        "lazy record diff view open: {elapsed:?}, rows={} changes={} bytes={bytes}",
        model.row_count(DiffLayout::Unified),
        model.changed_rows(DiffLayout::Unified).len(),
    );
    black_box(view);
}
