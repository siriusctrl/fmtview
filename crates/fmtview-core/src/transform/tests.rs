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

fn display_json(input: &[u8]) -> String {
    String::from_utf8(format_record_display_bytes(input, jsonl_options()).unwrap()).unwrap()
}

#[test]
fn sibling_media_requires_explicit_encoding_or_a_typed_image_shape() {
    for input in [
        br#"{"source":{"media_type":"image/png","data":"deadbeef"}}"#.as_slice(),
        br#"{"source":{"media_type":"image/png","encoding":"utf8","data":"AAAAAAAAAAAAAAAA"}}"#,
        br#"{"attachment":{"media_type":"image/png","data":"iVBORw0KGgo="}}"#,
        br#"{"type":"text","attachment":{"media_type":"image/png","data":"iVBORw0KGgo="}}"#,
        br#"{"type":"image","attachment":{"media_type":"image/png","encoding":"utf8","data":"iVBORw0KGgo="}}"#,
    ] {
        let output = display_json(input);
        assert!(!output.contains("<media"), "{output}");
    }

    let same_object =
        display_json(br#"{"type":"image","media_type":"image/png","data":"iVBORw0KGgo="}"#);
    assert!(
        same_object.contains("<media image/png; 8 decoded bytes>"),
        "{same_object}"
    );

    let attachment = display_json(
        br#"{"type":"image","attachment":{"media_type":"image/png","data":"iVBORw0KGgo="}}"#,
    );
    assert!(
        attachment.contains("<media image/png; 8 decoded bytes>"),
        "{attachment}"
    );
}

#[test]
fn escaped_media_metadata_and_payload_use_json_string_semantics() {
    let output = display_json(
        br#"{"t\u0079pe":"base\u00364","media\u005ftype":"image\u002fpng","data":"iVBORw0KGgo\u003d"}"#,
    );

    assert!(
        output.contains("<media image/png; 8 decoded bytes>"),
        "{output}"
    );

    for (payload, expected) in [
        (r"QU\u004aD", "3 decoded bytes"),
        (r"AA\/A", "3 decoded bytes"),
        (r"QU\u0022D", "invalid base64"),
        (r"QU\u000aD", "invalid base64"),
        (r"QU\ud83d\ude80D", "invalid base64"),
    ] {
        let input = format!(r#"{{"content":"data:image/png;base64,{payload}"}}"#);
        let output = display_json(input.as_bytes());
        assert!(
            output.contains(expected),
            "payload={payload:?} output={output}"
        );
    }
}

#[test]
fn base64_alphabet_and_padding_follow_the_declared_encoding() {
    for (payload, expected) in [
        ("TQ==", "1 decoded bytes"),
        ("TQ", "1 decoded bytes"),
        ("TWE=", "2 decoded bytes"),
        ("TWE", "2 decoded bytes"),
        ("AA+/", "3 decoded bytes"),
        ("AA-_", "invalid base64"),
        ("A", "invalid base64"),
        ("AA=A", "invalid base64"),
    ] {
        let input = format!(r#"{{"content":"data:image/png;base64,{payload}"}}"#);
        let output = display_json(input.as_bytes());
        assert!(
            output.contains(expected),
            "payload={payload:?} output={output}"
        );
    }

    for (encoding, payload, expected) in [
        ("base64url", "AA-_", "3 decoded bytes"),
        ("base64url", "AA+/", "invalid base64"),
        ("base64", "AA+/", "3 decoded bytes"),
        ("base64", "AA-_", "invalid base64"),
    ] {
        let input =
            format!(r#"{{"media_type":"image/png","encoding":"{encoding}","data":"{payload}"}}"#);
        let output = display_json(input.as_bytes());
        assert!(
            output.contains(expected),
            "encoding={encoding} payload={payload:?} output={output}"
        );
    }
}

#[test]
fn media_collapse_does_not_rewrite_tool_argument_strings_or_invalid_data_uris() {
    let arguments =
        display_json(br#"{"arguments":"data:image/png;base64,iVBORw0KGgo=","other":"ok"}"#);
    assert!(
        arguments.contains("data:image/png;base64,iVBORw0KGgo="),
        "{arguments}"
    );
    assert!(!arguments.contains("<media"), "{arguments}");

    let invalid_uri = display_json(br#"{"content":"data:not-media;base64,QUJD"}"#);
    assert!(
        invalid_uri.contains("data:not-media;base64,QUJD"),
        "{invalid_uri}"
    );
    assert!(!invalid_uri.contains("<media"), "{invalid_uri}");

    let escaped_header = display_json(br#"{"content":"data:image\u002fpng;base64,iVBORw0KGgo="}"#);
    assert!(
        escaped_header.contains(r"data:image\u002fpng;base64,iVBORw0KGgo="),
        "{escaped_header}"
    );
    assert!(!escaped_header.contains("<media"), "{escaped_header}");
}
