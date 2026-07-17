use super::*;

#[test]
fn viewport_can_start_inside_wrapped_logical_line() {
    let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, false),
            x: 0,
            width: 4,
            wrap: true,
            mode: FormatKind::Json,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let first = render_viewport(
        &lines,
        1,
        0,
        2,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );
    assert_eq!(first.last_line_number, Some(1));
    assert_eq!(span_text(&first.lines[0].spans), "1 │ abcd");
    assert_eq!(span_text(&first.lines[1].spans), "  ┆ efgh");

    let second = render_viewport(
        &lines,
        1,
        1,
        2,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );
    assert_eq!(second.last_line_number, Some(1));
    assert_eq!(span_text(&second.lines[0].spans), "  ┆ efgh");
    assert_eq!(span_text(&second.lines[1].spans), "  ┆ ijkl");
}

#[test]
fn viewport_reports_actual_last_logical_line() {
    let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, false),
            x: 0,
            width: 4,
            wrap: true,
            mode: FormatKind::Json,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        2,
        3,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );

    assert_eq!(viewport.last_line_number, Some(2));
    assert_eq!(span_text(&viewport.lines[0].spans), "  ┆ ijkl");
    assert_eq!(span_text(&viewport.lines[1].spans), "2 │ next");
}

#[test]
fn markdown_viewport_reuses_inner_code_highlighter() {
    let lines = vec![
        "```json".to_owned(),
        r#"{"ok": true}"#.to_owned(),
        "```".to_owned(),
    ];
    let line_modes = vec![FormatKind::Markdown, FormatKind::Json, FormatKind::Markdown];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, false),
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Markdown,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        0,
        3,
        request,
        &mut cache,
        ViewportRenderOptions {
            line_modes: Some(&line_modes),
            chat_role_marks: None,
            search_query: None,
            active_search_match: None,
        },
    );

    assert_eq!(span_text(&viewport.lines[1].spans), r#"2 │ {"ok": true}"#);
    assert_eq!(
        styles_for_text(&viewport.lines[1].spans, r#""ok""#),
        vec![key_style()]
    );
    assert_eq!(
        styles_for_text(&viewport.lines[1].spans, "true"),
        vec![bool_style()]
    );
}

#[test]
fn json_viewport_shows_chat_role_gutter_on_message_start() {
    let lines = vec![
        "[".to_owned(),
        "  {".to_owned(),
        r#"    "role": "user","#.to_owned(),
        r#"    "content": "hello""#.to_owned(),
        "  }".to_owned(),
        "]".to_owned(),
    ];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, true),
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Json,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();
    let mut role_tracker = crate::formats::json::chat::ChatRoleTracker::default();
    let role_marks = role_tracker.mark_lines(&lines, 0);

    let viewport = render_viewport(
        &lines,
        1,
        0,
        6,
        request,
        &mut cache,
        ViewportRenderOptions {
            chat_role_marks: Some(&role_marks),
            ..ViewportRenderOptions::default()
        },
    );

    assert_eq!(span_text(&viewport.lines[0].spans), "1 │   │ [");
    assert_eq!(span_text(&viewport.lines[1].spans), "2 │ U │   {");
    assert_eq!(
        span_text(&viewport.lines[2].spans),
        r#"3 │   │     "role": "user","#
    );
    let user_style = crate::formats::json::chat::ChatRole::User.style();
    assert_eq!(viewport.lines[1].spans[1].style, user_style);
    assert_eq!(viewport.lines[1].spans[2].style, gutter_style());
    assert_eq!(viewport.lines[2].spans[2].style, user_style);
    assert_eq!(viewport.lines[3].spans[2].style, user_style);
    assert_eq!(viewport.lines[4].spans[2].style, gutter_style());
}

#[test]
fn json_chat_role_gutter_adapts_to_available_width() {
    let file = indexed_lines(&[r#"{"role":"assistant"}"#]);
    let state = ViewState::default();

    let wide = draw_layout(
        ratatui::layout::Size::new(80, 20),
        &file,
        &state,
        FormatKind::Json,
    );
    assert!(wide.context.gutter.chat_role_enabled());
    assert_eq!(wide.gutter_width, 8);

    let narrow = draw_layout(
        ratatui::layout::Size::new(61, 20),
        &file,
        &state,
        FormatKind::Json,
    );
    assert!(!narrow.context.gutter.chat_role_enabled());
    assert_eq!(narrow.gutter_width, 4);
}

#[test]
fn json_chat_role_guide_colors_wrapped_body_rows() {
    let lines = vec![
        "{".to_owned(),
        r#"  "role": "user","#.to_owned(),
        r#"  "content": "a long message body that wraps across several visual rows""#.to_owned(),
        "}".to_owned(),
    ];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, true),
            x: 0,
            width: 12,
            wrap: true,
            mode: FormatKind::Json,
        },
        row_limit: 32,
    };
    let mut role_tracker = crate::formats::json::chat::ChatRoleTracker::default();
    let role_marks = role_tracker.mark_lines(&lines, 0);
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        0,
        32,
        request,
        &mut cache,
        ViewportRenderOptions {
            chat_role_marks: Some(&role_marks),
            ..ViewportRenderOptions::default()
        },
    );

    assert!(viewport.lines.len() > lines.len());
    assert_eq!(span_text(&viewport.lines[0].spans), "1 │ U │ {");
    let user_style = crate::formats::json::chat::ChatRole::User.style();
    let last = viewport.lines.len() - 1;
    assert_eq!(viewport.lines[0].spans[1].style, user_style);
    assert_eq!(viewport.lines[0].spans[2].style, gutter_style());
    for line in &viewport.lines[1..last] {
        assert_eq!(line.spans[2].style, user_style);
    }
    assert_eq!(viewport.lines[last].spans[2].style, gutter_style());
    for line in viewport.lines.iter().skip(1) {
        assert!(!line.spans[1].content.contains('U'));
    }
}

