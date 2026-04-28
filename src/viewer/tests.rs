use std::{
    cell::Cell,
    io::{self, Write},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::highlight::{highlight_json_like, highlight_xml_line};
use super::input::*;
use super::palette::*;
use super::render::*;
use super::*;

// Correctness tests run by default and should avoid wall-clock assertions.

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
            mode: ViewMode::Plain,
        },
    )
    .remove(0);
    assert_eq!(span_text(&line.spans), r#" 12 │   "name": "fmtview","#);
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
            mode: ViewMode::Plain,
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
            mode: ViewMode::Plain,
        },
    );

    assert_eq!(span_text(&lines[0].spans), "1 │ cde");
}

#[test]
fn mouse_wheel_scrolls_by_logical_line() {
    let mut state = ViewState::default();
    let action = handle_event(
        mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.top, MOUSE_SCROLL_LINES);

    let action = handle_event(
        mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.top, 0);
}

#[test]
fn down_scrolls_inside_overflowing_wrapped_line_first() {
    let mut state = ViewState {
        top_max_row_offset: 2,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, 1);
    assert!(!state.wrap_bounds_stale);

    state.top_row_offset = state.top_max_row_offset;
    let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 1);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn batched_scroll_stops_after_crossing_to_unmeasured_wrapped_line() {
    let mut state = ViewState::default();

    assert!(scroll_down_by(&mut state, 10, 3));

    assert_eq!(state.top, 1);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn up_from_logical_line_moves_to_previous_line_tail() {
    let mut state = ViewState {
        top: 1,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::Up, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, TAIL_ROW_OFFSET);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn viewport_can_start_inside_wrapped_logical_line() {
    let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let first = render_viewport(&lines, 1, 0, 2, request, &mut cache, None);
    assert_eq!(first.last_line_number, Some(1));
    assert_eq!(span_text(&first.lines[0].spans), "1 │ abcd");
    assert_eq!(span_text(&first.lines[1].spans), "  ┆ efgh");

    let second = render_viewport(&lines, 1, 1, 2, request, &mut cache, None);
    assert_eq!(second.last_line_number, Some(1));
    assert_eq!(span_text(&second.lines[0].spans), "  ┆ efgh");
    assert_eq!(span_text(&second.lines[1].spans), "  ┆ ijkl");
}

#[test]
fn viewport_reports_actual_last_logical_line() {
    let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(&lines, 1, 2, 3, request, &mut cache, None);

    assert_eq!(viewport.last_line_number, Some(2));
    assert_eq!(span_text(&viewport.lines[0].spans), "  ┆ ijkl");
    assert_eq!(span_text(&viewport.lines[1].spans), "2 │ next");
}

#[test]
fn wrapped_progress_advances_by_visible_bytes() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "abcdefghijkl").unwrap();
    writeln!(temp, "next").unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };

    assert_eq!(
        viewer_progress_percent(
            &file,
            context,
            1,
            Some(ViewportBottom {
                line_index: 0,
                byte_end: 8,
                line_end: false,
            }),
        ),
        44
    );

    assert_eq!(
        viewer_progress_percent(
            &file,
            context,
            1,
            Some(ViewportBottom {
                line_index: 0,
                byte_end: 12,
                line_end: true,
            }),
        ),
        72
    );

    assert_eq!(
        viewer_progress_percent(
            &file,
            context,
            2,
            Some(ViewportBottom {
                line_index: 1,
                byte_end: 4,
                line_end: true,
            }),
        ),
        100
    );
}

#[test]
fn tail_position_keeps_nowrap_last_page_full() {
    assert_eq!(last_full_logical_page_top(10, 3), 7);
    assert_eq!(last_full_logical_page_top(2, 5), 0);
}

#[test]
fn wrapped_tail_position_can_start_inside_last_line() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "prev").unwrap();
    writeln!(temp, "abcdefghijkl").unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };

    let tail = compute_tail_position(&file, 2, context).unwrap();

    assert_eq!(
        tail,
        ViewPosition {
            top: 1,
            row_offset: 1
        }
    );
}

