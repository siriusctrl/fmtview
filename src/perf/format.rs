use std::{io::Write, time::Instant};

use tempfile::NamedTempFile;

use crate::{
    input::InputSource,
    transform::{FormatKind, FormatOptions, format_record_to_string, format_source_to_temp},
};

use super::{
    fixtures::{
        HUGE_STRING_FRAGMENT, generated_huge_object_array_record,
        generated_huge_string_field_record, generated_json_document_source,
        generated_jsonl_records, generated_xml_document_source,
    },
    runner::{BenchCase, BenchSample},
};

pub(super) const CASES: &[BenchCase] = &[
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