#[test]
fn json_tool_role_uses_t_label_and_interior_guide() {
    let lines = vec![
        "{".to_owned(),
        r#"  "role": "tool","#.to_owned(),
        r#"  "content": {"ok": true}"#.to_owned(),
        "}".to_owned(),
    ];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, true),
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Json,
        },
        row_limit: 8,
    };
    let mut role_tracker = crate::formats::json::chat::ChatRoleTracker::default();
    let role_marks = role_tracker.mark_lines(&lines, 0);
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        0,
        lines.len(),
        request,
        &mut cache,
        ViewportRenderOptions {
            chat_role_marks: Some(&role_marks),
            ..ViewportRenderOptions::default()
        },
    );

    let tool_style = crate::formats::json::chat::ChatRole::Tool.style();
    assert_eq!(span_text(&viewport.lines[0].spans), "1 │ T │ {");
    assert_eq!(viewport.lines[0].spans[1].style, tool_style);
    assert_eq!(viewport.lines[0].spans[2].style, gutter_style());
    assert_eq!(viewport.lines[1].spans[2].style, tool_style);
    assert_eq!(viewport.lines[2].spans[2].style, tool_style);
    assert_eq!(viewport.lines[3].spans[2].style, gutter_style());
}

#[test]
fn markdown_json_code_does_not_enable_chat_role_gutter() {
    let lines = vec![
        "```json".to_owned(),
        r#"{"role":"assistant"}"#.to_owned(),
        "```".to_owned(),
    ];
    let line_modes = vec![FormatKind::Markdown, FormatKind::Json, FormatKind::Markdown];
    let request = RenderRequest {
        context: RenderContext {
            gutter: GutterLayout::new(1, false),
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Markdown,
        },
        row_limit: 8,
    };
    let mut cache = RenderedLineCache::default();

    let viewport = render_viewport(
        &lines,
        1,
        0,
        3,
        request,
        &mut cache,
        ViewportRenderOptions {
            line_modes: Some(&line_modes),
            chat_role_marks: None,
            search_query: None,
            active_search_match: None,
        },
    );

    assert_eq!(
        span_text(&viewport.lines[1].spans),
        r#"2 │ {"role":"assistant"}"#
    );
}

#[test]
fn markdown_mode_cache_resolves_fence_state_before_window() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "# notes").unwrap();
    writeln!(temp, "```toml").unwrap();
    writeln!(temp, "[viewer]").unwrap();
    writeln!(temp, "mode = \"markdown\"").unwrap();
    writeln!(temp, "```").unwrap();
    let file = IndexedTempFile::new("notes".to_owned(), temp).unwrap();
    let lines = file.read_window(2, 1).unwrap();
    let mut cache = MarkdownModeCache::default();

    let modes = cache
        .line_modes(&file, 2, &lines, FormatKind::Markdown)
        .unwrap()
        .unwrap();

    assert_eq!(modes, vec![FormatKind::Toml]);
}

#[test]
fn markdown_mode_cache_keeps_interval_checkpoints_only() {
    let mut temp = NamedTempFile::new().unwrap();
    for index in 0..1_600 {
        if index == 10 {
            writeln!(temp, "```toml").unwrap();
        } else if index == 1_200 {
            writeln!(temp, "```").unwrap();
        } else {
            writeln!(temp, "line {index}").unwrap();
        }
    }
    let file = IndexedTempFile::new("notes".to_owned(), temp).unwrap();
    let mut cache = MarkdownModeCache::default();

    for start in 0..1_500 {
        let lines = file.read_window(start, 1).unwrap();
        cache
            .line_modes(&file, start, &lines, FormatKind::Markdown)
            .unwrap();
    }

    assert_eq!(cache.checkpoint_count(), 3);
}

#[test]
fn wrapped_progress_advances_by_visible_bytes() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "abcdefghijkl").unwrap();
    writeln!(temp, "next").unwrap();
    let file = IndexedTempFile::new("test".to_owned(), temp).unwrap();
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
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
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 4,
        wrap: true,
        mode: FormatKind::Json,
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
        ViewportRenderOptions::default(),
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
    assert!(tail.row_offset < top_line_scroll_limit(tail.top + 1, context, &cache));
    assert_eq!(
        effective_top_row_offset(tail.top + 1, context, &cache, Some(tail)),
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
    let tail = compute_tail_position(&file, 2, context).unwrap();
    let lines = file.read_window(0, 2).unwrap();
    let mut cache = RenderedLineCache::default();

    let partial = render_viewport(
        &lines,
        1,
        2,
        2,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );
    let max_offset = effective_top_row_offset(1, context, &cache, Some(tail));
    let clamped = render_viewport(
        &lines,
        1,
        max_offset,
        2,
        request,
        &mut cache,
        ViewportRenderOptions::default(),
    );
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
