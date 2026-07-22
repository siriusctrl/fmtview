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

fn jsonl_options() -> FormatOptions {
    FormatOptions {
        kind: FormatKind::Jsonl,
        indent: 2,
    }
}

#[test]
fn viewer_display_collapses_valid_data_uri_without_decoding_it() {
    let input = br#"{"content":"data:image/png;base64,iVBORw0KGgo="}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""content": "<media image/png; 8 decoded bytes>""#));
    assert!(!output.contains("iVBORw0KGgo="));
}

#[test]
fn viewer_display_collapses_same_object_base64_media() {
    let input = br#"{"source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""data": "<media image/png; 8 decoded bytes>""#));
}

#[test]
fn viewer_display_labels_invalid_high_confidence_media_without_claiming_a_size() {
    let input = br#"{"content":"data:image/png;base64,not base64!"}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""content": "<media image/png; invalid base64>""#));
    assert!(!output.contains("decoded bytes"));
}

#[test]
fn invalid_media_escapes_are_consumed_through_the_real_string_boundary() {
    let input = br#"{"content":"data:image/png;base64,bad\"still-bad\\tail","after":{"ok":true}}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""content": "<media image/png; invalid base64>""#));
    assert!(output.contains(r#""after": {"#));
    assert!(output.contains(r#""ok": true"#));
}

#[test]
fn sibling_media_metadata_must_precede_data_in_the_same_object() {
    let input = br#"{"source":{"data":"iVBORw0KGgo=","type":"base64","media_type":"image/png"}}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""data": "iVBORw0KGgo=""#));
    assert!(!output.contains("<media"));
}

#[test]
fn viewer_display_keeps_unrelated_strings_and_exact_argument_tokens() {
    let input = br#"{"content":"iVBORw0KGgo=","arguments":"{\"cmd\":\"cargo  test\",\"unicode\":\"\\u0041\"}","artifact":{"future":true}}"#;

    let output = format_record_display_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""content": "iVBORw0KGgo=""#));
    assert!(output.contains(r#""arguments": "{\"cmd\":\"cargo  test\",\"unicode\":\"\\u0041\"}""#));
    assert!(output.contains(r#""artifact": {"#));
    assert!(output.contains(r#""future": true"#));
}

#[test]
fn redirected_record_format_never_collapses_media() {
    let input = br#"{"content":"data:image/png;base64,iVBORw0KGgo="}"#;

    let output = format_record_bytes(input, jsonl_options()).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("data:image/png;base64,iVBORw0KGgo="));
    assert!(!output.contains("<media"));
}
