use super::*;

#[test]
fn bracket_keys_start_structure_navigation_tasks() {
    let mut state = ViewState::default();

    let action = handle_key_event(KeyCode::Char(']'), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert!(state.structure_task.is_some());

    let action = handle_key_event(KeyCode::Char('['), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert!(state.structure_task.is_none());
    assert_eq!(
        state.search_message.as_deref(),
        Some("no previous structure")
    );
}

#[test]
fn manual_scroll_resets_structure_repeat_anchor() {
    let file = indexed_file(&["{", r#"  "a": {"#, "  },", r#"  "b": {"#, "  }", "}"]);
    let mut state = ViewState::default();

    start_structure_navigation(
        &mut state,
        file.line_count(),
        file.line_count_exact(),
        StructureDirection::Forward,
    );
    assert!(process_structure_step(&file, &mut state, FormatKind::Json).unwrap());
    assert_eq!(state.structure_cursor, Some(1));

    handle_key_event(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
        &mut state,
        file.line_count(),
        5,
    );
    assert_eq!(state.structure_cursor, None);

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
            line: 3,
            byte_index: 2
        })
    );
}
