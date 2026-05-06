use super::*;

#[test]
fn viewport_can_start_inside_wrapped_logical_line() {
    let lines = vec!["abcdefghijkl".to_owned(), "next".to_owned()];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: SyntaxKind::Structured,
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
            gutter_digits: 1,
            x: 0,
            width: 4,
            wrap: true,
            mode: SyntaxKind::Structured,
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
    let line_modes = vec![
        SyntaxKind::Markdown,
        SyntaxKind::Structured,
        SyntaxKind::Markdown,
    ];
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: false,
            mode: SyntaxKind::Markdown,
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
            search_query: None,
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
fn markdown_syntax_cache_resolves_fence_state_before_window() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "# notes").unwrap();
    writeln!(temp, "```toml").unwrap();
    writeln!(temp, "[viewer]").unwrap();
    writeln!(temp, "mode = \"markdown\"").unwrap();
    writeln!(temp, "```").unwrap();
    let file = IndexedTempFile::new("notes".to_owned(), temp).unwrap();
    let lines = file.read_window(2, 1).unwrap();
    let mut cache = MarkdownSyntaxCache::default();

    let modes = cache
        .line_modes(&file, 2, &lines, SyntaxKind::Markdown)
        .unwrap()
        .unwrap();

    assert_eq!(modes, vec![SyntaxKind::Toml]);
}

#[test]
fn markdown_syntax_cache_keeps_interval_checkpoints_only() {
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
    let mut cache = MarkdownSyntaxCache::default();

    for start in 0..1_500 {
        let lines = file.read_window(start, 1).unwrap();
        cache
            .line_modes(&file, start, &lines, SyntaxKind::Markdown)
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
        gutter_digits: 1,
        x: 0,
        width: 4,
        wrap: true,
        mode: SyntaxKind::Structured,
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
        mode: SyntaxKind::Structured,
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
        mode: SyntaxKind::Structured,
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
        mode: SyntaxKind::Structured,
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
    let max_offset = effective_top_row_offset(1, 2, context, &cache, Some(tail));
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