#[test]
fn wrapped_tail_view_renders_last_full_page() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "{{").unwrap();
    writeln!(temp, "abcdefghijkl").unwrap();
    writeln!(temp, "}}").unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let tail = compute_tail_position(&file, 3, context).unwrap();
    let lines = file.read_window(tail.top, 3).unwrap();
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        tail.top + 1,
        tail.row_offset,
        3,
        request,
        &mut cache,
        None,
    );

    assert_eq!(
        tail,
        ViewPosition {
            top: 1,
            row_offset: 1
        }
    );
    assert_eq!(viewport.lines.len(), 3);
    assert_eq!(viewport.last_line_number, Some(3));
    assert!(viewport_reaches_file_end(&viewport, file.line_count()));
    assert!(
        tail.row_offset > top_line_tail_offset(tail.top + 1, 3, context, &cache),
        "global file tail may need a deeper offset than the top line's own full-page tail"
    );
    assert_eq!(
        effective_top_row_offset(tail.top + 1, 3, context, &cache, Some(tail)),
        tail.row_offset
    );
    assert_eq!(span_text(&viewport.lines[0].spans), "  ┆ efgh");
    assert_eq!(span_text(&viewport.lines[1].spans), "  ┆ ijkl");
    assert_eq!(span_text(&viewport.lines[2].spans), "3 │ }");
}

#[test]
fn eof_wrap_offset_clamps_to_last_full_page() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "abcdefghijkl").unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let tail = compute_tail_position(&file, 2, context).unwrap();
    let lines = file.read_window(0, 2).unwrap();
    let mut cache = RenderedLineCache::default();

    let partial = render_viewport(&lines, 1, 2, 2, request, &mut cache, None);
    let max_offset = effective_top_row_offset(1, 2, context, &cache, Some(tail));
    let clamped = render_viewport(&lines, 1, max_offset, 2, request, &mut cache, None);
    let progress = viewer_progress_percent(&file, context, 1, clamped.bottom);

    assert_eq!(
        tail,
        ViewPosition {
            top: 0,
            row_offset: 1
        }
    );
    assert!(viewport_reaches_file_end(&partial, file.line_count()));
    assert_eq!(partial.lines.len(), 1);
    assert_eq!(max_offset, 1);
    assert_eq!(clamped.lines.len(), 2);
    assert_eq!(progress, 100);
    assert_eq!(span_text(&clamped.lines[0].spans), "  ┆ efgh");
    assert_eq!(span_text(&clamped.lines[1].spans), "  ┆ ijkl");
}

#[test]
fn page_down_clamps_to_known_wrapped_tail() {
    let mut state = ViewState {
        top_max_row_offset: 5,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::PageDown, KeyModifiers::NONE, &mut state, 3, 10);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, 5);
}

#[test]
fn top_line_tail_offset_points_to_last_full_view() {
    let lines = ["abcdefghijklmnop".to_owned()];
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    cache.get_or_render_window(&lines[0], 1, 0, 8, request);

    assert_eq!(top_line_tail_offset(1, 2, context, &cache), 2);
}

#[test]
fn unknown_wrapped_tail_keeps_scrolling_inside_current_line() {
    let line = "a".repeat((WRAP_RENDER_CHUNK_ROWS + 10) * 4);
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: ViewMode::Plain,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    cache.get_or_render_window(&line, 1, 0, 8, request);
    let mut state = ViewState {
        top_max_row_offset: top_line_tail_offset(1, 2, context, &cache),
        ..ViewState::default()
    };

    assert_eq!(state.top_max_row_offset, usize::MAX);
    assert!(scroll_down_by(&mut state, 2, WRAP_RENDER_CHUNK_ROWS + 1));
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, WRAP_RENDER_CHUNK_ROWS + 1);
    assert!(!state.wrap_bounds_stale);
}

#[test]
fn footer_wrap_hint_matches_current_mode() {
    let state = ViewState::default();
    assert!(idle_footer_text(&state).contains("w unwrap"));

    let state = ViewState {
        wrap: false,
        ..ViewState::default()
    };
    assert!(idle_footer_text(&state).contains("w wrap"));
}

