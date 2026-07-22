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
fn top_line_scroll_limit_reaches_the_last_wrapped_row() {
    let lines = ["abcdefghijklmnop".to_owned()];
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    cache.get_or_render_window(&lines[0], 1, 0, 8, request);

    assert_eq!(top_line_scroll_limit(1, context, &cache), 3);
}

#[test]
fn wrapped_scroll_moves_each_visual_row_past_the_viewport_top() {
    let lines = ["abcdefghijklmnop".to_owned(), "next".to_owned()];
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    cache.get_or_render_window(&lines[0], 1, 0, 8, request);
    let mut state = ViewState {
        top_max_row_offset: top_line_scroll_limit(1, context, &cache),
        ..ViewState::default()
    };

    for expected in 1..=3 {
        assert!(scroll_down(&mut state, lines.len()));
        assert_eq!(state.top, 0);
        assert_eq!(state.top_row_offset, expected);
    }

    assert!(scroll_down(&mut state, lines.len()));
    assert_eq!(state.top, 1);
    assert_eq!(state.top_row_offset, 0);
}

#[test]
fn wrapped_scroll_up_returns_to_the_previous_lines_last_visual_row() {
    let lines = ["abcdefghijklmnop".to_owned(), "next".to_owned()];
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
    };
    let mut state = ViewState {
        top: 1,
        ..ViewState::default()
    };

    assert!(scroll_up(&mut state, lines.len()));
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, LAST_ROW_OFFSET);
    assert_eq!(exact_top_line_scroll_limit(&lines, context), 3);
}

#[test]
fn unknown_wrapped_tail_keeps_scrolling_inside_current_line() {
    let line = "a".repeat((WRAP_RENDER_CHUNK_ROWS + 10) * 4);
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
    };
    let request = RenderRequest {
        context,
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    cache.get_or_render_window(&line, 1, 0, 8, request);
    let mut state = ViewState {
        top_max_row_offset: top_line_scroll_limit(1, context, &cache),
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
    assert!(idle_footer_text(&state).contains("m select"));

    let state = ViewState {
        wrap: false,
        ..ViewState::default()
    };
    assert!(idle_footer_text(&state).contains("w wrap"));
}

#[test]
fn tool_pair_key_round_trips_between_exact_endpoints() {
    use std::sync::Arc;

    use crate::formats::json::tool_links::{ToolLink, ToolLinkStatus};

    let link = ToolLink {
        id: Arc::from("call_7"),
        call_line: Some(3),
        result_line: 10,
        status: ToolLinkStatus::Matched,
    };
    let mut state = ViewState {
        top: 10,
        ..ViewState::default()
    };
    state.set_tool_context(Some((link.clone(), 10)));

    let action = handle_key_event(KeyCode::Char('t'), KeyModifiers::NONE, &mut state, 20, 5);

    assert!(action.dirty);
    assert_eq!(state.tool_target, Some(3));
    assert_eq!(state.tool_selection, Some(link.clone()));

    state.top = 3;
    state.tool_target = None;
    state.set_tool_context(Some((link, 3)));
    handle_key_event(KeyCode::Char('t'), KeyModifiers::NONE, &mut state, 20, 5);
    assert_eq!(state.tool_target, Some(10));
}

#[test]
fn tool_result_footer_shows_pair_context_and_jump_hint() {
    use std::sync::Arc;

    use crate::formats::json::tool_links::{ToolLink, ToolLinkStatus};

    let file = indexed_lines(&["tool result"]);
    let mut state = ViewState {
        follow: Some(FollowState::Paused),
        ..ViewState::default()
    };
    state.set_tool_context(Some((
        ToolLink {
            id: Arc::from("call_123"),
            call_line: Some(4),
            result_line: 12,
            status: ToolLinkStatus::Matched,
        },
        12,
    )));

    let footer = file_footer_text(&file, &state);
    assert!(footer.contains("follow:off"));
    assert!(footer.contains("tool result ↑ call line 5"));
    assert!(footer.contains("id: call_123"));
    assert!(footer.contains("t jump"));
}

#[test]
fn footer_shows_mouse_restore_hint_when_selection_mode_is_active() {
    let state = ViewState {
        mouse_capture: false,
        ..ViewState::default()
    };

    assert_eq!(
        idle_footer_text(&state),
        " selection mode | m restore mouse "
    );
}

#[test]
fn notice_message_appears_in_footer_and_can_be_cleared() {
    let file = indexed_lines(&["plain text"]);
    let mut state = ViewState::default();
    state.set_notice(
        "showing plain text; use --type".to_owned(),
        Instant::now(),
        NOTICE_DURATION,
    );

    assert!(file_footer_text(&file, &state).contains("showing plain text"));
    assert_eq!(file_footer_style(&state), error_style());
    assert_eq!(
        state.footer_message.as_ref().map(|message| message.kind),
        Some(FooterMessageKind::Error)
    );

    let action = handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 1, 10);

    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.footer_message, None);
}

