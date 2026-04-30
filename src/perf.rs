use std::{
    env,
    io::Write,
    time::{Duration, Instant},
};

use tempfile::{Builder as TempFileBuilder, NamedTempFile};

use crate::{
    input::InputSource,
    load::{IndexedTempFile, LazyTransformedFile, ViewFile},
    transform::{FormatKind, FormatOptions, format_record_to_string, format_source_to_temp},
};

const DEFAULT_SAMPLES: usize = 7;
const HUGE_STRING_FRAGMENT: &[u8] = br#"<item id=\"1\"><name>visible</name></item>"#;

#[derive(Clone, Copy)]
struct BenchCase {
    label: &'static str,
    shape: &'static str,
    layer: &'static str,
    run: fn() -> BenchSample,
}

struct BenchSample {
    elapsed: Duration,
    records: usize,
    items: usize,
    string_bytes: usize,
    lines: usize,
    indexed_lines: usize,
    window_lines: usize,
    input_bytes: usize,
    output_bytes: usize,
}

impl BenchSample {
    fn elapsed_ms(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1000.0
    }
}

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_load_suite() {
    run_suite("load", LOAD_CASES);
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_format_suite() {
    run_suite("transform", FORMAT_CASES);
}

fn run_suite(name: &str, cases: &[BenchCase]) {
    let samples = sample_count();
    let case_filter = env::var("FMTVIEW_PERF_CASE").ok();

    println!("fmtview {name} performance smoke");
    println!("samples: {samples}");

    for case in cases.iter().filter(|case| {
        case_filter
            .as_deref()
            .is_none_or(|filter| case.matches(filter))
    }) {
        run_case(*case, samples);
    }
}

impl BenchCase {
    fn matches(self, filter: &str) -> bool {
        self.label.contains(filter) || self.shape.contains(filter) || self.layer.contains(filter)
    }
}

fn sample_count() -> usize {
    env::var("FMTVIEW_PERF_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SAMPLES)
}

fn run_case(case: BenchCase, samples: usize) {
    println!();
    println!("== {} ==", case.label);
    println!("shape={} layer={}", case.shape, case.layer);

    let mut timings = Vec::with_capacity(samples);
    for sample_index in 1..=samples {
        let sample = (case.run)();
        let ms = sample.elapsed_ms();
        timings.push(ms);
        println!(
            "sample {sample_index:02}: {ms:8.3}ms  records={}  items={}  string_bytes={}  lines={}  indexed_lines={}  window_lines={}  input_bytes={}  output_bytes={}",
            sample.records,
            sample.items,
            sample.string_bytes,
            sample.lines,
            sample.indexed_lines,
            sample.window_lines,
            sample.input_bytes,
            sample.output_bytes,
        );
    }

    let summary = TimingSummary::from_samples(&mut timings);
    println!(
        "time: median={:.3}ms min={:.3}ms max={:.3}ms avg={:.3}ms",
        summary.median, summary.min, summary.max, summary.avg
    );
}

struct TimingSummary {
    median: f64,
    min: f64,
    max: f64,
    avg: f64,
}

impl TimingSummary {
    fn from_samples(values: &mut [f64]) -> Self {
        values.sort_by(f64::total_cmp);
        let mid = values.len() / 2;
        let median = if values.len() % 2 == 0 {
            (values[mid - 1] + values[mid]) / 2.0
        } else {
            values[mid]
        };
        let min = values[0];
        let max = values[values.len() - 1];
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        Self {
            median,
            min,
            max,
            avg,
        }
    }
}

const LOAD_CASES: &[BenchCase] = &[
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

const FORMAT_CASES: &[BenchCase] = &[
    BenchCase {
        label: "jsonl record batch CPU",
        shape: "record-stream",
        layer: "transform",
        run: bench_jsonl_record_batch_format,
    },
    BenchCase {
        label: "jsonl source full format",
        shape: "record-stream",
        layer: "transform+write",
        run: bench_jsonl_source_full_format,
    },
    BenchCase {
        label: "single huge object-array record format",
        shape: "record-stream/huge-record",
        layer: "transform",
        run: bench_single_huge_object_array_record_format,
    },
    BenchCase {
        label: "single huge string field record format",
        shape: "record-stream/huge-record",
        layer: "transform",
        run: bench_single_huge_string_field_record_format,
    },
    BenchCase {
        label: "json whole-document format",
        shape: "whole-document",
        layer: "transform",
        run: bench_json_whole_document_format,
    },
    BenchCase {
        label: "xml whole-document format",
        shape: "whole-document",
        layer: "transform",
        run: bench_xml_whole_document_format,
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
    let file = LazyTransformedFile::new(&source, options).unwrap();
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
    let file = LazyTransformedFile::new(&source, options).unwrap();
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
    let file = LazyTransformedFile::new(&source, options).unwrap();
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
    let file = LazyTransformedFile::new(&source, options).unwrap();

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

fn bench_jsonl_record_batch_format() -> BenchSample {
    let records = generated_jsonl_records(16_384, 512);
    let input_bytes = records.iter().map(Vec::len).sum::<usize>();
    let started = Instant::now();
    let mut output_bytes = 0_usize;

    for record in &records {
        let rendered = format_record_to_string(record, FormatKind::Jsonl, 2).unwrap();
        output_bytes = output_bytes.saturating_add(rendered.len());
    }

    let elapsed = started.elapsed();
    assert!(output_bytes > input_bytes);
    BenchSample {
        elapsed,
        records: records.len(),
        items: 0,
        string_bytes: 0,
        lines: 0,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
    }
}

fn bench_jsonl_source_full_format() -> BenchSample {
    let mut temp = NamedTempFile::new().unwrap();
    let records = generated_jsonl_records(16_384, 512);
    let mut input_bytes = 0_usize;
    for record in &records {
        temp.write_all(record).unwrap();
        temp.write_all(b"\n").unwrap();
        input_bytes = input_bytes.saturating_add(record.len()).saturating_add(1);
    }
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };

    let started = Instant::now();
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;

    assert!(output_bytes > input_bytes);
    BenchSample {
        elapsed,
        records: records.len(),
        items: 0,
        string_bytes: 0,
        lines: 0,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
    }
}

fn bench_json_whole_document_format() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_json_document_source(32_768, 128);
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };

    let started = Instant::now();
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;

    assert!(output_bytes > input_bytes);
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
    }
}

fn bench_xml_whole_document_format() -> BenchSample {
    let (_temp, source, input_bytes, items) = generated_xml_document_source(65_536);
    let options = FormatOptions {
        kind: FormatKind::Xml,
        indent: 2,
    };

    let started = Instant::now();
    let formatted = format_source_to_temp(&source, &options).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = formatted.as_file().metadata().unwrap().len() as usize;

    assert!(output_bytes >= input_bytes);
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines: 0,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
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

fn bench_single_huge_object_array_record_format() -> BenchSample {
    let items = 32_768;
    let record = generated_huge_object_array_record(items, 128);
    let input_bytes = record.len();
    let started = Instant::now();
    let rendered = format_record_to_string(&record, FormatKind::Jsonl, 2).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = rendered.len();
    let lines = rendered.lines().count();

    assert!(lines > items);
    BenchSample {
        elapsed,
        records: 1,
        items,
        string_bytes: 0,
        lines,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
    }
}

fn bench_single_huge_string_field_record_format() -> BenchSample {
    let repeats = 600_000;
    let record = generated_huge_string_field_record(repeats);
    let input_bytes = record.len();
    let started = Instant::now();
    let rendered = format_record_to_string(&record, FormatKind::Jsonl, 2).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = rendered.len();
    let lines = rendered.lines().count();

    assert_eq!(lines, 5);
    BenchSample {
        elapsed,
        records: 1,
        items: 0,
        string_bytes: HUGE_STRING_FRAGMENT.len() * repeats,
        lines,
        indexed_lines: 0,
        window_lines: 0,
        input_bytes,
        output_bytes,
    }
}

fn generated_jsonl_records(count: usize, message_len: usize) -> Vec<Vec<u8>> {
    let message = "x".repeat(message_len);
    (0..count)
        .map(|index| {
            format!(
                r#"{{"index":{index},"level":"info","message":"{message}","payload":{{"xml":"<root><item id=\"{index}\"><name>visible</name></item></root>","items":[{{"a":1}},{{"b":2}},{{"c":{{"d":true}}}}]}}}}"#
            )
            .into_bytes()
        })
        .collect()
}

fn generated_jsonl_source(
    count: usize,
    message_len: usize,
    suffix: &str,
) -> (NamedTempFile, InputSource, usize) {
    let mut temp = TempFileBuilder::new().suffix(suffix).tempfile().unwrap();
    let records = generated_jsonl_records(count, message_len);
    let mut input_bytes = 0_usize;
    for record in records {
        temp.write_all(&record).unwrap();
        temp.write_all(b"\n").unwrap();
        input_bytes = input_bytes.saturating_add(record.len()).saturating_add(1);
    }
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source, input_bytes)
}

fn generated_huge_object_array_record(items: usize, message_len: usize) -> Vec<u8> {
    let message = "y".repeat(message_len);
    let mut record = Vec::new();
    write!(record, r#"{{"kind":"huge","items":["#).unwrap();
    for index in 0..items {
        if index > 0 {
            record.push(b',');
        }
        write!(
            record,
            r#"{{"index":{index},"message":"{message}","nested":{{"ok":true,"value":{index}}}}}"#
        )
        .unwrap();
    }
    record.extend_from_slice(b"]}");
    record
}

fn generated_json_document_source(
    items: usize,
    message_len: usize,
) -> (NamedTempFile, InputSource, usize, usize) {
    let mut temp = TempFileBuilder::new().suffix(".json").tempfile().unwrap();
    let record = generated_huge_object_array_record(items, message_len);
    temp.write_all(&record).unwrap();
    temp.flush().unwrap();
    let input_bytes = record.len();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source, input_bytes, items)
}

fn generated_xml_document_source(items: usize) -> (NamedTempFile, InputSource, usize, usize) {
    let mut temp = TempFileBuilder::new().suffix(".xml").tempfile().unwrap();
    temp.write_all(b"<root>").unwrap();
    let mut input_bytes = "<root>".len();
    for index in 0..items {
        let item = format!(
            r#"<item id="{index}"><name>visible</name><value>{index}</value><flag>true</flag></item>"#
        );
        temp.write_all(item.as_bytes()).unwrap();
        input_bytes = input_bytes.saturating_add(item.len());
    }
    temp.write_all(b"</root>").unwrap();
    input_bytes = input_bytes.saturating_add("</root>".len());
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source, input_bytes, items)
}

fn generated_huge_string_field_record(repeats: usize) -> Vec<u8> {
    let mut record = Vec::with_capacity(HUGE_STRING_FRAGMENT.len() * repeats + 128);
    record.extend_from_slice(br#"{"id":1,"kind":"huge-string","message":""#);
    for _ in 0..repeats {
        record.extend_from_slice(HUGE_STRING_FRAGMENT);
    }
    record.extend_from_slice(br#""}"#);
    record
}

fn generated_huge_string_jsonl_source(
    repeats: usize,
    suffix: &str,
) -> (NamedTempFile, InputSource, usize) {
    let mut temp = TempFileBuilder::new().suffix(suffix).tempfile().unwrap();
    temp.write_all(br#"{"id":1,"kind":"huge-string","message":""#)
        .unwrap();
    for _ in 0..repeats {
        temp.write_all(HUGE_STRING_FRAGMENT).unwrap();
    }
    temp.write_all(br#""}"#).unwrap();
    temp.write_all(b"\n").unwrap();
    temp.flush().unwrap();
    let input_bytes = temp.as_file().metadata().unwrap().len() as usize;
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source, input_bytes)
}
