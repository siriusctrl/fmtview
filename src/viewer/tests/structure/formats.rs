use super::*;

#[test]
fn structure_navigation_lands_on_visible_markdown_headings() {
    let file = indexed_file(&[
        "# Title",
        "intro",
        "## Visible",
        "body",
        "## Large",
        "detail",
        "still",
        "# Next",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 3)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Markdown).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 2,
            byte_index: 0
        })
    );
}

#[test]
fn structure_navigation_lands_on_visible_jinja_blocks() {
    let file = indexed_file(&[
        "<main>",
        "{% if user %}",
        "  {{ user.name }}",
        "{% endif %}",
        "{% for item in items %}",
        "  {{ item }}",
        "{% endfor %}",
        "</main>",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 3)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Jinja).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 0
        })
    );
}

#[test]
fn structure_navigation_finds_previous_block() {
    let file = indexed_file(&["# Title", "", "text", "## Details", "more", "## Later"]);
    let mut state = ViewState {
        top: 5,
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Backward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Markdown).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 3,
            byte_index: 0
        })
    );
}

#[test]
fn structure_navigation_reports_missing_block() {
    let file = indexed_file(&["plain", "text"]);
    let mut state = ViewState::default();

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Markdown).unwrap());
    assert_eq!(state.search_message.as_deref(), Some("no next structure"));
}
