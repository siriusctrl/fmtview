use super::*;

#[test]
fn rendered_line_cache_reuses_until_context_changes() {
    let mut cache = RenderedLineCache::default();
    let request = RenderRequest {
        context: RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 3,
            wrap: false,
            mode: SyntaxKind::Structured,
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
            mode: SyntaxKind::Structured,
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
            mode: SyntaxKind::Structured,
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
            mode: SyntaxKind::Structured,
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
            mode: SyntaxKind::Structured,
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
