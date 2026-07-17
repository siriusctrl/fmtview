use super::{
    cache::{RenderedLineCache, RenderedVisualRow},
    line::rendered_row_count,
    search::apply_search_highlight,
    types::{RenderContext, RenderRequest, RenderedViewport, ViewPosition, ViewportBottom},
};
use crate::{
    formats::json::chat::ChatRoleMark,
    transform::FormatKind,
    tui::{text::char_count, wrap::continuation_indent},
    viewer::file::input::SearchTarget,
};

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::viewer) struct ViewportRenderOptions<'a> {
    pub(in crate::viewer) line_modes: Option<&'a [FormatKind]>,
    pub(in crate::viewer) chat_role_marks: Option<&'a [ChatRoleMark]>,
    pub(in crate::viewer) search_query: Option<&'a str>,
    pub(in crate::viewer) active_search_match: Option<SearchTarget>,
}

pub(in crate::viewer) fn render_viewport(
    lines: &[String],
    first_line_number: usize,
    top_row_offset: usize,
    height: usize,
    request: RenderRequest,
    cache: &mut RenderedLineCache,
    options: ViewportRenderOptions<'_>,
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
            line_request(
                request,
                options.line_modes.and_then(|modes| modes.first().copied()),
            ),
        );
        if !top_rows.is_empty() {
            last_line_number = Some(first_line_number);
        }
        let chat_role = options
            .chat_role_marks
            .and_then(|marks| marks.first())
            .copied()
            .unwrap_or_default();
        if options.search_query.is_some() {
            for row in top_rows.into_iter().take(height) {
                bottom = Some(ViewportBottom {
                    line_index: first_line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                let active_range = active_search_range(
                    top_line,
                    first_line_number - 1,
                    &row,
                    request.context,
                    options,
                );
                rendered.push(apply_search_highlight(
                    apply_chat_role_gutter(row.line, row.row_index, chat_role, request.context),
                    options.search_query,
                    request.context,
                    active_range,
                ));
            }
        } else {
            for row in top_rows.into_iter().take(height) {
                bottom = Some(ViewportBottom {
                    line_index: first_line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(apply_chat_role_gutter(
                    row.line,
                    row.row_index,
                    chat_role,
                    request.context,
                ));
            }
        }
    }

    for (index, line) in remaining_lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        let line_number = first_line_number + index + 1;
        let offset = index + 1;
        let rows = cache.get_or_render_window(
            line,
            line_number,
            0,
            remaining,
            line_request(
                request,
                options
                    .line_modes
                    .and_then(|modes| modes.get(offset).copied()),
            ),
        );
        let chat_role = options
            .chat_role_marks
            .and_then(|marks| marks.get(offset))
            .copied()
            .unwrap_or_default();
        let taken = rows.len().min(remaining);
        if taken > 0 {
            last_line_number = Some(line_number);
        }
        if options.search_query.is_some() {
            for row in rows.into_iter().take(remaining) {
                bottom = Some(ViewportBottom {
                    line_index: line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                let active_range =
                    active_search_range(line, line_number - 1, &row, request.context, options);
                rendered.push(apply_search_highlight(
                    apply_chat_role_gutter(row.line, row.row_index, chat_role, request.context),
                    options.search_query,
                    request.context,
                    active_range,
                ));
            }
        } else {
            for row in rows.into_iter().take(remaining) {
                bottom = Some(ViewportBottom {
                    line_index: line_number - 1,
                    byte_end: row.end_byte,
                    line_end: row.line_end,
                });
                rendered.push(apply_chat_role_gutter(
                    row.line,
                    row.row_index,
                    chat_role,
                    request.context,
                ));
            }
        }
    }

    RenderedViewport {
        lines: rendered,
        last_line_number,
        bottom,
    }
}

fn active_search_range(
    line: &str,
    line_index: usize,
    row: &RenderedVisualRow,
    context: RenderContext,
    options: ViewportRenderOptions<'_>,
) -> Option<std::ops::Range<usize>> {
    let query = options.search_query?;
    let active = options.active_search_match?;
    if active.line != line_index
        || active.byte_index < row.start_byte
        || active.byte_index >= row.end_byte
    {
        return None;
    }
    let before = line.get(row.start_byte..active.byte_index)?;
    let row_prefix_chars = char_count(before);
    let indent = if context.wrap && row.row_index > 0 {
        continuation_indent(line, context.width)
    } else {
        0
    };
    let start = context
        .gutter
        .content_start()
        .saturating_add(indent)
        .saturating_add(row_prefix_chars);
    Some(start..start.saturating_add(char_count(query)))
}

fn apply_chat_role_gutter(
    mut line: ratatui::text::Line<'static>,
    row_index: usize,
    mark: ChatRoleMark,
    context: RenderContext,
) -> ratatui::text::Line<'static> {
    if !context.gutter.chat_role_enabled() {
        return line;
    }
    if line.spans.len() >= 3 {
        let [label, guide] =
            context
                .gutter
                .chat_role(mark.role, mark.label && row_index == 0, mark.guide);
        line.spans[1] = label;
        line.spans[2] = guide;
    }
    line
}

fn line_request(request: RenderRequest, mode: Option<FormatKind>) -> RenderRequest {
    let Some(mode) = mode else {
        return request;
    };
    RenderRequest {
        context: RenderContext {
            mode,
            ..request.context
        },
        ..request
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

pub(in crate::viewer) fn exact_top_line_scroll_limit(
    lines: &[String],
    context: RenderContext,
) -> usize {
    if !context.wrap {
        return 0;
    }

    lines
        .first()
        .map(|line| rendered_row_count(line, context).saturating_sub(1))
        .unwrap_or(0)
}

pub(in crate::viewer) fn effective_top_row_offset(
    line_number: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
    tail: Option<ViewPosition>,
) -> usize {
    let mut max_offset = top_line_scroll_limit(line_number, context, cache);
    if context.wrap
        && let Some(tail) = tail
        && tail.top + 1 == line_number
    {
        max_offset = max_offset.min(tail.row_offset);
    }
    max_offset
}

pub(in crate::viewer) fn top_line_scroll_limit(
    line_number: usize,
    context: RenderContext,
    cache: &RenderedLineCache,
) -> usize {
    if !context.wrap {
        return 0;
    }

    let status = cache.status(line_number);
    match status.total_rows {
        Some(rows) => rows.saturating_sub(1),
        None if status.known_rows > 0 => usize::MAX,
        None => 0,
    }
}
