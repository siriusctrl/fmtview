use std::{
    io::Write,
    time::{Duration, Instant},
};

use tempfile::Builder as TempFileBuilder;
use tempfile::NamedTempFile;

use crate::transform::{FormatKind, FormatOptions};

use super::*;

fn temp_source(contents: &[u8]) -> (NamedTempFile, InputSource) {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(contents).unwrap();
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source)
}

#[test]
fn lazy_load_reads_only_records_needed_for_first_window() {
    let mut data = Vec::new();
    for index in 0..1000 {
        writeln!(
            data,
            "{{\"index\":{index},\"payload\":{{\"name\":\"item\",\"ok\":true}}}}"
        )
        .unwrap();
    }
    let (_temp, source) = temp_source(&data);
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };
    let file = LazyTransformedFile::new(&source, options).unwrap();

    let lines = file.read_window(0, 12).unwrap();

    assert_eq!(lines[0], "{");
    assert!(lines.iter().any(|line| line.contains("\"payload\"")));
    assert!(
        file.loaded_record_count() < 4,
        "first window should not scan all input records"
    );
}

#[test]
fn lazy_load_idle_preload_advances_known_line_count() {
    let mut data = Vec::new();
    for index in 0..10 {
        writeln!(data, "{{\"index\":{index},\"payload\":{{\"ok\":true}}}}").unwrap();
    }
    let (_temp, source) = temp_source(&data);
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };
    let file = LazyTransformedFile::new(&source, options).unwrap();

    file.read_window(0, 4).unwrap();
    let before = file.line_count();
    let changed = file.preload(20, 2, Duration::from_secs(1)).unwrap();
    let after = file.line_count();

    assert!(changed);
    assert!(after > before);
    assert!(!file.line_count_exact());
}

#[test]
fn explicit_jsonl_errors_on_malformed_record() {
    let (_temp, source) = temp_source(b"{\"ok\":true}\n{\"broken\":\n");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let file = LazyTransformedFile::new(&source, options).unwrap();

    let error = file.read_window(0, 20).unwrap_err();

    assert!(error.to_string().contains("failed to parse JSON record"));
}

#[test]
fn lazy_load_reads_spooled_lines_after_preload() {
    let (_temp, source) = temp_source(b"{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n");
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };
    let file = LazyTransformedFile::new(&source, options).unwrap();

    assert!(file.preload(20, 3, Duration::from_secs(1)).unwrap());
    assert!(file.indexed_line_count() > 6);
    assert!(file.read_window(0, 1).unwrap()[0].contains('{'));
    assert!(
        file.read_window(file.indexed_line_count().saturating_sub(2), 2)
            .unwrap()
            .iter()
            .any(|line| line.contains("\"c\""))
    );
}

#[test]
#[ignore = "performance smoke; generates a large temporary JSONL-style file"]
fn perf_lazy_load_first_window_does_not_scan_generated_large_file() {
    let mut temp = NamedTempFile::new().unwrap();
    let record = br#"{"level":"info","payload":{"message":"lazy performance smoke","xml":"<root><item id=\"1\"><name>visible</name></item><item id=\"2\"><name>visible</name></item></root>","items":[{"a":1},{"b":2},{"c":{"d":{"e":true}}}]}}"#;
    let target = 128 * 1024 * 1024;
    let mut written = 0_usize;
    while written < target {
        temp.write_all(record).unwrap();
        temp.write_all(b"\n").unwrap();
        written += record.len() + 1;
    }
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 40).unwrap();
    let elapsed = started.elapsed();

    eprintln!(
        "lazy generated first window: {elapsed:?}, records read: {}",
        file.loaded_record_count()
    );
    assert!(!lines.is_empty());
    assert!(
        file.loaded_record_count() < 8,
        "first window should read only enough records to fill the viewport"
    );
}

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_lazy_first_window_format() {
    let (_temp, source, input_bytes) = generated_jsonl_source(16_384, 512, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Auto,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 120).unwrap();
    let elapsed = started.elapsed();

    eprintln!(
        "lazy first window format: {elapsed:?}, records={}, lines={}, input_bytes={input_bytes}",
        file.loaded_record_count(),
        lines.len(),
    );
    assert!(!lines.is_empty());
    assert!(
        file.loaded_record_count() < 32,
        "first window should only format records needed for visible lines"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "lazy first window format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_lazy_preload_records_format() {
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

    eprintln!(
        "lazy preload records format: {elapsed:?}, records={}, lines={}, input_bytes={input_bytes}",
        file.loaded_record_count(),
        file.indexed_line_count(),
    );
    assert!(changed);
    assert!(file.loaded_record_count() > 512);
    assert!(
        elapsed < Duration::from_secs(5),
        "lazy preload records format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_lazy_huge_string_first_window_format() {
    let (_temp, source, input_bytes) = generated_huge_string_jsonl_source(600_000, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };

    let started = Instant::now();
    let file = LazyTransformedFile::new(&source, options).unwrap();
    let lines = file.read_window(0, 6).unwrap();
    let elapsed = started.elapsed();

    eprintln!(
        "lazy huge string first window format: {elapsed:?}, records={}, lines={}, input_bytes={input_bytes}",
        file.loaded_record_count(),
        lines.len(),
    );
    assert_eq!(file.loaded_record_count(), 1);
    assert_eq!(lines.len(), 5);
    assert!(
        elapsed < Duration::from_secs(5),
        "lazy huge string first window format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/load-performance.sh"]
fn perf_lazy_huge_string_preload_format() {
    let (_temp, source, input_bytes) = generated_huge_string_jsonl_source(600_000, ".jsonl");
    let options = FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    };
    let file = LazyTransformedFile::new(&source, options).unwrap();

    let started = Instant::now();
    let changed = file.preload(10, 1, Duration::from_secs(30)).unwrap();
    let elapsed = started.elapsed();

    eprintln!(
        "lazy huge string preload format: {elapsed:?}, records={}, lines={}, input_bytes={input_bytes}",
        file.loaded_record_count(),
        file.indexed_line_count(),
    );
    assert!(changed);
    assert_eq!(file.loaded_record_count(), 1);
    assert_eq!(file.indexed_line_count(), 5);
    assert!(
        elapsed < Duration::from_secs(5),
        "lazy huge string preload format took {elapsed:?}"
    );
}

fn generated_jsonl_source(
    count: usize,
    message_len: usize,
    suffix: &str,
) -> (NamedTempFile, InputSource, usize) {
    let mut temp = TempFileBuilder::new().suffix(suffix).tempfile().unwrap();
    let message = "z".repeat(message_len);
    let mut input_bytes = 0_usize;
    for index in 0..count {
        let record = format!(
            r#"{{"index":{index},"level":"info","message":"{message}","payload":{{"xml":"<root><item id=\"{index}\"><name>visible</name></item></root>","items":[{{"a":1}},{{"b":2}},{{"c":{{"d":true}}}}]}}}}"#
        );
        temp.write_all(record.as_bytes()).unwrap();
        temp.write_all(b"\n").unwrap();
        input_bytes = input_bytes.saturating_add(record.len()).saturating_add(1);
    }
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    (temp, source, input_bytes)
}

const HUGE_STRING_FRAGMENT: &[u8] = br#"<item id=\"1\"><name>visible</name></item>"#;

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
