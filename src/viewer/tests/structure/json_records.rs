use super::*;

#[test]
fn structure_navigation_jumps_inside_jsonl_record_before_next_record() {
    let file = indexed_file(&[
        "{",
        r#"  "id": 1,"#,
        r#"  "payload": {"#,
        r#"    "items": ["#,
        "      {",
        r#"        "name": "first""#,
        "      },",
        "      {",
        r#"        "name": "second""#,
        "      }",
        "    ]",
        "  }",
        "}",
        "{",
        r#"  "id": 2"#,
        "}",
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
            line: 2,
            byte_index: 2
        })
    );

    assert!(resolve_structure_target_position(
        &mut state,
        &file.read_window(0, file.line_count()).unwrap(),
        8,
        RenderContext {
            gutter_digits: 1,
            chat_gutter: false,
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Json,
        }
    ));
    assert_eq!(state.top, 2);
    assert!(state.structure_target.is_none());

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
            byte_index: 4
        })
    );
}