#[test]
fn notice_message_expires_after_deadline() {
    let now = Instant::now();
    let mut state = ViewState::default();
    state.set_notice(
        "showing plain text; use --type".to_owned(),
        now,
        NOTICE_DURATION,
    );

    assert!(!state.expire_footer_message(now + NOTICE_DURATION - Duration::from_millis(1)));
    assert!(state.footer_message.is_some());
    assert!(state.expire_footer_message(now + NOTICE_DURATION));
    assert_eq!(state.footer_message, None);
}

#[test]
fn search_prompt_covers_notice_without_clearing_it() {
    let file = indexed_lines(&["plain text"]);
    let now = Instant::now();
    let mut state = ViewState::default();
    state.set_notice(
        "showing plain text; use --type".to_owned(),
        now,
        NOTICE_DURATION,
    );

    handle_key_event(KeyCode::Char('/'), KeyModifiers::NONE, &mut state, 1, 10);

    assert!(file_footer_text(&file, &state).contains("search:"));
    assert!(state.footer_message.is_some());

    handle_key_event(KeyCode::Esc, KeyModifiers::NONE, &mut state, 1, 10);

    assert!(file_footer_text(&file, &state).contains("showing plain text"));
    assert_eq!(file_footer_style(&state), error_style());
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
fn no_next_block_redraw_preserves_wrapped_tail_with_sticky_rows() {
    let mut temp = NamedTempFile::new().unwrap();
    let long = "tail wrap ".repeat(32);
    writeln!(temp, "{{").unwrap();
    writeln!(temp, r#"  "payload": {{"#).unwrap();
    writeln!(temp, r#"    "long": "{long}""#).unwrap();
    writeln!(temp, "  }}").unwrap();
    writeln!(temp, "}}").unwrap();
    temp.flush().unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();

    let mut state = ViewState::default();
    let layout = draw_layout(
        ratatui::layout::Size::new(48, 8),
        &file,
        &state,
        FormatKind::Json,
    );
    let sticky_visible_height = visible_height_for_sticky(layout.base_visible_height, 1);
    let base_tail = compute_tail_position(&file, layout.base_visible_height, layout.context)
        .expect("base tail");
    let sticky_tail =
        compute_tail_position(&file, sticky_visible_height, layout.context).expect("sticky tail");
    assert_ne!(
        base_tail, sticky_tail,
        "fixture must expose the base-height/sticky-height tail mismatch"
    );

    state.top = sticky_tail.top;
    state.top_row_offset = sticky_tail.row_offset;
    state.viewport_at_tail = true;
    state.preserve_tail_on_next_draw = true;
    let mut breadcrumb = JsonBreadcrumbCache::default();
    let mut tail_cache = TailPositionCache::default();

    let sticky = sync_sticky_layout(
        &file,
        FormatKind::Json,
        &mut state,
        &mut breadcrumb,
        &mut tail_cache,
        layout,
    )
    .unwrap();

    assert!(!sticky.lines.is_empty());
    assert_eq!(state.top, sticky_tail.top);
    assert_eq!(state.top_row_offset, sticky_tail.row_offset);
}

#[test]
fn manual_scroll_clears_pending_tail_preservation() {
    let mut state = ViewState {
        preserve_tail_on_next_draw: true,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 10, 5);

    assert!(action.dirty);
    assert!(!state.preserve_tail_on_next_draw);
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
    let action = handle_key_event_with_count(
        KeyCode::Enter,
        KeyModifiers::NONE,
        &mut state,
        1,
        false,
        false,
        10,
    );

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
        gutter: GutterLayout::new(4, false),
        x: 0,
        width: 80,
        wrap: false,
        mode: FormatKind::Json,
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