#[test]
fn wrap_position_appears_in_mode_and_footer() {
    let state = ViewState {
        top_row_offset: 12_480,
        ..ViewState::default()
    };

    assert_eq!(display_mode_text(&state), "wrap +12,480 rows");
    assert!(idle_footer_text(&state).starts_with(" +12,480 rows | "));
}

#[test]
fn end_key_targets_wrapped_file_tail_even_on_last_line() {
    let mut state = ViewState::default();

    let action = handle_key_event(KeyCode::End, KeyModifiers::NONE, &mut state, 1, 10);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, TAIL_ROW_OFFSET);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn digits_plus_enter_jumps_to_line_number() {
    let mut state = ViewState::default();

    handle_key_event(KeyCode::Char('1'), KeyModifiers::NONE, &mut state, 100, 10);
    handle_key_event(KeyCode::Char('2'), KeyModifiers::NONE, &mut state, 100, 10);
    assert_eq!(state.jump_buffer, "12");
    assert_eq!(state.top, 0);

    let action = handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 100, 10);

    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.jump_buffer, "");
    assert_eq!(state.top, 11);
}

#[test]
fn line_jump_clamps_to_valid_range() {
    let mut state = ViewState::default();

    for ch in "999".chars() {
        handle_key_event(KeyCode::Char(ch), KeyModifiers::NONE, &mut state, 5, 10);
    }
    handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 5, 10);
    assert_eq!(state.top, 4);

    handle_key_event(KeyCode::Char('0'), KeyModifiers::NONE, &mut state, 5, 10);
    handle_key_event(KeyCode::Enter, KeyModifiers::NONE, &mut state, 5, 10);
    assert_eq!(state.top, 0);
}

#[test]
fn line_jump_supports_backspace_and_escape_cancel() {
    let mut state = ViewState::default();

    handle_key_event(KeyCode::Char('4'), KeyModifiers::NONE, &mut state, 20, 10);
    handle_key_event(KeyCode::Char('2'), KeyModifiers::NONE, &mut state, 20, 10);
    let action = handle_key_event(KeyCode::Backspace, KeyModifiers::NONE, &mut state, 20, 10);
    assert!(action.dirty);
    assert_eq!(state.jump_buffer, "4");

    let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 20, 10);
    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.jump_buffer, "");

    let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 20, 10);
    assert!(!action.dirty);
    assert!(action.quit);
}

#[test]
fn ctrl_d_and_ctrl_u_are_not_bound() {
    let mut state = ViewState {
        top: 10,
        ..ViewState::default()
    };

    let action = handle_key_event(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
        &mut state,
        100,
        20,
    );
    assert!(!action.dirty);
    assert_eq!(state.top, 10);

    let action = handle_key_event(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        &mut state,
        100,
        20,
    );
    assert!(!action.dirty);
    assert_eq!(state.top, 10);
}

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

    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.top, 1);
    assert_eq!(state.search_message.as_deref(), Some("match: needle"));

    handle_key_event(KeyCode::Char('n'), KeyModifiers::NONE, &mut state, 4, 10);
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.top, 3);

    handle_key_event(KeyCode::Char('N'), KeyModifiers::NONE, &mut state, 4, 10);
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.top, 1);
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
    assert_eq!(state.top, 0);

    handle_key_event(
        KeyCode::Char('N'),
        KeyModifiers::NONE,
        &mut state,
        file.line_count(),
        10,
    );
    assert!(process_search_step(&file, &mut state).unwrap());
    assert_eq!(state.top, 2);
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
            mode: ViewMode::Plain,
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
            mode: ViewMode::Plain,
        },
        row_limit: 16,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(&lines, 1, 0, 16, request, &mut cache, None);

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
            mode: ViewMode::Plain,
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

#[test]
fn syntax_palette_uses_muted_indexed_colors() {
    assert_eq!(plain_style().fg, Some(PALETTE_TEXT));
    assert_eq!(plain_style().bg, None);
    assert_eq!(gutter_style().fg, Some(PALETTE_MUTED));
    assert_eq!(key_style().fg, Some(PALETTE_BLUE));
    assert_eq!(string_style().fg, Some(PALETTE_GREEN));
    assert_eq!(number_style().fg, Some(PALETTE_ORANGE));
    assert_eq!(error_style().fg, Some(PALETTE_RED));
    assert_eq!(search_match_bg(), PALETTE_SEARCH_MATCH);
}

