use std::{
    io::Write,
    time::{Duration, Instant},
};

use tempfile::NamedTempFile;

use crate::{
    load::{IndexedTempFile, LazyTransformedRecordsFile, ViewFile},
    transform::{FormatKind, FormatOptions, format_source_to_temp},
};

use super::{
    fixtures::{
        HUGE_STRING_FRAGMENT, generated_huge_string_jsonl_source, generated_json_document_source,
        generated_jsonl_source, generated_xml_document_source,
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
