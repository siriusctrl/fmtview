use std::{
    io::Write as _,
    time::{Duration, Instant},
};

use tempfile::NamedTempFile;

use super::*;
use crate::input::InputSource;

#[test]
fn trims_crlf_line_endings() {
    assert_eq!(trim_record_line_end(b"{\"a\":1}\r\n"), b"{\"a\":1}");
}

#[test]
fn preserves_empty_jsonl_lines() {
    let line = b"\n";
    assert!(trim_record_line_end(line).is_empty());
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_jsonl_record_batch_format() {
    let records = generated_jsonl_records(16_384, 512);
    let input_bytes = records.iter().map(Vec::len).sum::<usize>();
    let started = Instant::now();
    let mut output_bytes = 0_usize;

    for record in &records {
        let rendered = format_record_to_string(record, FormatKind::Jsonl, 2).unwrap();
        output_bytes = output_bytes.saturating_add(rendered.len());
    }

    let elapsed = started.elapsed();
    eprintln!(
        "jsonl record batch format: {elapsed:?}, records={}, input_bytes={input_bytes}, output_bytes={output_bytes}",
        records.len()
    );
    assert!(output_bytes > input_bytes);
    assert!(
        elapsed < Duration::from_secs(5),
        "jsonl record batch format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_jsonl_source_full_format() {
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
    let output_bytes = formatted.as_file().metadata().unwrap().len();

    eprintln!(
        "jsonl source full format: {elapsed:?}, records={}, input_bytes={input_bytes}, output_bytes={output_bytes}",
        records.len()
    );
    assert!(output_bytes > input_bytes as u64);
    assert!(
        elapsed < Duration::from_secs(5),
        "jsonl source full format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_single_huge_object_array_record_format() {
    let items = 32_768;
    let record = generated_huge_object_array_record(items, 128);
    let input_bytes = record.len();
    let started = Instant::now();
    let rendered = format_record_to_string(&record, FormatKind::Jsonl, 2).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = rendered.len();
    let lines = rendered.lines().count();

    eprintln!(
        "single huge object-array record format: {elapsed:?}, records=1, items={items}, lines={lines}, input_bytes={input_bytes}, output_bytes={output_bytes}",
    );
    assert!(lines > items);
    assert!(
        elapsed < Duration::from_secs(5),
        "single huge object-array record format took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/format-performance.sh"]
fn perf_single_huge_string_field_record_format() {
    let repeats = 600_000;
    let record = generated_huge_string_field_record(repeats);
    let input_bytes = record.len();
    let started = Instant::now();
    let rendered = format_record_to_string(&record, FormatKind::Jsonl, 2).unwrap();
    let elapsed = started.elapsed();
    let output_bytes = rendered.len();
    let lines = rendered.lines().count();

    eprintln!(
        "single huge string field record format: {elapsed:?}, records=1, string_bytes={}, lines={lines}, input_bytes={input_bytes}, output_bytes={output_bytes}",
        HUGE_STRING_FRAGMENT.len() * repeats
    );
    assert_eq!(lines, 5);
    assert!(
        elapsed < Duration::from_secs(10),
        "single huge string field record format took {elapsed:?}"
    );
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

const HUGE_STRING_FRAGMENT: &[u8] = br#"<item id=\"1\"><name>visible</name></item>"#;

fn generated_huge_string_field_record(repeats: usize) -> Vec<u8> {
    let mut record = Vec::with_capacity(HUGE_STRING_FRAGMENT.len() * repeats + 128);
    record.extend_from_slice(br#"{"id":1,"kind":"huge-string","message":""#);
    for _ in 0..repeats {
        record.extend_from_slice(HUGE_STRING_FRAGMENT);
    }
    record.extend_from_slice(br#""}"#);
    record
}
