use super::*;

#[test]
#[ignore = "performance smoke; run with cargo test --release perf_huge_wrapped_line_paths -- --ignored --nocapture --test-threads=1"]
fn perf_huge_wrapped_line_paths() {
    let message = format!(
        r#"  "message": "<root>{}</root>""#,
        r#"<item id=\"1\"><name>visible</name></item>"#.repeat(600_000)
    );
    let context = RenderContext {
        gutter: GutterLayout::new(1, false),
        x: 0,
        width: 94,
        wrap: true,
        mode: FormatKind::Json,
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
        ViewportRenderOptions::default(),
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
            gutter: GutterLayout::new(4, false),
            x: 0,
            width: 96,
            wrap: true,
            mode: FormatKind::Json,
        },
        row_limit: render_row_limit(27),
    };
    let mut cache = RenderedLineCache::default();
    let started = Instant::now();
    let mut rendered_rows = 0_usize;
    let mut background_cells = 0_usize;

    for top in 0..400 {
        let viewport = render_viewport(
            &lines[top..],
            top + 1,
            0,
            27,
            request,
            &mut cache,
            ViewportRenderOptions::default(),
        );
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
