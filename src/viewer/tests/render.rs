use super::*;

#[test]
fn slices_by_character_not_byte() {
    assert_eq!(slice_chars("a路径b", 1, 3), "路径");
}

#[test]
fn styled_line_keeps_a_gutter() {
    let line = render_logical_line(
        r#"  "name": "fmtview","#,
        12,
        1,
        RenderContext {
            gutter_digits: 3,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    )
    .remove(0);
    assert_eq!(span_text(&line.spans), r#" 12 │   "name": "fmtview","#);
}

#[test]
fn zero_width_gutter_renders_selectable_text_only() {
    let line = render_logical_line(
        r#"  "name": "fmtview","#,
        12,
        1,
        RenderContext {
            gutter_digits: 0,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    )
    .remove(0);

    assert_eq!(span_text(&line.spans), r#"  "name": "fmtview","#);
}

#[test]
fn wrap_uses_continuation_gutter_and_indent() {
    let lines = render_logical_line(
        r#"  "payload": "abcdefghijklmnopqrstuvwxyz","#,
        7,
        3,
        RenderContext {
            gutter_digits: 2,
            x: 0,
            width: 18,
            wrap: true,
            mode: SyntaxKind::Structured,
        },
    );

    assert!(lines.len() > 1);
    assert!(span_text(&lines[0].spans).starts_with(" 7 │ "));
    assert!(span_text(&lines[1].spans).starts_with("   ┆     "));
}

#[test]
fn continuation_gutter_marks_deep_wrapped_offsets() {
    assert_eq!(span_text(&[continuation_gutter(1, 1)]), "  ┆ ");
    assert_eq!(span_text(&[continuation_gutter(8, 1)]), "  ┊ ");
    assert_eq!(span_text(&[continuation_gutter(64, 1)]), "  ┠ ");
}

#[test]
fn nowrap_applies_horizontal_offset() {
    let lines = render_logical_line(
        "abcdef",
        1,
        1,
        RenderContext {
            gutter_digits: 1,
            x: 2,
            width: 3,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    );

    assert_eq!(span_text(&lines[0].spans), "1 │ cde");
}