#[test]
fn ansi_draw_writes_compact_indexed_colors() {
    let mut cell = ratatui::buffer::Cell::EMPTY;
    cell.set_symbol("x").set_fg(PALETTE_BLUE);
    let mut output = Vec::new();

    draw_cells(&mut output, vec![(0, 0, &cell)]).unwrap();

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("\x1b[38;5;75m"));
    assert!(!output.contains("\x1b[49m"));
}

#[test]
fn shifted_wheel_scrolls_horizontally_in_nowrap() {
    let mut state = ViewState {
        wrap: false,
        ..ViewState::default()
    };
    let action = handle_event(
        mouse_event(MouseEventKind::ScrollDown, KeyModifiers::SHIFT),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.x, MOUSE_HORIZONTAL_COLUMNS);

    let action = handle_event(
        mouse_event(MouseEventKind::ScrollUp, KeyModifiers::SHIFT),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.x, 0);
}

#[test]
fn rendered_line_cache_reuses_until_context_changes() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 3,
            wrap: false,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };

    let first = {
        let rows = cache.get_or_render("abcdef", 1, request);
        span_text(&rows[0].spans)
    };
    assert_eq!(first, "1 │ abc");

    cache.get_or_render("abcdef", 1, request);
    assert_eq!(cache.lines.len(), 1);

    let shifted = RenderRequest {
        context: RenderContext {
            x: 2,
            ..request.context
        },
        ..request
    };
    let second = {
        let rows = cache.get_or_render("abcdef", 1, shifted);
        span_text(&rows[0].spans)
    };

    assert_eq!(second, "1 │ cde");
    assert_eq!(cache.lines.len(), 1);
}

#[test]
fn wrapped_render_cache_reuses_adjacent_rows_from_chunk() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let line = "a".repeat(4096);

    let first = cache.get_or_render_window(&line, 1, 100, 2, request);
    assert_eq!(first.len(), 2);
    assert_eq!(cache.lines.get(&1).unwrap().chunks.len(), 1);

    let second = cache.get_or_render_window(&line, 1, 101, 2, request);
    assert_eq!(second.len(), 2);
    assert_eq!(cache.lines.get(&1).unwrap().chunks.len(), 1);
}

#[test]
fn wrapped_render_cache_records_deep_checkpoints() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 16,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let line = format!(
        r#"  "xml": "<root>{}</root>""#,
        r#"<item><name>visible</name></item>"#.repeat(2_000)
    );

    let rows = cache.get_or_render_window(&line, 1, 3_000, 4, request);
    assert_eq!(rows.len(), 4);

    let cached = cache.lines.get(&1).unwrap();
    assert!(
        cached.index.wrap.checkpoints.len() > 4,
        "deep wrapped render should leave reusable row checkpoints"
    );
    assert!(
        !cached.index.highlight.json_value_strings.is_empty(),
        "deep JSON string render should leave XML state checkpoints"
    );

    let checkpointed =
        cache.get_or_render_window(&line, 1, 3_000 + WRAP_RENDER_CHUNK_ROWS + 8, 4, request);
    assert_eq!(checkpointed.len(), 4);
}

#[test]
fn wrapped_deep_window_keeps_embedded_xml_pair_colors() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 12,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let line = format!(
        r#"  "xml": "{}<root><child>visible</child></root>""#,
        "x".repeat(480)
    );
    let row_start = wrap_ranges(
        &line,
        request.context.width,
        continuation_indent(&line, request.context.width),
        80,
    )
    .iter()
    .position(|range| line[range.start_byte..range.end_byte].contains("<child>"))
    .unwrap();

    let rows = cache.get_or_render_window(&line, 1, row_start, 3, request);
    let spans = rows
        .iter()
        .flat_map(|row| row.line.spans.iter().cloned())
        .collect::<Vec<_>>();
    let child_styles = styles_for_text(&spans, "child");

    assert_eq!(child_styles.len(), 2);
    assert_eq!(child_styles[0], child_styles[1]);
}

