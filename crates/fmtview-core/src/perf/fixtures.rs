use std::io::Write;

use tempfile::{Builder as TempFileBuilder, NamedTempFile};

use crate::input::InputSource;

pub(super) const HUGE_STRING_FRAGMENT: &[u8] = br#"<item id=\"1\"><name>visible</name></item>"#;

pub(super) fn generated_jsonl_records(count: usize, message_len: usize) -> Vec<Vec<u8>> {
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

pub(super) fn generated_jsonl_source(
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

pub(super) fn generated_huge_object_array_record(items: usize, message_len: usize) -> Vec<u8> {
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

pub(super) fn generated_json_document_source(
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

pub(super) fn generated_xml_document_source(
    items: usize,
) -> (NamedTempFile, InputSource, usize, usize) {
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

pub(super) fn generated_huge_string_field_record(repeats: usize) -> Vec<u8> {
    let mut record = Vec::with_capacity(HUGE_STRING_FRAGMENT.len() * repeats + 128);
    record.extend_from_slice(br#"{"id":1,"kind":"huge-string","message":""#);
    for _ in 0..repeats {
        record.extend_from_slice(HUGE_STRING_FRAGMENT);
    }
    record.extend_from_slice(br#""}"#);
    record
}

pub(super) fn generated_huge_string_jsonl_source(
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
