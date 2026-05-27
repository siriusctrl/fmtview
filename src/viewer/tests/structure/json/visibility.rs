use super::*;

#[test]
fn ranked_structure_navigation_keeps_partially_visible_json_blocks() {
    let file = indexed_file(&[
        "{",
        r#"  "small": {"#,
        r#"    "x": 1"#,
        "  },",
        r#"  "large": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 2)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}

#[test]
fn structure_navigation_lands_on_visible_previous_json_landmarks() {
    let file = indexed_file(&[
        "{",
        r#"  "large": {"#,
        r#"    "items": ["#,
        "      {",
        r#"        "id": 1"#,
        "      }",
        "    ]",
        "  },",
        r#"  "small": {"#,
        r#"    "x": 1"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        top: 1,
        structure_cursor: Some(10),
        structure_viewport: Some(structure_viewport(1, 10)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Backward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 3,
            byte_index: 6
        })
    );
}

#[test]
fn ranked_structure_navigation_treats_clipped_nowrap_blocks_as_unseen() {
    let file = indexed_file(&[
        "{",
        r#"  "small": {"#,
        r#"    "long": "abcdefghijklmnopqrstuvwxyz""#,
        "  },",
        r#"  "large": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(StructureViewport {
            wrap: false,
            width: 12,
            ..structure_viewport(0, 3)
        }),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}

#[test]
fn ranked_structure_navigation_keeps_blocks_cut_off_by_wrapping() {
    let file = indexed_file(&[
        "{",
        r#"  "small": {"#,
        r#"    "x": 1"#,
        "  },",
        r#"  "large": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(StructureViewport {
            bottom_line_end: false,
            ..structure_viewport(0, 3)
        }),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}
