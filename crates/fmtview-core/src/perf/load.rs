use std::{
    io::Write,
    time::{Duration, Instant},
};

use tempfile::NamedTempFile;

use crate::{
    load::{IndexedTempFile, LazyTransformedRecordsFile, RecordTimelineViewFile, ViewFile},
    timeline::{FileRecordTimeline, RecordLoadLimit},
    transform::{FormatKind, FormatOptions, format_source_to_temp},
    viewer::FileViewer,
};
use ratatui::layout::Size;

use super::{
    fixtures::{
        HUGE_STRING_FRAGMENT, generated_huge_media_jsonl_source,
        generated_huge_string_jsonl_source, generated_json_document_source, generated_jsonl_source,
        generated_xml_document_source,
    },
    runner::{BenchCase, BenchSample},
};

pub(super) const CASES: &[BenchCase] = &[
    BenchCase {
        label: "raw indexed load",
        shape: "line-indexed",
        layer: "load",
        run: bench_raw_indexed_load,
    },
    BenchCase {
        label: "lazy record first-window load+transform",
        shape: "record-stream",
        layer: "load+transform+readback",
        run: bench_lazy_first_window_format,
    },
    BenchCase {
        label: "lazy record preload load+transform",
        shape: "record-stream",
        layer: "load+transform+spool",
        run: bench_lazy_preload_records_format,
    },
    BenchCase {
        label: "lazy huge string first-window load+transform",
        shape: "record-stream/huge-record",
        layer: "load+transform+readback",
        run: bench_lazy_huge_string_first_window_format,
    },
    BenchCase {
        label: "lazy huge string preload transform+spool",
        shape: "record-stream/huge-record",
        layer: "transform+spool",
        run: bench_lazy_huge_string_preload_format,
    },
    BenchCase {
        label: "lazy huge media first-window collapse",
        shape: "record-stream/huge-media",
        layer: "scan+collapse+spool+readback",
        run: bench_lazy_huge_media_first_window,
    },
    BenchCase {
        label: "timeline tail-first open+format",
        shape: "growing-record-stream/tail",
        layer: "reverse-scan+transform+spool+readback",
        run: bench_timeline_tail_open,
    },
    BenchCase {
        label: "timeline older prepend+format",
        shape: "growing-record-stream/older",
        layer: "reverse-scan+transform+prepend",
        run: bench_timeline_prepend,
    },
    BenchCase {
        label: "timeline append refresh+format",
        shape: "growing-record-stream/newer",
        layer: "refresh+transform+append",
        run: bench_timeline_refresh,
    },
    BenchCase {
        label: "timeline follow refresh+render",
        shape: "growing-record-stream/follow",
        layer: "refresh+transform+viewport",
        run: bench_timeline_follow,
    },
    BenchCase {
        label: "json whole-document eager view open",
        shape: "whole-document",
        layer: "transform+index+readback",
        run: bench_json_whole_document_eager_view_open,
    },
    BenchCase {
        label: "json whole-document index+readback",
        shape: "whole-document",
        layer: "index+readback",
        run: bench_json_whole_document_index_readback,
    },
    BenchCase {
        label: "xml whole-document eager view open",
        shape: "whole-document",
        layer: "transform+index+readback",
        run: bench_xml_whole_document_eager_view_open,
    },
    BenchCase {
        label: "xml whole-document index+readback",
        shape: "whole-document",
        layer: "index+readback",
        run: bench_xml_whole_document_index_readback,
    },
];
fn bench_raw_indexed_load() -> BenchSample {
    let mut temp = NamedTempFile::new().unwrap();
    let line = format!("{}\n", "x".repeat(240));
    let lines = 250_000;
    for _ in 0..lines {
        temp.write_all(line.as_bytes()).unwrap();
    }
    temp.flush().unwrap();
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;

    let started = Instant::now();
    let indexed = IndexedTempFile::new("raw".to_owned(), temp).unwrap();
    let elapsed = started.elapsed();
    let window = indexed.read_window(120_000, 120).unwrap();

    assert_eq!(indexed.line_count(), lines);
    assert_eq!(window.len(), 120);
    BenchSample {
        elapsed,
        records: 0,
        items: 0,
        string_bytes: 0,
        lines: 0,
        indexed_lines: indexed.line_count(),
        window_lines: window.len(),
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_lazy_first_window_format() -> BenchSample {
    let (_temp, source, input_bytes) = generated_jsonl_source(16_384, 512, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedRecordsFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 120).unwrap();
    let elapsed = started.elapsed();

    assert!(!lines.is_empty());
    assert!(file.loaded_record_count() < 32);
    BenchSample {
        elapsed,
        records: file.loaded_record_count(),
        items: 0,
        string_bytes: 0,
        lines: lines.len(),
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_lazy_preload_records_format() -> BenchSample {
    let (_temp, source, input_bytes) = generated_jsonl_source(16_384, 512, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };
    let file = LazyTransformedRecordsFile::new(&source, options).unwrap();
    file.read_window(0, 40).unwrap();

    let started = Instant::now();
    let changed = file
        .preload(20_000, 1_024, Duration::from_secs(30))
        .unwrap();
    let elapsed = started.elapsed();

    assert!(changed);
    assert!(file.loaded_record_count() > 512);
    BenchSample {
        elapsed,
        records: file.loaded_record_count(),
        items: 0,
        string_bytes: 0,
        lines: file.indexed_line_count(),
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_lazy_huge_string_first_window_format() -> BenchSample {
    let (_temp, source, input_bytes) = generated_huge_string_jsonl_source(600_000, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedRecordsFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 6).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(file.loaded_record_count(), 1);
    assert_eq!(lines.len(), 5);
    BenchSample {
        elapsed,
        records: file.loaded_record_count(),
        items: 0,
        string_bytes: HUGE_STRING_FRAGMENT.len() * 600_000,
        lines: lines.len(),
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_lazy_huge_string_preload_format() -> BenchSample {
    let (_temp, source, input_bytes) = generated_huge_string_jsonl_source(600_000, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let file = LazyTransformedRecordsFile::new(&source, options).unwrap();

    let started = Instant::now();
    let changed = file.preload(10, 1, Duration::from_secs(30)).unwrap();
    let elapsed = started.elapsed();

    assert!(changed);
    assert_eq!(file.loaded_record_count(), 1);
    assert_eq!(file.indexed_line_count(), 5);
    BenchSample {
        elapsed,
        records: file.loaded_record_count(),
        items: 0,
        string_bytes: HUGE_STRING_FRAGMENT.len() * 600_000,
        lines: file.indexed_line_count(),
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_lazy_huge_media_first_window() -> BenchSample {
    const PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
    let (_temp, source, input_bytes) = generated_huge_media_jsonl_source(PAYLOAD_BYTES, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedRecordsFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 8).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(file.loaded_record_count(), 1);
    assert!(
        lines
            .iter()
            .any(|line| { line.contains("<media image/png; 12582912 decoded bytes>") })
    );
    assert!(lines.iter().map(String::len).sum::<usize>() < 1024);
    BenchSample {
        elapsed,
        records: 1,
        items: 0,
        string_bytes: PAYLOAD_BYTES,
        lines: lines.len(),
        indexed_lines: file.indexed_line_count(),
        window_lines: lines.len(),
        input_bytes,
        output_bytes: lines.iter().map(String::len).sum(),
    }
}

fn bench_timeline_tail_open() -> BenchSample {
    let temp = generated_follow_file(250_000);
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };

    let started = Instant::now();
    let timeline = FileRecordTimeline::open(temp.path(), "tail.jsonl").unwrap();
    let file = RecordTimelineViewFile::new(Box::new(timeline), options).unwrap();
    let line_count = file.line_count();
    let window = file
        .read_window(line_count.saturating_sub(120), 120)
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(window.len(), 120);
    assert!(window.iter().any(|line| line.contains("249999")));
    BenchSample {
        elapsed,
        records: 128,
        items: 0,
        string_bytes: 0,
        lines: line_count,
        indexed_lines: 0,
        window_lines: window.len(),
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_timeline_prepend() -> BenchSample {
    let temp = generated_follow_file(50_000);
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let timeline = FileRecordTimeline::open(temp.path(), "prepend.jsonl").unwrap();
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        options,
        RecordLoadLimit::new(16, 512 * 1024),
    )
    .unwrap();
    let before = file.line_count();

    let started = Instant::now();
    let change = file.load_older_records(512, 8 * 1024 * 1024).unwrap();
    let elapsed = started.elapsed();

    assert!(change.inserted_lines > 0);
    assert_eq!(change.inserted_at, 0);
    assert!(file.line_count() > before);
    BenchSample {
        elapsed,
        records: 512,
        items: 0,
        string_bytes: 0,
        lines: change.inserted_lines,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_timeline_refresh() -> BenchSample {
    const APPENDED: usize = 512;
    let mut temp = generated_follow_file(10_000);
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let timeline = FileRecordTimeline::open(temp.path(), "refresh.jsonl").unwrap();
    let file = RecordTimelineViewFile::new(Box::new(timeline), options).unwrap();
    for index in 10_000..10_000 + APPENDED {
        writeln!(
            temp,
            "{{\"index\":{index},\"message\":\"timeline benchmark\"}}"
        )
        .unwrap();
    }
    temp.flush().unwrap();
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;

    let started = Instant::now();
    let change = file.refresh_records(APPENDED, 8 * 1024 * 1024).unwrap();
    let elapsed = started.elapsed();

    assert!(change.appended_lines > 0);
    BenchSample {
        elapsed,
        records: APPENDED,
        items: 0,
        string_bytes: 0,
        lines: change.appended_lines,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes: 0,
    }
}

fn bench_timeline_follow() -> BenchSample {
    const APPENDED: usize = 32;
    let mut temp = generated_follow_file(10_000);
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let timeline = FileRecordTimeline::open(temp.path(), "follow.jsonl").unwrap();
    let file = RecordTimelineViewFile::new(Box::new(timeline), options).unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(100, 30);
    viewer.render(size, None).unwrap();
    for index in 10_000..10_000 + APPENDED {
        writeln!(
            temp,
            "{{\"index\":{index},\"message\":\"follow benchmark\"}}"
        )
        .unwrap();
    }
    temp.flush().unwrap();
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;

    let started = Instant::now();
    assert!(viewer.preload().unwrap());
    let frame = viewer.render(size, None).unwrap();
    let elapsed = started.elapsed();

    assert!(frame.footer_text.contains("follow:on"));
    BenchSample {
        elapsed,
        records: APPENDED,
        items: 0,
        string_bytes: 0,
        lines: frame.styled.len(),
        indexed_lines: 0,
        window_lines: frame.styled.len(),
        input_bytes,
        output_bytes: 0,
    }
}

fn generated_follow_file(records: usize) -> NamedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for index in 0..records {
        writeln!(
            temp,
            "{{\"index\":{index},\"message\":\"timeline benchmark\"}}"
        )
        .unwrap();
    }
    temp.flush().unwrap();
    temp
}

fn bench_json_whole_document_eager_view_open() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_json_document_source(32_768, 128);
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };

    let started = Instant::now();
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;
    let indexed = IndexedTempFile::new(source.label().to_owned(), formatted).unwrap();
    let window = indexed.read_window(120_000, 120).unwrap();
    let elapsed = started.elapsed();

    assert!(!window.is_empty());
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: indexed.line_count(),
        window_lines: window.len(),
        input_bytes,
        output_bytes,
    }
}

fn bench_json_whole_document_index_readback() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_json_document_source(32_768, 128);
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;

    let started = Instant::now();
    let indexed = IndexedTempFile::new(source.label().to_owned(), formatted).unwrap();
    let window = indexed.read_window(120_000, 120).unwrap();
    let elapsed = started.elapsed();

    assert!(!window.is_empty());
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: indexed.line_count(),
        window_lines: window.len(),
        input_bytes,
        output_bytes,
    }
}

fn bench_xml_whole_document_eager_view_open() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_xml_document_source(65_536);
    let options = FormatOptions {
        kind: FormatKind::Xml,
        indent: 2,
    };

    let started = Instant::now();
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;
    let indexed = IndexedTempFile::new(source.label().to_owned(), formatted).unwrap();
    let window = indexed.read_window(120_000, 120).unwrap();
    let elapsed = started.elapsed();

    assert!(!window.is_empty());
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: indexed.line_count(),
        window_lines: window.len(),
        input_bytes,
        output_bytes,
    }
}

fn bench_xml_whole_document_index_readback() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_xml_document_source(65_536);
    let options = FormatOptions {
        kind: FormatKind::Xml,
        indent: 2,
    };
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;

    let started = Instant::now();
    let indexed = IndexedTempFile::new(source.label().to_owned(), formatted).unwrap();
    let window = indexed.read_window(120_000, 120).unwrap();
    let elapsed = started.elapsed();

    assert!(!window.is_empty());
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: indexed.line_count(),
        window_lines: window.len(),
        input_bytes,
        output_bytes,
    }
}
