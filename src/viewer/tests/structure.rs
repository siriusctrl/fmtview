use super::*;

fn indexed_file(lines: &[&str]) -> IndexedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(temp, "{line}").unwrap();
    }
    temp.flush().unwrap();
    IndexedTempFile::new("test".to_owned(), temp).unwrap()
}

fn structure_viewport(top: usize, bottom: usize) -> StructureViewport {
    StructureViewport {
        top,
        top_row_offset: 0,
        bottom,
        bottom_line_end: true,
        x: 0,
        width: 80,
        wrap: true,
    }
}

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

#[test]
fn smart_structure_navigation_skips_fully_visible_json_blocks() {
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
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 4,
            byte_index: 2
        })
    );
}

#[test]
fn smart_structure_navigation_keeps_partially_visible_json_blocks() {
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
fn smart_structure_navigation_skips_visible_previous_blocks() {
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
            line: 0,
            byte_index: 0
        })
    );
}

#[test]
fn smart_structure_navigation_treats_clipped_nowrap_blocks_as_unseen() {
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
fn smart_structure_navigation_keeps_blocks_cut_off_by_wrapping() {
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

#[test]
fn smart_structure_navigation_skips_fully_visible_markdown_sections() {
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
            line: 4,
            byte_index: 0
        })
    );
}

#[test]
fn smart_structure_navigation_skips_fully_visible_jinja_blocks() {
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
            line: 4,
            byte_index: 0
        })
    );
}

#[test]
fn bracket_keys_start_structure_navigation_tasks() {
    let mut state = ViewState::default();

    let action = handle_key_event(KeyCode::Char(']'), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert!(state.structure_task.is_some());

    let action = handle_key_event(KeyCode::Char('['), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert!(state.structure_task.is_none());
    assert_eq!(state.search_message.as_deref(), Some("no previous block"));
}

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
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
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
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
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
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 3,
            byte_index: 4
        })
    );
}

#[test]
fn structure_target_moves_even_when_already_visible() {
    let mut state = ViewState {
        structure_target: Some(SearchTarget {
            line: 4,
            byte_index: 0,
        }),
        ..ViewState::default()
    };

    assert!(resolve_structure_target_position(
        &mut state,
        &[
            "{".to_owned(),
            r#"  "a": 1,"#.to_owned(),
            r#"  "b": {"#.to_owned(),
            "  },".to_owned(),
            r#"  "c": {"#.to_owned(),
        ],
        10,
        RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
        }
    ));
    assert_eq!(state.top, 4);
    assert!(state.structure_target.is_none());
}

#[test]
fn structure_target_near_eof_clamps_to_last_full_page() {
    let file = indexed_file(&[
        "{",
        r#"  "a": {"#,
        "  },",
        r#"  "b": {"#,
        "  },",
        r#"  "tail": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_target: Some(SearchTarget {
            line: 5,
            byte_index: 2,
        }),
        ..ViewState::default()
    };
    let mut line_cache = LineWindowCache::default();
    let mut tail_cache = TailPositionCache::default();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 80,
        wrap: false,
        mode: SyntaxKind::Structured,
    };

    let tail = resolve_targets_from_view(
        &file,
        &mut state,
        &mut line_cache,
        4,
        context,
        &mut tail_cache,
    )
    .unwrap();

    assert_eq!(
        tail,
        Some(ViewPosition {
            top: 4,
            row_offset: 0
        })
    );
    assert_eq!(state.top, 4);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.structure_target.is_none());
    assert_eq!(file.read_window(state.top, 4).unwrap().len(), 4);
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
    assert_eq!(state.search_message.as_deref(), Some("no next block"));
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
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
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
    assert!(process_structure_step(&file, &mut state, SyntaxKind::Structured).unwrap());
    assert_eq!(
        state.structure_target,
        Some(SearchTarget {
            line: 3,
            byte_index: 2
        })
    );
}
