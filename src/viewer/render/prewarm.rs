use std::time::Instant;

use crate::line_index::ViewFile;

use super::super::{
    PREWARM_BUDGET, PREWARM_MAX_LINE_BYTES, PREWARM_MAX_LINES, PREWARM_PAGES,
    RENDER_CACHE_MAX_ROWS_PER_LINE, WRAP_PREWARM_LOGICAL_LINES, WRAP_RENDER_CHUNK_ROWS,
};
use super::{
    cache::{LineWindowCache, RenderedLineCache},
    types::RenderRequest,
};

pub(in crate::viewer) fn prewarm_render_cache(
    file: &dyn ViewFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    top: usize,
    top_row_offset: usize,
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
            top,
            top_row_offset,
            visible_height,
            request,
        );
        return;
    }

    let side = visible_height.saturating_mul(PREWARM_PAGES);
    let start = top.saturating_sub(side);
    let count = visible_height
        .saturating_add(side.saturating_mul(2))
        .min(PREWARM_MAX_LINES)
        .min(file.line_count().saturating_sub(start));
    let margin = visible_height.saturating_mul(2).max(32);
    let Ok(lines) = line_cache.read(file, start, count, margin) else {
        return;
    };

    let started = Instant::now();
    for (index, line) in lines.lines.iter().enumerate() {
        if line.len() > PREWARM_MAX_LINE_BYTES {
            continue;
        }
        render_cache.get_or_render(line, start + index + 1, request);
        if started.elapsed() >= PREWARM_BUDGET {
            break;
        }
    }
}

pub(in crate::viewer) fn prewarm_wrapped_render_cache(
    file: &dyn ViewFile,
    line_cache: &mut LineWindowCache,
    render_cache: &mut RenderedLineCache,
    top: usize,
    top_row_offset: usize,
    visible_height: usize,
    request: RenderRequest,
) {
    let count = visible_height
        .saturating_add(WRAP_PREWARM_LOGICAL_LINES)
        .min(file.line_count().saturating_sub(top));
    let Ok(lines) = line_cache.read(file, top, count, WRAP_PREWARM_LOGICAL_LINES) else {
        return;
    };

    let started = Instant::now();
    if let Some(line) = lines.lines.first() {
        prewarm_wrapped_line_chunks(
            render_cache,
            line,
            top + 1,
            top_row_offset,
            visible_height,
            request,
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
        render_cache.get_or_render_window(line, top + index + 1, 0, visible_height, request);
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
