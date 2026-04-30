use std::{io::Write, time::Duration};

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
