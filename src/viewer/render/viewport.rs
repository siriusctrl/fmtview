use super::{
    cache::RenderedLineCache,
    line::rendered_row_count,
    search::apply_search_highlight,
    types::{RenderContext, RenderRequest, RenderedViewport, ViewPosition, ViewportBottom},
};

pub(in crate::viewer) fn render_viewport(
    lines: &[String],
    first_line_number: usize,
    top_row_offset: usize,
    height: usize,
    request: RenderRequest,
    cache: &mut RenderedLineCache,
    search_query: Option<&str>,
) -> RenderedViewport {
    let mut rendered = Vec::with_capacity(height);
    let mut last_line_number = None;

    let Some((top_line, remaining_lines)) = lines.split_first() else {
        return RenderedViewport {
            lines: rendered,
            last_line_number,
            bottom: None,
        };
    };

    let mut bottom = None;
    if height > 0 {
        let top_rows = cache.get_or_render_window(
            top_line,
            first_line_number,
            top_row_offset,
            height.saturating_add(1),
            request,
        );
        if !top_rows.is_empty() {
            last_line_number = Some(first_line_number);
        }
        if search_query.is_some() {
            for row in top_rows.into_iter().take(height) {
                bottom = Some(ViewportBottom {
                    line_index: first_line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(apply_search_highlight(
                    row.line,
                    search_query,
                    request.context.gutter_digits,
                ));
            }
        } else {
            for row in top_rows.into_iter().take(height) {
                bottom = Some(ViewportBottom {
                    line_index: first_line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(row.line);
            }
        }
    }

    for (index, line) in remaining_lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        let line_number = first_line_number + index + 1;
        let rows = cache.get_or_render_window(line, line_number, 0, remaining, request);
        let taken = rows.len().min(remaining);
        if taken > 0 {
            last_line_number = Some(line_number);
        }
        if search_query.is_some() {
            for row in rows.into_iter().take(remaining) {
                bottom = Some(ViewportBottom {
                    line_index: line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(apply_search_highlight(
                    row.line,
                    search_query,
                    request.context.gutter_digits,
                ));
            }
        } else {
            for row in rows.into_iter().take(remaining) {
                bottom = Some(ViewportBottom {
                    line_index: line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(row.line);
            }
        }
    }

    RenderedViewport {
        lines: rendered,
        last_line_number,
        bottom,
    }
}

#[cfg(test)]
pub(in crate::viewer) fn viewport_reaches_file_end(
    viewport: &RenderedViewport,
    line_count: usize,
) -> bool {
    viewport
        .bottom
        .is_some_and(|bottom| bottom.line_end && bottom.line_index + 1 >= line_count)
}

pub(in crate::viewer) fn exact_top_line_tail_offset(
    lines: &[String],
    visible_height: usize,
    context: RenderContext,
) -> usize {
    if visible_height == 0 || !context.wrap {
        return 0;
    }

    let Some(line) = lines.first() else {
        return 0;
    };

    rendered_row_count(line, context).saturating_sub(visible_height)
}

pub(in crate::viewer) fn effective_top_row_offset(
    line_number: usize,
    visible_height: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
    tail: Option<ViewPosition>,
) -> usize {
    let mut max_offset = top_line_tail_offset(line_number, visible_height, context, cache);
    if context.wrap
        && let Some(tail) = tail
        && tail.top + 1 == line_number
    {
        max_offset = max_offset.max(tail.row_offset);
    }
    max_offset
}

pub(in crate::viewer) fn top_line_tail_offset(
    line_number: usize,
    visible_height: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
) -> usize {
    if visible_height == 0 || !context.wrap {
        return 0;
    }

    let status = cache.status(line_number);
    match status.total_rows {
        Some(rows) => rows.saturating_sub(visible_height),
        None if status.known_rows > 0 => usize::MAX,
        None => 0,
    }
}
