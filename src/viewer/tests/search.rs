use super::*;

#[test]
fn slash_search_finds_and_repeats_matches() {
    let file = indexed_lines(&["alpha", "beta needle", "gamma", "needle again"]);
    let mut state = ViewState::default();

    handle_key_event(KeyCode::Char('/'), KeyModifiers::NONE, &mut state, 4, 10);
    for ch in "needle".chars() {
        handle_key_event(KeyCode::Char(ch), KeyModifiers::NONE, &mut state, 4, 10);
    }
    assert!(state.search_active);

    handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 4, 10);
    assert!(!state.search_active);
    assert_eq!(state.search_query, "needle");
    assert!(state.search_task.is_some());
    assert!(process_search_index_step(&file, &mut state).unwrap());

    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(1));
    assert_eq!(state.search_match_ordinal, Some(1));
    assert_eq!(state.search_message.as_deref(), Some("match: needle"));

    handle_key_event(KeyCode::Char('n'), KeyModifiers::NONE, &mut state, 4, 10);
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(3));
    assert_eq!(state.search_match_ordinal, Some(2));

    handle_key_event(KeyCode::Char('N'), KeyModifiers::NONE, &mut state, 4, 10);
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(1));
    assert_eq!(state.search_match_ordinal, Some(1));
}

#[test]
fn search_match_index_counts_occurrences_lazily() {
    let file = indexed_lines(&[
        "needle one needle two",
        "middle",
        "needle three",
        "tail needle four",
    ]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        0,
        file.line_count(),
    );

    assert!(process_search_index_step(&file, &mut state).unwrap());
    let index = state.search_index.as_ref().unwrap();
    assert_eq!(index.matches, 4);
    assert_eq!(index.counted_lines, 4);
    assert!(index.exact);
    assert_eq!(search_count_text(&state).as_deref(), Some("4 matches"));
}

#[test]
fn search_count_text_shows_current_match_ordinal() {
    let file = indexed_lines(&[
        "needle one needle two",
        "middle",
        "needle three",
        "tail needle four",
    ]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        2,
        file.line_count(),
    );

    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(2));
    assert_eq!(state.search_match_ordinal, None);
    assert!(process_search_index_step(&file, &mut state).unwrap());
    assert_eq!(state.search_match_ordinal, Some(3));

    assert_eq!(search_count_text(&state).as_deref(), Some("3/4 matches"));
}

#[test]
fn search_ordinal_does_not_scan_unindexed_prefix() {
    struct SparseSearchFile {
        reads_before_target: Cell<usize>,
    }

    impl ViewFile for SparseSearchFile {
        fn label(&self) -> &str {
            "sparse"
        }

        fn line_count(&self) -> usize {
            1_000
        }

        fn byte_len(&self) -> u64 {
            0
        }

        fn byte_offset_for_line(&self, _line: usize) -> u64 {
            0
        }

        fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
            if start < 900 {
                self.reads_before_target
                    .set(self.reads_before_target.get().saturating_add(1));
            }
            Ok((start..start.saturating_add(count).min(self.line_count()))
                .map(|line| {
                    if line == 900 {
                        "needle".to_owned()
                    } else {
                        "line".to_owned()
                    }
                })
                .collect())
        }
    }

    let file = SparseSearchFile {
        reads_before_target: Cell::new(0),
    };
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        900,
        file.line_count(),
    );

    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(900));
    assert_eq!(state.search_match_ordinal, None);
    assert_eq!(file.reads_before_target.get(), 0);
}

#[test]
fn search_count_text_keeps_lazy_suffix_with_current_match_ordinal() {
    let file = indexed_lines(&["needle one", "needle two", "needle three"]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        1,
        file.line_count(),
    );

    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_match_ordinal, None);
    state.search_match_ordinal = Some(2);
    let index = state.search_index.as_mut().unwrap();
    index.matches = 2;
    index.counted_lines = 2;
    index.line_match_totals = vec![1, 2];
    index.exact = false;

    assert_eq!(search_count_text(&state).as_deref(), Some("2/2+ matches"));
}