#[test]
fn wrapped_deep_window_keeps_prefix_xml_state_for_visible_close_tag() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 12,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: 8,
    };
    let line = format!(
        r#"  "xml": "<root><child>{}</child></root>""#,
        "x".repeat(480)
    );
    let row_start = wrap_ranges(
        &line,
        request.context.width,
        continuation_indent(&line, request.context.width),
        120,
    )
    .iter()
    .position(|range| line[range.start_byte..range.end_byte].contains("</child>"))
    .unwrap();

    let rows = cache.get_or_render_window(&line, 1, row_start, 2, request);
    let spans = rows
        .iter()
        .flat_map(|row| row.line.spans.iter().cloned())
        .collect::<Vec<_>>();
    let child_styles = styles_for_text(&spans, "child");

    assert_eq!(child_styles, vec![xml_depth_style(1)]);
}

#[test]
#[ignore = "performance smoke; run with cargo test --release perf_huge_wrapped_line_paths -- --ignored --nocapture --test-threads=1"]
fn perf_huge_wrapped_line_paths() {
    let message = format!(
        r#"  "message": "<root>{}</root>""#,
        r#"<item id=\"1\"><name>visible</name></item>"#.repeat(600_000)
    );
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 94,
        wrap: true,
        mode: ViewMode::Plain,
    };

    let started = Instant::now();
    let rows = render_logical_line_window_with_status(&message, 5, 0, 27, context);
    let first_window = started.elapsed();
    eprintln!("huge wrapped first-window render: {first_window:?}");
    assert_eq!(rows.rows.len(), 27);
    assert!(
        first_window < Duration::from_millis(1_000),
        "first-window render took {first_window:?}"
    );

    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "{{").unwrap();
    writeln!(temp, r#"  "id": 1,"#).unwrap();
    writeln!(temp, r#"  "kind": "huge-single-line-xml-message","#).unwrap();
    writeln!(temp, r#"  "repeats": 600000,"#).unwrap();
    writeln!(temp, "{message}").unwrap();
    writeln!(temp, "}}").unwrap();
    let file = IndexedTempFile::new("huge".to_owned(), temp).unwrap();

    let started = Instant::now();
    let visible_height = 27;
    let tail = compute_tail_position(&file, visible_height, context).unwrap();
    let tail_elapsed = started.elapsed();
    eprintln!("huge wrapped tail position: {tail_elapsed:?}");
    assert_eq!(tail.top, 4);
    assert!(
        tail_elapsed < Duration::from_millis(1_000),
        "tail position took {tail_elapsed:?}"
    );

    let request = RenderRequest {
        context,
        row_limit: render_row_limit(visible_height),
    };
    let lines = file.read_window(tail.top, visible_height).unwrap();
    let mut cache = RenderedLineCache::default();
    let started = Instant::now();
    let viewport = render_viewport(
        &lines,
        tail.top + 1,
        tail.row_offset,
        visible_height,
        request,
        &mut cache,
        None,
    );
    let tail_render = started.elapsed();
    eprintln!("huge wrapped tail-window render: {tail_render:?}");
    assert_eq!(viewport.lines.len(), visible_height);
    assert_eq!(viewport.last_line_number, Some(6));
    assert!(
        tail_render < Duration::from_millis(1_000),
        "tail-window render took {tail_render:?}"
    );

    let checkpointed_row = tail.row_offset.saturating_sub(WRAP_RENDER_CHUNK_ROWS * 2);
    let started = Instant::now();
    let checkpointed_rows = cache.get_or_render_window(
        &lines[0],
        tail.top + 1,
        checkpointed_row,
        visible_height,
        request,
    );
    let checkpointed_render = started.elapsed();
    eprintln!("huge wrapped checkpointed-window render: {checkpointed_render:?}");
    assert_eq!(checkpointed_rows.len(), visible_height);
    assert!(
        checkpointed_render < Duration::from_millis(200),
        "checkpointed-window render took {checkpointed_render:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/viewer-performance.sh"]
fn perf_repeated_viewport_scroll_render() {
    let mut lines = Vec::new();
    for index in 0..1_200 {
        lines.push(format!(
            r#"{{"index":{index},"level":"debug","message":"scroll performance viewport {}","payload":"<root><item id=\"{index}\">value</item></root>"}}"#,
            "x".repeat(240)
        ));
    }
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 4,
            x: 0,
            width: 96,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: render_row_limit(27),
    };
    let mut cache = RenderedLineCache::default();
    let started = Instant::now();
    let mut rendered_rows = 0_usize;
    let mut background_cells = 0_usize;

    for top in 0..400 {
        let viewport = render_viewport(&lines[top..], top + 1, 0, 27, request, &mut cache, None);
        rendered_rows += viewport.lines.len();
        background_cells += background_cell_count(&viewport.lines);
    }

    let elapsed = started.elapsed();
    eprintln!(
        "repeated viewport scroll render: {elapsed:?}, rows={rendered_rows}, background_cells={background_cells}"
    );
    assert_eq!(
        background_cells, 0,
        "non-search scrolling should not repaint styled background cells"
    );
    assert!(
        elapsed < Duration::from_millis(750),
        "repeated viewport render took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/viewer-performance.sh"]
fn perf_terminal_scroll_draw_bytes() {
    let mut lines = Vec::new();
    for index in 0..1_200 {
        lines.push(format!(
            r#"{{"index":{index},"level":"debug","message":"scroll performance viewport {}","payload":"<root><item id=\"{index}\">value</item></root>"}}"#,
            "x".repeat(240)
        ));
    }
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 4,
            x: 0,
            width: 111,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: render_row_limit(32),
    };
    let byte_count = Rc::new(Cell::new(0_usize));
    let writer = CountingWriter {
        bytes: Rc::clone(&byte_count),
    };
    let backend = CrosstermBackend::new(writer);
    let mut terminal = ViewerTerminal::new(backend);
    let area = Rect::new(0, 0, 120, 35);
    let mut cache = RenderedLineCache::default();
    let started = Instant::now();
    let mut rendered_rows = 0_usize;
    let mut background_cells = 0_usize;

    for top in 0..400 {
        let viewport = render_viewport(&lines[top..], top + 1, 0, 32, request, &mut cache, None);
        rendered_rows += viewport.lines.len();
        background_cells += background_cell_count(&viewport.lines);
        let body_lines = viewport.lines;
        let position = ViewPosition { top, row_offset: 0 };
        terminal
            .draw(
                area,
                body_lines,
                " perf ".to_owned(),
                " q/Esc quit ".to_owned(),
                position,
                terminal.scroll_hint(position),
            )
            .unwrap();
    }

    let elapsed = started.elapsed();
    eprintln!(
        "terminal scroll draw: {elapsed:?}, rows={rendered_rows}, bytes={}, background_cells={background_cells}",
        byte_count.get()
    );
    assert!(byte_count.get() > 0);
    assert!(
        elapsed < Duration::from_millis(1_500),
        "terminal scroll draw took {elapsed:?}"
    );
}

#[test]
#[ignore = "performance smoke; run benches/viewer-performance.sh"]
fn perf_terminal_visual_row_scroll_bytes() {
    let message = (0..2_000)
        .map(|index| format!("chunk-{index:04}-abcdefghijklmnopqrstuvwxyz;"))
        .collect::<String>();
    let line = format!(
        r#"{{"index":0,"level":"debug","message":"{}","payload":"<root><item id=\"0\">value</item></root>"}}"#,
        message
    );
    let lines = vec![line];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 4,
            x: 0,
            width: 111,
            wrap: true,
            mode: ViewMode::Plain,
        },
        row_limit: render_row_limit(32),
    };
    let byte_count = Rc::new(Cell::new(0_usize));
    let writer = CountingWriter {
        bytes: Rc::clone(&byte_count),
    };
    let backend = CrosstermBackend::new(writer);
    let mut terminal = ViewerTerminal::new(backend);
    let area = Rect::new(0, 0, 120, 35);
    let mut cache = RenderedLineCache::default();
    let started = Instant::now();
    let mut rendered_rows = 0_usize;
    let mut background_cells = 0_usize;

    for row_offset in 0..400 {
        let viewport = render_viewport(&lines, 1, row_offset, 32, request, &mut cache, None);
        rendered_rows += viewport.lines.len();
        background_cells += background_cell_count(&viewport.lines);
        terminal
            .draw(
                area,
                viewport.lines,
                " perf ".to_owned(),
                " q/Esc quit ".to_owned(),
                ViewPosition { top: 0, row_offset },
                terminal.scroll_hint(ViewPosition { top: 0, row_offset }),
            )
            .unwrap();
    }

    let elapsed = started.elapsed();
    eprintln!(
        "terminal visual row scroll: {elapsed:?}, rows={rendered_rows}, bytes={}, background_cells={background_cells}",
        byte_count.get()
    );
    assert!(byte_count.get() > 0);
    assert_eq!(
        background_cells, 0,
        "non-search scrolling should not repaint styled background cells"
    );
    assert!(
        elapsed < Duration::from_millis(1_500),
        "terminal visual row scroll took {elapsed:?}"
    );
}

