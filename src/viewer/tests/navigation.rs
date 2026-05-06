use super::*;

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
        mode: SyntaxKind::Structured,
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
        mode: SyntaxKind::Structured,
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
fn line_jump_on_incomplete_lazy_file_can_target_unloaded_line() {
    let mut state = ViewState::default();

    handle_key_event(KeyCode::Char('6'), KeyModifiers::NONE, &mut state, 1, 10);
    let action =
        handle_key_event_with_count(KeyCode::Enter, KeyModifiers::NONE, &mut state, 1, false, 10);

    assert!(action.dirty);
    assert_eq!(state.top, 5);
}

#[test]
fn incomplete_lazy_file_does_not_clamp_optimistic_jump_before_reading() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, r#"{{"id":1,"payload":{{"a":1,"b":2,"c":3,"d":4}}}}"#).unwrap();
    temp.flush().unwrap();
    let source = InputSource::from_arg(temp.path().to_str().unwrap(), None).unwrap();
    let file = LazyTransformedRecordsFile::new(
        &source,
        FormatOptions {
            kind: FormatKind::Jsonl,
            indent: 2,
        },
    )
    .unwrap();
    assert!(!file.line_count_exact());

    let mut state = ViewState {
        top: 5,
        ..ViewState::default()
    };
    let mut caches = ViewerCaches::default();
    let context = RenderContext {
        gutter_digits: 4,
        x: 0,
        width: 80,
        wrap: false,
        mode: SyntaxKind::Structured,
    };

    adjust_state_for_visible_height(&file, &mut state, 10, context, &mut caches.tail).unwrap();
    let lines = caches.line.read(&file, state.top, 3, 0).unwrap();

    assert_eq!(state.top, 5);
    assert!(!lines.lines.is_empty());
    assert!(lines.lines[0].contains("\"c\""));
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