#[test]
fn search_match_index_keeps_lazy_suffix_for_incomplete_files() {
    struct IncompleteViewFile {
        lines: Vec<String>,
    }

    impl ViewFile for IncompleteViewFile {
        fn label(&self) -> &str {
            "lazy"
        }

        fn line_count(&self) -> usize {
            self.lines.len()
        }

        fn line_count_exact(&self) -> bool {
            false
        }

        fn byte_len(&self) -> u64 {
            0
        }

        fn byte_offset_for_line(&self, _line: usize) -> u64 {
            0
        }

        fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
            Ok(self.lines.iter().skip(start).take(count).cloned().collect())
        }
    }

    let file = IncompleteViewFile {
        lines: vec!["needle".to_owned(), "needle needle".to_owned()],
    };
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        0,
        file.line_count(),
    );

    assert!(process_search_index_step(&file, &mut state).unwrap());
    let index = state.search_index.as_ref().unwrap();
    assert_eq!(index.matches, 3);
    assert_eq!(index.counted_lines, 2);
    assert!(!index.exact);
    assert_eq!(search_count_text(&state).as_deref(), Some("3+ matches"));
}

#[test]
fn search_jump_places_later_logical_line_with_context() {
    let file = indexed_lines(&[
        "line 1",
        "line 2",
        "line 3",
        "line 4",
        "line 5",
        "line 6",
        "line 7",
        "line 8",
        "line 9",
        "line 10",
        "line 11 needle",
    ]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        0,
        file.line_count(),
    );
    assert!(process_search_step(&file, &mut state).unwrap());

    let lines = file.read_window(state.top, 32).unwrap();
    assert!(resolve_search_target_position(
        &mut state,
        &lines,
        9,
        RenderContext {
            gutter_digits: 2,
            x: 0,
            width: 40,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    ));

    assert_eq!(state.top, 7);
    assert!(state.search_target.is_some());

    let lines = file.read_window(state.top, 32).unwrap();
    assert!(!resolve_search_target_position(
        &mut state,
        &lines,
        9,
        RenderContext {
            gutter_digits: 2,
            x: 0,
            width: 40,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    ));
    assert_eq!(state.top, 7);
    assert_eq!(state.search_target, None);
}

#[test]
fn wrapped_search_jumps_to_visual_row_containing_match() {
    let line = format!("{}needle suffix", "a".repeat(140));
    let file = indexed_lines(&[line.as_str()]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Forward,
        0,
        file.line_count(),
    );
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.top, 0);
    assert_eq!(
        state.search_target,
        Some(SearchTarget {
            line: 0,
            byte_index: line.find("needle").unwrap()
        })
    );

    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 20,
        wrap: true,
        mode: SyntaxKind::Structured,
    };
    let lines = file.read_window(state.top, 1).unwrap();
    let target_row = visual_row_for_byte(&line, line.find("needle").unwrap(), context);
    assert!(!resolve_search_target_position(
        &mut state, &lines, 4, context
    ));

    assert!(state.top_row_offset > 0);
    assert_eq!(state.top_row_offset, target_row - search_context_rows(4));

    let request = RenderRequest {
        context,
        row_limit: render_row_limit(4),
    };
    let mut cache = RenderedLineCache::default();
    let viewport = render_viewport(
        &lines,
        state.top + 1,
        state.top_row_offset,
        4,
        request,
        &mut cache,
        ViewportRenderOptions {
            line_modes: None,
            search_query: Some("needle"),
        },
    );

    assert!(viewport.lines.iter().any(|line| {
        line.spans.iter().any(|span| {
            span.content.as_ref() == "needle" && span.style.bg == Some(search_match_bg())
        })
    }));
}

#[test]
fn wrapped_search_keeps_visible_match_position() {
    let line = format!("{}needle suffix", "a".repeat(140));
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 20,
        wrap: true,
        mode: SyntaxKind::Structured,
    };
    let target_row = visual_row_for_byte(&line, line.find("needle").unwrap(), context);
    let mut state = ViewState {
        top_row_offset: target_row.saturating_sub(1),
        search_target: Some(SearchTarget {
            line: 0,
            byte_index: line.find("needle").unwrap(),
        }),
        ..ViewState::default()
    };

    assert!(!resolve_search_target_position(
        &mut state,
        &[line],
        4,
        context
    ));

    assert_eq!(state.top_row_offset, target_row.saturating_sub(1));
}

