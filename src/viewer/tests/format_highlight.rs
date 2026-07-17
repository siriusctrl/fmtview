use super::*;

#[test]
fn json_highlight_preserves_visible_text() {
    let spans = highlight_json_like(r#"  "ok": true, "n": 42, "none": null"#);
    assert_eq!(span_text(&spans), r#"  "ok": true, "n": 42, "none": null"#);
}

#[test]
fn json_string_escape_tokens_are_highlighted() {
    let spans = highlight_json_like(r#"  "text": "line\nnext\t\u263A\\done""#);
    assert_eq!(span_text(&spans), r#"  "text": "line\nnext\t\u263A\\done""#);

    assert_eq!(styles_for_text(&spans, r#"\n"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\t"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\u263A"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\\"#), vec![escape_style()]);
}

#[test]
fn json_chat_role_values_get_role_styles() {
    let spans = highlight_json_like(
        r#""role": "system", "role": "user", "role": "assistant", "role": "tool", "content": "assistant""#,
    );

    let system_style = styles_for_text(&spans, r#""system""#);
    let user_style = styles_for_text(&spans, r#""user""#);
    let assistant_style = styles_for_text(&spans, r#""assistant""#);
    let tool_style = styles_for_text(&spans, r#""tool""#);
    assert_eq!(system_style.len(), 1);
    assert_eq!(user_style.len(), 1);
    assert_eq!(assistant_style.len(), 1);
    assert_eq!(tool_style.len(), 1);
    assert_ne!(system_style[0], string_style());
    assert_ne!(user_style[0], string_style());
    assert_ne!(assistant_style[0], string_style());
    assert_ne!(tool_style[0], string_style());
    assert_ne!(system_style[0], user_style[0]);
    assert_ne!(system_style[0], assistant_style[0]);
    assert_ne!(user_style[0], assistant_style[0]);
    assert_ne!(tool_style[0], system_style[0]);
    assert_ne!(tool_style[0], user_style[0]);
    assert_ne!(tool_style[0], assistant_style[0]);

    assert_eq!(styles_for_text(&spans, "assistant"), vec![string_style()]);
}

#[test]
fn xml_highlight_preserves_visible_text() {
    let spans = highlight_xml_line(r#"<root id="1"><child>value</child></root>"#);
    assert_eq!(
        span_text(&spans),
        r#"<root id="1"><child>value</child></root>"#
    );
}

#[test]
fn embedded_xml_string_uses_tag_pairing() {
    let spans = highlight_json_like(r#"  "xml": "<root><child id=\"1\">v</child></root>""#);
    assert_eq!(
        span_text(&spans),
        r#"  "xml": "<root><child id=\"1\">v</child></root>""#
    );

    let root_styles = styles_for_text(&spans, "root");
    assert_eq!(root_styles.len(), 2);
    assert_eq!(root_styles[0], root_styles[1]);

    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles.len(), 2);
    assert_eq!(child_styles[0], child_styles[1]);
    assert_ne!(root_styles[0], child_styles[0]);
    assert_eq!(
        styles_for_text(&spans, r#"\""#),
        vec![escape_style(), escape_style()]
    );
}

#[test]
fn mismatched_inline_xml_tag_is_marked() {
    let spans = highlight_json_like(r#"  "xml": "<root></child>""#);
    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles, vec![error_style()]);
}

#[test]
fn unmatched_inline_xml_close_tag_is_marked() {
    let spans = highlight_json_like(r#"  "xml": "</child>""#);
    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles, vec![error_style()]);
}
