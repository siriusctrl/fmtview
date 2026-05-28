use std::time::Instant;

use crate::load::ViewFile;

use crate::viewer::file::{
    PREWARM_BUDGET, PREWARM_MAX_LINE_BYTES, PREWARM_MAX_LINES, PREWARM_PAGES,
    RENDER_CACHE_MAX_ROWS_PER_LINE, WRAP_PREWARM_LOGICAL_LINES, WRAP_RENDER_CHUNK_ROWS,
};

use super::super::markdown_modes::MarkdownModeCache;
use super::{
    cache::{LineWindowCache, RenderedLineCache},
    types::{RenderContext, RenderRequest, ViewPosition},
};

pub(in crate::viewer) fn prewarm_render_cache(
    file: &dyn ViewFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    markdown_cache: &mut MarkdownModeCache,
    position: ViewPosition,
    visible_height: usize,
    request: RenderRequest,
) {
    if visible_height == 0 || file.line_count() == 0 {
        return;
    }
    if request.context.wrap {
        prewarm_wrapped_render_cache(
            file,
            line_cache,
            render_cache,
            markdown_cache,
            position,
            visible_height,
            request,
        );
        return;
    }

    let side = visible_height.saturating_mul(PREWARM_PAGES);
    let start = position.top.saturating_sub(side);
    let count = visible_height
        .saturating_add(side.saturating_mul(2))
        .min(PREWARM_MAX_LINES)
        .min(file.line_count().saturating_sub(start));
    let margin = visible_height.saturating_mul(2).max(32);
    let Ok(lines) = line_cache.read(file, start, count, margin) else {
        return;
    };

    let started = Instant::now();
    let line_modes = markdown_cache
        .line_modes(file, start, lines.lines, request.context.mode)
        .ok()
        .flatten();
    for (index, line) in lines.lines.iter().enumerate() {
        if line.len() > PREWARM_MAX_LINE_BYTES {
            continue;
        }
        render_cache.get_or_render(
            line,
            start + index + 1,
            line_request(
                request,
                line_modes
                    .as_deref()
                    .and_then(|modes| modes.get(index).copied()),
            ),
        );
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

pub(in crate::viewer) fn prewarm_wrapped_render_cache(
    file: &dyn ViewFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    markdown_cache: &mut MarkdownModeCache,
    position: ViewPosition,
    visible_height: usize,
    request: RenderRequest,
) {
    let count = visible_height
        .saturating_add(WRAP_PREWARM_LOGICAL_LINES)
        .min(file.line_count().saturating_sub(position.top));
    let Ok(lines) = line_cache.read(file, position.top, count, WRAP_PREWARM_LOGICAL_LINES) else {
        return;
    };

    let started = Instant::now();
    let line_modes = markdown_cache
        .line_modes(file, position.top, lines.lines, request.context.mode)
        .ok()
        .flatten();
    if let Some(line) = lines.lines.first() {
        prewarm_wrapped_line_chunks(
            render_cache,
            line,
            position.top + 1,
            position.row_offset,
            visible_height,
            line_request(
                request,
                line_modes
                    .as_deref()
                    .and_then(|modes| modes.first().copied()),
            ),
        );
        if started.elapsed() >= PREWARM_BUDGET {
            return;
        }
    }

    for (index, line) in lines
        .lines
        .iter()
        .enumerate()
        .skip(1)
        .take(WRAP_PREWARM_LOGICAL_LINES)
    {
        render_cache.get_or_render_window(
            line,
            position.top + index + 1,
            0,
            visible_height,
            line_request(
                request,
                line_modes
                    .as_deref()
                    .and_then(|modes| modes.get(index).copied()),
            ),
        );
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

pub(in crate::viewer) fn prewarm_wrapped_line_chunks(
    render_cache: &mut RenderedLineCache,
    line: &str,
    line_number: usize,
    top_row_offset: usize,
    visible_height: usize,
    request: RenderRequest,
) {
    let status = render_cache.status(line_number);
    if status.total_rows.is_none() && status.known_rows > 0 {
        render_cache.get_or_render_window(
            line,
            line_number,
            status.known_rows,
            visible_height,
            request,
        );
    }

    if top_row_offset > 0 {
        let previous = top_row_offset.saturating_sub(WRAP_RENDER_CHUNK_ROWS);
        render_cache.get_or_render_window(line, line_number, previous, visible_height, request);
    }
}

pub(in crate::viewer) fn render_row_limit(visible_height: usize) -> usize {
    visible_height
        .saturating_mul(2)
        .clamp(32, RENDER_CACHE_MAX_ROWS_PER_LINE)
}

fn line_request(
    request: RenderRequest,
    mode: Option<crate::transform::FormatKind>,
) -> RenderRequest {
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