#[test]
fn backward_search_targets_last_match_on_matching_line() {
    let line = "needle first then needle last";
    let file = indexed_lines(&[line]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "needle".to_owned(),
        SearchDirection::Backward,
        0,
        file.line_count(),
    );
    assert!(process_search_step(&file, &mut state).unwrap());

    assert_eq!(
        state.search_target,
        Some(SearchTarget {
            line: 0,
            byte_index: line.rfind("needle").unwrap()
        })
    );
}

#[test]
fn search_reports_not_found_and_can_clear_message() {
    let file = indexed_lines(&["alpha", "beta"]);
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "missing".to_owned(),
        SearchDirection::Forward,
        0,
        file.line_count(),
    );
    assert!(process_search_step(&file, &mut state).unwrap());

    assert_eq!(state.top, 0);
    assert_eq!(state.search_message.as_deref(), Some("not found: missing"));

    let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 2, 10);
    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.search_message, None);
}

#[test]
fn backward_search_does_not_rearm_incomplete_lazy_prefix() {
    struct IncompleteViewFile {
        lines: Vec<String>,
    }

    impl ViewFile for IncompleteViewFile {
        fn label(&self) -> &str {
            "lazy"
        }

        fn line_count(&self) -> usize {
            self.lines.len()
        }

        fn line_count_exact(&self) -> bool {
            false
        }

        fn byte_len(&self) -> u64 {
            0
        }

        fn byte_offset_for_line(&self, _line: usize) -> u64 {
            0
        }

        fn read_window(&self, start: usize, count: usize) -> Result<Vec<String>> {
            Ok(self.lines.iter().skip(start).take(count).cloned().collect())
        }
    }

    let file = IncompleteViewFile {
        lines: vec!["alpha".to_owned(), "beta".to_owned()],
    };
    let mut state = ViewState::default();

    start_search(
        &mut state,
        "missing".to_owned(),
        SearchDirection::Backward,
        1,
        file.line_count(),
    );
    assert!(process_search_step(&file, &mut state).unwrap());

    assert!(state.search_task.is_none());
    assert_eq!(state.search_message.as_deref(), Some("not found: missing"));
}

#[test]
fn repeated_search_wraps_around_file_edges() {
    let file = indexed_lines(&["needle first", "middle", "needle last"]);
    let mut state = ViewState {
        top: 2,
        search_query: "needle".to_owned(),
        ..ViewState::default()
    };

    handle_key_event(
        KeyCode::Char('n'),
        KeyModifiers::NONE,
        &mut state,
        file.line_count(),
        10,
    );
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(0));

    handle_key_event(
        KeyCode::Char('N'),
        KeyModifiers::NONE,
        &mut state,
        file.line_count(),
        10,
    );
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.search_cursor, Some(2));
}

#[test]
fn search_highlight_adds_background_without_replacing_foreground() {
    let line = render_logical_line(
        r#"  "needle": "needle","#,
        1,
        1,
        RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    )
    .remove(0);

    let highlighted = apply_search_highlight(line, Some("needle"), 1);
    let styles = styles_for_text(&highlighted.spans, "needle");

    assert_eq!(styles.len(), 2);
    assert!(
        styles
            .iter()
            .all(|style| style.bg == Some(search_match_bg()))
    );
    assert!(styles.iter().any(|style| style.fg == Some(PALETTE_BLUE)));
    assert!(styles.iter().any(|style| style.fg == Some(PALETTE_GREEN)));
}

#[test]
fn non_search_viewport_render_does_not_paint_background_cells() {
    let lines = vec![
        r#"{"kind":"alpha","message":"plain viewport line"}"#.to_owned(),
        r#"{"kind":"beta","message":"another rendered line"}"#.to_owned(),
    ];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: true,
            mode: SyntaxKind::Structured,
        },
        row_limit: 16,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        0,
        16,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );

    assert_eq!(background_cell_count(&viewport.lines), 0);
}

#[test]
fn search_background_is_scoped_to_match_spans_only() {
    let line = render_logical_line(
        r#"  "needle": "needle","#,
        1,
        1,
        RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Structured,
        },
    )
    .remove(0);

    let highlighted = apply_search_highlight(line, Some("needle"), 1);
    let background_spans = highlighted
        .spans
        .iter()
        .filter(|span| span.style.bg == Some(search_match_bg()))
        .collect::<Vec<_>>();

    assert_eq!(background_spans.len(), 2);
    assert!(
        background_spans
            .iter()
            .all(|span| span.content.as_ref() == "needle")
    );
}
