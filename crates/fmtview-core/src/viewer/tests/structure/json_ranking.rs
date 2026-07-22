use super::*;

#[test]
fn ranked_structure_navigation_skips_fully_visible_json_detail_blocks() {
    let file = indexed_file(&[
        "{",
        r#"  "small": {"#,
        r#"    "x": 1"#,
        "  },",
        r#"  "large": {"#,
        r#"    "nested": {"#,
        r#"      "x": 1"#,
        "    }",
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 4)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 4,
            byte_index: 2
        })
    );
}

#[test]
fn structure_navigation_lands_on_visible_json_array_items() {
    let file = indexed_file(&[
        "[",
        "  {",
        r#"    "id": 1"#,
        "  },",
        "  {",
        r#"    "id": 2"#,
        "  },",
        "  {",
        r#"    "id": 3"#,
        "  }",
        "]",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 10)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );

    assert!(resolve_structure_target_position(
        &mut state,
        &file.read_window(0, file.line_count()).unwrap(),
        10,
        RenderContext {
            gutter: GutterLayout::new(2, false),
            x: 0,
            width: 80,
            wrap: true,
            mode: FormatKind::Json,
        }
    ));
    state.structure_viewport = Some(structure_viewport(1, 10));

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 4,
            byte_index: 2
        })
    );
}

#[test]
fn structure_navigation_prefers_sibling_array_item_over_nested_payload() {
    let file = indexed_file(&[
        "[",
        "  {",
        r#"    "id": 1,"#,
        r#"    "payload": {"#,
        r#"      "nested": {"#,
        r#"        "x": 1"#,
        "      }",
        "    }",
        "  },",
        "  {",
        r#"    "id": 2"#,
        "  }",
        "]",
    ]);
    let mut state = ViewState {
        structure_cursor: Some(1),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 9,
            byte_index: 2
        })
    );

    let mut state = ViewState {
        structure_cursor: Some(9),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Backward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}

#[test]
fn structure_navigation_prefers_json_chat_messages() {
    let file = indexed_file(&[
        "[",
        "  {",
        r#"    "metadata": {"#,
        r#"      "role": "observer""#,
        "    }",
        "  },",
        "  {",
        r#"    "role": "system","#,
        r#"    "content": "rules""#,
        "  },",
        "  {",
        r#"    "message": {"#,
        r#"      "role": "assistant","#,
        r#"      "content": "nested""#,
        "    }",
        "  }",
        "]",
    ]);
    let mut state = ViewState::default();

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 6,
            byte_index: 2
        })
    );

    state.structure_cursor = Some(6);
    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 11,
            byte_index: 4
        })
    );
}

#[test]
fn structure_navigation_treats_tool_objects_as_chat_messages() {
    let file = indexed_file(&[
        "[",
        "  {",
        r#"    "content": "unlabeled""#,
        "  },",
        "  {",
        r#"    "role": "tool","#,
        r#"    "content": "result""#,
        "  }",
        "]",
    ]);
    let mut state = ViewState::default();

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 4,
            byte_index: 2
        })
    );
}

#[test]
fn backward_structure_navigation_prefers_json_chat_messages() {
    let file = indexed_file(&[
        "[",
        "  {",
        r#"    "role": "user","#,
        r#"    "content": "hello""#,
        "  },",
        "  {",
        r#"    "payload": {"#,
        r#"      "id": 1"#,
        "    }",
        "  },",
        "  {",
        r#"    "message": {"#,
        r#"      "role": "assistant""#,
        "    }",
        "  }",
        "]",
    ]);
    let mut state = ViewState {
        structure_cursor: Some(14),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Backward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Jsonl).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 11,
            byte_index: 4
        })
    );

    state.structure_cursor = Some(11);
    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Backward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Jsonl).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}

#[test]
fn structure_navigation_lands_on_large_visible_json_composite_fields() {
    let file = indexed_file(&[
        "{",
        r#"  "payload": {"#,
        r#"    "items": ["#,
        "      {",
        r#"        "id": 1"#,
        "      }",
        "    ]",
        "  },",
        r#"  "tail": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_viewport: Some(structure_viewport(0, 10)),
        ..ViewState::default()
    };

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 1,
            byte_index: 2
        })
    );
}
