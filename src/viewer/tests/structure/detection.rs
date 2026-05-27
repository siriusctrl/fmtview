use super::*;

#[test]
fn structure_points_are_format_specific() {
    assert!(is_structure_point(
        SyntaxKind::Structured,
        r#"  "items": ["#,
        Some(r#"  "id": 1,"#)
    ));
    assert!(is_structure_point(
        SyntaxKind::Structured,
        "    {",
        Some(r#"  "items": ["#)
    ));
    assert!(!is_structure_point(
        SyntaxKind::Structured,
        r#"  "id": 1,"#,
        Some("{")
    ));

    assert!(is_structure_point(
        SyntaxKind::Structured,
        "  <item id=\"1\">",
        Some("<root>")
    ));
    assert!(!is_structure_point(
        SyntaxKind::Structured,
        "  </item>",
        Some("value")
    ));

    assert!(is_structure_point(SyntaxKind::Markdown, "## Details", None));
    assert!(is_structure_point(SyntaxKind::Toml, "[server]", None));
    assert!(is_structure_point(
        SyntaxKind::Jinja,
        "{% for item in items %}",
        None
    ));
    assert!(is_structure_point(
        SyntaxKind::Plain,
        "next paragraph",
        Some("")
    ));
    assert!(!is_structure_point(
        SyntaxKind::Plain,
        "same paragraph",
        Some("previous text")
    ));
}
