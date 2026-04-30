use super::*;

#[test]
fn trims_crlf_line_endings() {
    assert_eq!(trim_record_line_end(b"{\"a\":1}\r\n"), b"{\"a\":1}");
}

#[test]
fn preserves_empty_jsonl_lines() {
    let line = b"\n";
    assert!(trim_record_line_end(line).is_empty());
}