#[test]
fn json_highlight_preserves_visible_text() {
    let spans = highlight_json_like(r#"  "ok": true, "n": 42, "none": null"#);
    assert_eq!(span_text(&spans), r#"  "ok": true, "n": 42, "none": null"#);
}

#[test]
fn json_string_escape_tokens_are_highlighted() {
    let spans = highlight_json_like(r#"  "text": "line\nnext\t\u263A\\done""#);
    assert_eq!(span_text(&spans), r#"  "text": "line\nnext\t\u263A\\done""#);

    assert_eq!(styles_for_text(&spans, r#"\n"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\t"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\u263A"#), vec![escape_style()]);
    assert_eq!(styles_for_text(&spans, r#"\\"#), vec![escape_style()]);
}

#[test]
fn xml_highlight_preserves_visible_text() {
    let spans = highlight_xml_line(r#"<root id="1"><child>value</child></root>"#);
    assert_eq!(
        span_text(&spans),
        r#"<root id="1"><child>value</child></root>"#
    );
}

#[test]
fn embedded_xml_string_uses_tag_pairing() {
    let spans = highlight_json_like(r#"  "xml": "<root><child id=\"1\">v</child></root>""#);
    assert_eq!(
        span_text(&spans),
        r#"  "xml": "<root><child id=\"1\">v</child></root>""#
    );

    let root_styles = styles_for_text(&spans, "root");
    assert_eq!(root_styles.len(), 2);
    assert_eq!(root_styles[0], root_styles[1]);

    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles.len(), 2);
    assert_eq!(child_styles[0], child_styles[1]);
    assert_ne!(root_styles[0], child_styles[0]);
    assert_eq!(
        styles_for_text(&spans, r#"\""#),
        vec![escape_style(), escape_style()]
    );
}

#[test]
fn mismatched_inline_xml_tag_is_marked() {
    let spans = highlight_json_like(r#"  "xml": "<root></child>""#);
    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles, vec![error_style()]);
}

#[test]
fn unmatched_inline_xml_close_tag_is_marked() {
    let spans = highlight_json_like(r#"  "xml": "</child>""#);
    let child_styles = styles_for_text(&spans, "child");
    assert_eq!(child_styles, vec![error_style()]);
}

fn span_text(spans: &[Span<'static>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn background_cell_count(lines: &[Line<'static>]) -> usize {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.style.bg.is_some())
        .map(|span| span.content.chars().count())
        .sum()
}

struct CountingWriter {
    bytes: Rc<Cell<usize>>,
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.set(self.bytes.get().saturating_add(buf.len()));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
    spans
        .iter()
        .filter(|span| span.content.as_ref() == text)
        .map(|span| span.style)
        .collect()
}

fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> Event {
    Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers,
    })
}

fn indexed_lines(lines: &[&str]) -> IndexedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(temp, "{line}").unwrap();
    }
    IndexedTempFile::new("test".to_owned(), temp).unwrap()
}
