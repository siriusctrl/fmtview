use super::*;

#[test]
fn structure_points_are_format_specific() {
    assert!(is_structure_point(
        FormatKind::Json,
        r#"  "items": ["#,
        Some(r#"  "id": 1,"#)
    ));
    assert!(is_structure_point(
        FormatKind::Json,
        "    {",
        Some(r#"  "items": ["#)
    ));
    assert!(!is_structure_point(
        FormatKind::Json,
        r#"  "id": 1,"#,
        Some("{")
    ));

    assert!(is_structure_point(
        FormatKind::Xml,
        "  <item id=\"1\">",
        Some("<root>")
    ));
    assert!(!is_structure_point(
        FormatKind::Xml,
        "  </item>",
        Some("value")
    ));

    assert!(is_structure_point(FormatKind::Markdown, "## Details", None));
    assert!(is_structure_point(FormatKind::Toml, "[server]", None));
    assert!(is_structure_point(
        FormatKind::Jinja,
        "{% for item in items %}",
        None
    ));
    assert!(is_structure_point(
        FormatKind::Plain,
        "next paragraph",
        Some("")
    ));
    assert!(!is_structure_point(
        FormatKind::Plain,
        "same paragraph",
        Some("previous text")
    ));
}
