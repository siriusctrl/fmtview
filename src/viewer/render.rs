use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    ops::Range,
    time::Instant,
};

use anyhow::Result;
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::line_index::ViewFile;

use super::highlight::{
    HighlightCheckpointIndex, highlight_content, highlight_content_window_indexed,
};
use super::palette::{gutter_style, plain_style, search_match_bg};
use super::{
    PREWARM_BUDGET, PREWARM_MAX_LINE_BYTES, PREWARM_MAX_LINES, PREWARM_PAGES,
    RENDER_CACHE_MAX_LINES, RENDER_CACHE_MAX_ROWS_PER_LINE, ViewMode,
    WRAP_CHECKPOINT_INTERVAL_ROWS, WRAP_GUTTER_MAJOR_TICK_ROWS, WRAP_GUTTER_MINOR_TICK_ROWS,
    WRAP_PREWARM_LOGICAL_LINES, WRAP_RENDER_CHUNK_ROWS, WRAP_RENDER_CHUNKS_PER_LINE,
    input::ViewState,
};

#[derive(Debug, Default)]
pub(super) struct LineWindowCache {
    pub(super) start: usize,
    pub(super) lines: Vec<String>,
}

pub(super) struct LineWindow<'a> {
    pub(super) lines: &'a [String],
}

impl LineWindowCache {
    pub(super) fn read(
        &mut self,
        file: &dyn ViewFile,
        top: usize,
        height: usize,
        margin: usize,
    ) -> Result<LineWindow<'_>> {
        if height == 0 || (file.line_count_exact() && top >= file.line_count()) {
            return Ok(LineWindow { lines: &[] });
        }

        let cached_end = self.start.saturating_add(self.lines.len());
        let requested_end = if file.line_count_exact() {
            top.saturating_add(height).min(file.line_count())
        } else {
            top.saturating_add(height)
        };
        if top >= self.start && requested_end <= cached_end {
            let start = top - self.start;
            let end = requested_end - self.start;
            return Ok(LineWindow {
                lines: &self.lines[start..end],
            });
        }

        let fetch_start = top.saturating_sub(margin);
        let fetch_count = if file.line_count_exact() {
            height
                .saturating_add(margin.saturating_mul(2))
                .min(file.line_count().saturating_sub(fetch_start))
        } else {
            height.saturating_add(margin.saturating_mul(2))
        };
        self.lines = file.read_window(fetch_start, fetch_count)?;
        self.start = fetch_start;

        let start = top - self.start;
        let end = requested_end
            .saturating_sub(self.start)
            .min(self.lines.len());
        Ok(LineWindow {
            lines: &self.lines[start..end],
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct RenderedLineCache {
    pub(super) request: Option<RenderRequest>,
    pub(super) lines: HashMap<usize, CachedRenderedLine>,
    pub(super) order: VecDeque<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct RenderedVisualRow {
    pub(super) line: Line<'static>,
    pub(super) end_byte: usize,
    pub(super) line_end: bool,
}

#[derive(Debug, Default)]
pub(super) struct CachedRenderedLine {
    pub(super) chunks: VecDeque<RenderedLineChunk>,
    pub(super) total_rows: Option<usize>,
    pub(super) index: LineRenderIndex,
}

#[derive(Debug)]
pub(super) struct RenderedLineChunk {
    pub(super) start_row: usize,
    pub(super) rows: Vec<RenderedVisualRow>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RenderedLineStatus {
    pub(super) known_rows: usize,
    pub(super) total_rows: Option<usize>,
}

#[derive(Debug, Default)]
pub(super) struct LineRenderIndex {
    pub(super) wrap: WrapCheckpointIndex,
    pub(super) highlight: HighlightCheckpointIndex,
}

#[derive(Debug, Default)]
pub(super) struct WrapCheckpointIndex {
    pub(super) checkpoints: Vec<WrapCheckpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WrapCheckpoint {
    pub(super) row: usize,
    pub(super) start_byte: usize,
    pub(super) start_char: usize,
}

impl RenderedLineCache {
    pub(super) fn get_or_render(
        &mut self,
        line: &str,
        line_number: usize,
        request: RenderRequest,
    ) -> Vec<Line<'static>> {
        self.get_or_render_window(line, line_number, 0, request.row_limit, request)
            .into_iter()
            .map(|row| row.line)
            .collect()
    }

    pub(super) fn get_or_render_window(
        &mut self,
        line: &str,
        line_number: usize,
        row_start: usize,
        max_rows: usize,
        request: RenderRequest,
    ) -> Vec<RenderedVisualRow> {
        if self.request != Some(request) {
            self.request = Some(request);
            self.lines.clear();
            self.order.clear();
        }

        if max_rows == 0 {
            return Vec::new();
        }

        if !self.lines.contains_key(&line_number) {
            self.evict_until_room();
            self.order.push_back(line_number);
        }

        match self.lines.entry(line_number) {
            Entry::Occupied(mut entry) => {
                entry
                    .get_mut()
                    .render_window(line, line_number, row_start, max_rows, request)
            }
            Entry::Vacant(entry) => {
                let mut cached = CachedRenderedLine::default();
                let rows = cached.render_window(line, line_number, row_start, max_rows, request);
                entry.insert(cached);
                rows
            }
        }
    }

    pub(super) fn status(&self, line_number: usize) -> RenderedLineStatus {
        self.lines
            .get(&line_number)
            .map(CachedRenderedLine::status)
            .unwrap_or(RenderedLineStatus {
                known_rows: 0,
                total_rows: None,
            })
    }

    pub(super) fn evict_until_room(&mut self) {
        while self.lines.len() >= RENDER_CACHE_MAX_LINES {
            if let Some(line_number) = self.order.pop_front() {
                self.lines.remove(&line_number);
            } else {
                break;
            }
        }
    }
}

impl CachedRenderedLine {
    pub(super) fn render_window(
        &mut self,
        line: &str,
        line_number: usize,
        row_start: usize,
        max_rows: usize,
        request: RenderRequest,
    ) -> Vec<RenderedVisualRow> {
        if let Some(rows) = self.cached_window(row_start, max_rows) {
            return rows;
        }

        if self
            .total_rows
            .is_some_and(|total_rows| row_start >= total_rows)
        {
            return Vec::new();
        }

        let chunk_rows = if request.context.wrap {
            max_rows.max(WRAP_RENDER_CHUNK_ROWS)
        } else {
            max_rows
        };
        let rendered = render_logical_line_window_with_status_indexed(
            line,
            line_number,
            row_start,
            chunk_rows,
            request.context,
            &mut self.index,
        );
        if let Some(total_rows) = rendered.total_rows {
            self.total_rows = Some(total_rows);
        }
        if !rendered.rows.is_empty() {
            self.chunks.push_back(RenderedLineChunk {
                start_row: row_start,
                rows: rendered.rows,
            });
            while self.chunks.len() > WRAP_RENDER_CHUNKS_PER_LINE {
                self.chunks.pop_front();
            }
        }

        self.cached_window(row_start, max_rows).unwrap_or_default()
    }

    pub(super) fn cached_window(
        &self,
        row_start: usize,
        max_rows: usize,
    ) -> Option<Vec<RenderedVisualRow>> {
        let desired_end = row_start.saturating_add(max_rows);
        self.chunks.iter().find_map(|chunk| {
            let chunk_end = chunk.start_row.saturating_add(chunk.rows.len());
            if row_start < chunk.start_row || row_start >= chunk_end {
                return None;
            }
            if chunk_end < desired_end
                && self
                    .total_rows
                    .is_none_or(|total_rows| total_rows > chunk_end)
            {
                return None;
            }
            let start = row_start - chunk.start_row;
            let end = start.saturating_add(max_rows).min(chunk.rows.len());
            Some(chunk.rows[start..end].to_vec())
        })
    }

    pub(super) fn status(&self) -> RenderedLineStatus {
        let known_rows = self
            .chunks
            .iter()
            .map(|chunk| chunk.start_row.saturating_add(chunk.rows.len()))
            .max()
            .unwrap_or(0);
        RenderedLineStatus {
            known_rows,
            total_rows: self.total_rows,
        }
    }
}

impl WrapCheckpointIndex {
    pub(super) fn start_for(&self, row_start: usize) -> WrapCheckpoint {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.row <= row_start)
            .copied()
            .unwrap_or(WrapCheckpoint {
                row: 0,
                start_byte: 0,
                start_char: 0,
            })
    }

    pub(super) fn remember(&mut self, checkpoint: WrapCheckpoint) {
        if checkpoint.row == 0 || checkpoint.row % WRAP_CHECKPOINT_INTERVAL_ROWS != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&checkpoint.row, |existing| existing.row)
        {
            Ok(_) => {}
            Err(position) => self.checkpoints.insert(position, checkpoint),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewPosition {
    pub(super) top: usize,
    pub(super) row_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TailPositionKey {
    pub(super) line_count: usize,
    pub(super) visible_height: usize,
    pub(super) width: usize,
}

#[derive(Debug, Default)]
pub(super) struct TailPositionCache {
    pub(super) key: Option<TailPositionKey>,
    pub(super) position: Option<ViewPosition>,
}

impl TailPositionCache {
    pub(super) fn position(
        &mut self,
        file: &dyn ViewFile,
        visible_height: usize,
        context: RenderContext,
    ) -> Result<ViewPosition> {
        if !context.wrap {
            return Ok(ViewPosition {
                top: last_full_logical_page_top(file.line_count(), visible_height),
                row_offset: 0,
            });
        }

        let key = TailPositionKey {
            line_count: file.line_count(),
            visible_height,
            width: context.width,
        };
        if self.key == Some(key) {
            if let Some(position) = self.position {
                return Ok(position);
            }
        }

        let position = compute_tail_position(file, visible_height, context)?;
        self.key = Some(key);
        self.position = Some(position);
        Ok(position)
    }
}

pub(super) fn compute_tail_position(
    file: &dyn ViewFile,
    visible_height: usize,
    context: RenderContext,
) -> Result<ViewPosition> {
    let line_count = file.line_count();
    if line_count == 0 || visible_height == 0 {
        return Ok(ViewPosition {
            top: 0,
            row_offset: 0,
        });
    }

    if !context.wrap {
        return Ok(ViewPosition {
            top: last_full_logical_page_top(line_count, visible_height),
            row_offset: 0,
        });
    }

    let mut needed_rows = visible_height;
    let mut end = line_count;
    while end > 0 {
        let start = end.saturating_sub(visible_height.max(32));
        let lines = file.read_window(start, end - start)?;
        for (index, line) in lines.iter().enumerate().rev() {
            let line_index = start + index;
            let rows = rendered_row_count(line, context);
            if rows >= needed_rows {
                return Ok(ViewPosition {
                    top: line_index,
                    row_offset: rows - needed_rows,
                });
            }
            needed_rows -= rows;
        }
        end = start;
    }

    Ok(ViewPosition {
        top: 0,
        row_offset: 0,
    })
}

pub(super) fn last_full_logical_page_top(line_count: usize, visible_height: usize) -> usize {
    line_count.saturating_sub(visible_height.max(1))
}

pub(super) fn is_after_tail(state: &ViewState, tail: ViewPosition) -> bool {
    state.top > tail.top || (state.top == tail.top && state.top_row_offset > tail.row_offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderRequest {
    pub(super) context: RenderContext,
    pub(super) row_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderContext {
    pub(super) gutter_digits: usize,
    pub(super) x: usize,
    pub(super) width: usize,
    pub(super) wrap: bool,
    pub(super) mode: ViewMode,
}

#[derive(Debug)]
pub(super) struct RenderedViewport {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) last_line_number: Option<usize>,
    pub(super) bottom: Option<ViewportBottom>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ViewportBottom {
    pub(super) line_index: usize,
    pub(super) byte_end: usize,
    pub(super) line_end: bool,
}

pub(super) fn render_viewport(
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
    }

    RenderedViewport {
        lines: rendered,
        last_line_number,
        bottom,
    }
}

#[cfg(test)]
pub(super) fn viewport_reaches_file_end(viewport: &RenderedViewport, line_count: usize) -> bool {
    viewport
        .bottom
        .is_some_and(|bottom| bottom.line_end && bottom.line_index + 1 >= line_count)
}

pub(super) fn exact_top_line_tail_offset(
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

pub(super) fn effective_top_row_offset(
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

pub(super) fn top_line_tail_offset(
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

pub(super) fn apply_search_highlight(
    line: Line<'static>,
    query: Option<&str>,
    gutter_digits: usize,
) -> Line<'static> {
    let Some(query) = query else {
        return line;
    };
    if query.is_empty() {
        return line;
    }

    let visual_text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let ranges = search_match_ranges(&visual_text, query, gutter_digits + 3);
    if ranges.is_empty() {
        return line;
    }

    Line {
        style: line.style,
        alignment: line.alignment,
        spans: apply_search_ranges_to_spans(&line.spans, &ranges),
    }
}

pub(super) fn search_match_ranges(text: &str, query: &str, start_char: usize) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let total_chars = char_count(text);
    if start_char >= total_chars {
        return Vec::new();
    }

    let search_text = slice_chars(text, start_char, total_chars);
    let query_len = char_count(query);
    search_text
        .match_indices(query)
        .map(|(byte_index, _)| {
            let start = start_char + char_count(&search_text[..byte_index]);
            start..start + query_len
        })
        .collect()
}

pub(super) fn apply_search_ranges_to_spans(
    spans: &[Span<'static>],
    ranges: &[Range<usize>],
) -> Vec<Span<'static>> {
    let mut highlighted = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = char_count(text);
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let split_points = search_split_points(span_start, span_end, ranges);
        for window in split_points.windows(2) {
            let start = window[0];
            let end = window[1];
            if start == end {
                continue;
            }

            let mut style = span.style;
            if range_is_highlighted(start, end, ranges) {
                style = style.bg(search_match_bg());
            }
            push_styled_span(
                &mut highlighted,
                slice_chars(text, start - span_start, end - span_start),
                style,
            );
        }
    }

    highlighted
}

pub(super) fn search_split_points(
    span_start: usize,
    span_end: usize,
    ranges: &[Range<usize>],
) -> Vec<usize> {
    let mut points = vec![span_start, span_end];
    for range in ranges {
        let start = range.start.max(span_start).min(span_end);
        let end = range.end.max(span_start).min(span_end);
        if start < end {
            points.push(start);
            points.push(end);
        }
    }
    points.sort_unstable();
    points.dedup();
    points
}

pub(super) fn range_is_highlighted(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| start >= range.start && end <= range.end)
}

pub(super) fn prewarm_render_cache(
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

pub(super) fn prewarm_wrapped_render_cache(
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

pub(super) fn prewarm_wrapped_line_chunks(
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

pub(super) fn render_row_limit(visible_height: usize) -> usize {
    visible_height
        .saturating_mul(2)
        .clamp(32, RENDER_CACHE_MAX_ROWS_PER_LINE)
}

#[cfg(test)]
pub(super) fn render_logical_line(
    line: &str,
    line_number: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window(line, line_number, 0, max_rows, context)
}

#[derive(Debug)]
pub(super) struct RenderedLineWindow {
    pub(super) rows: Vec<RenderedVisualRow>,
    pub(super) total_rows: Option<usize>,
}

#[cfg(test)]
pub(super) fn render_logical_line_window(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    render_logical_line_window_with_status(line, line_number, row_start, max_rows, context)
        .rows
        .into_iter()
        .map(|row| row.line)
        .collect()
}

#[cfg(test)]
pub(super) fn render_logical_line_window_with_status(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
) -> RenderedLineWindow {
    let mut index = LineRenderIndex::default();
    render_logical_line_window_with_status_indexed(
        line,
        line_number,
        row_start,
        max_rows,
        context,
        &mut index,
    )
}

pub(super) fn render_logical_line_window_with_status_indexed(
    line: &str,
    line_number: usize,
    row_start: usize,
    max_rows: usize,
    context: RenderContext,
    index: &mut LineRenderIndex,
) -> RenderedLineWindow {
    if max_rows == 0 {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows: None,
        };
    }

    if !context.wrap {
        if row_start > 0 {
            return RenderedLineWindow {
                rows: Vec::new(),
                total_rows: Some(1),
            };
        }
        let line_chars = char_count(line);
        return RenderedLineWindow {
            rows: vec![RenderedVisualRow {
                line: styled_segment(
                    line_number_gutter(line_number, context.gutter_digits),
                    line,
                    context.x,
                    context.x.saturating_add(context.width),
                    context.mode,
                ),
                end_byte: byte_index_for_char(
                    line,
                    context.x.saturating_add(context.width).min(line_chars),
                ),
                line_end: context.x.saturating_add(context.width) >= line_chars,
            }],
            total_rows: Some(1),
        };
    }

    let wrap_window = wrap_ranges_window_indexed(
        line,
        context.width,
        continuation_indent(line, context.width),
        row_start,
        max_rows,
        Some(&mut index.wrap),
    );
    let visible_ranges = wrap_window.ranges;
    let total_rows = wrap_window.total_rows;
    let highlight_end_byte = visible_ranges
        .iter()
        .map(|range| range.end_byte)
        .max()
        .unwrap_or(0);
    let Some(first_range) = visible_ranges.first() else {
        return RenderedLineWindow {
            rows: Vec::new(),
            total_rows,
        };
    };
    let highlight_start_byte = first_range.start_byte;
    let highlight_start_char = first_range.start_char;
    let spans = highlight_content_window_indexed(
        line,
        context.mode,
        highlight_start_byte,
        highlight_end_byte,
        Some(&mut index.highlight),
    );
    let rows = visible_ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let row_index = row_start + index;
            let gutter = if row_index == 0 {
                line_number_gutter(line_number, context.gutter_digits)
            } else {
                continuation_gutter(row_index, context.gutter_digits)
            };
            let mut line_spans = vec![gutter];
            if range.continuation_indent > 0 {
                push_styled_span(
                    &mut line_spans,
                    " ".repeat(range.continuation_indent),
                    plain_style(),
                );
            }
            line_spans.extend(slice_spans(
                &spans,
                range.start_char - highlight_start_char,
                range.end_char - highlight_start_char,
            ));
            RenderedVisualRow {
                line: Line::from(line_spans),
                end_byte: range.end_byte,
                line_end: range.end_byte >= line.len(),
            }
        })
        .collect();
    RenderedLineWindow { rows, total_rows }
}

pub(super) fn rendered_row_count(line: &str, context: RenderContext) -> usize {
    if !context.wrap {
        return 1;
    }

    wrapped_row_count(
        line,
        context.width,
        continuation_indent(line, context.width),
    )
}

pub(super) fn wrapped_row_count(line: &str, width: usize, continuation_indent: usize) -> usize {
    if line.is_empty() || width == 0 {
        return 1;
    }

    let mut rows = 0_usize;
    let mut start_byte = 0_usize;
    let mut start_char = 0_usize;
    while start_byte < line.len() {
        let continuation = rows > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        rows = rows.saturating_add(1);
    }

    rows
}

pub(super) fn styled_segment(
    gutter: Span<'static>,
    line: &str,
    start: usize,
    end: usize,
    mode: ViewMode,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(gutter);
    let highlight_prefix = slice_chars(line, 0, end);
    spans.extend(slice_spans(
        &highlight_content(&highlight_prefix, mode),
        start,
        end,
    ));
    Line::from(spans)
}

pub(super) fn line_number_gutter(line_number: usize, gutter_digits: usize) -> Span<'static> {
    Span::styled(format!("{line_number:>gutter_digits$} │ "), gutter_style())
}

pub(super) fn continuation_gutter(row_index: usize, gutter_digits: usize) -> Span<'static> {
    let marker = continuation_gutter_marker(row_index);
    Span::styled(format!("{:>gutter_digits$} {marker} ", ""), gutter_style())
}

pub(super) fn continuation_gutter_marker(row_index: usize) -> char {
    if row_index > 0 && row_index % WRAP_GUTTER_MAJOR_TICK_ROWS == 0 {
        '┠'
    } else if row_index > 0 && row_index % WRAP_GUTTER_MINOR_TICK_ROWS == 0 {
        '┊'
    } else {
        '┆'
    }
}

pub(super) fn format_count(value: usize) -> String {
    let raw = value.to_string();
    let mut formatted = String::with_capacity(raw.len() + raw.len() / 3);
    let first_group = raw.len() % 3;
    for (index, ch) in raw.chars().enumerate() {
        if index > 0
            && (index == first_group || (index > first_group && (index - first_group) % 3 == 0))
        {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WrapRange {
    pub(super) start_char: usize,
    pub(super) end_char: usize,
    pub(super) start_byte: usize,
    pub(super) end_byte: usize,
    pub(super) continuation_indent: usize,
}

#[derive(Debug)]
pub(super) struct WrapWindow {
    pub(super) ranges: Vec<WrapRange>,
    pub(super) total_rows: Option<usize>,
}

#[cfg(test)]
pub(super) fn wrap_ranges(
    line: &str,
    width: usize,
    continuation_indent: usize,
    max_rows: usize,
) -> Vec<WrapRange> {
    wrap_ranges_window(line, width, continuation_indent, 0, max_rows).ranges
}

#[cfg(test)]
pub(super) fn wrap_ranges_window(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
) -> WrapWindow {
    wrap_ranges_window_indexed(line, width, continuation_indent, row_start, max_rows, None)
}

pub(super) fn wrap_ranges_window_indexed(
    line: &str,
    width: usize,
    continuation_indent: usize,
    row_start: usize,
    max_rows: usize,
    mut checkpoints: Option<&mut WrapCheckpointIndex>,
) -> WrapWindow {
    if max_rows == 0 {
        return WrapWindow {
            ranges: Vec::new(),
            total_rows: None,
        };
    }

    if line.is_empty() || width == 0 {
        return WrapWindow {
            ranges: vec![WrapRange {
                start_char: 0,
                end_char: 0,
                start_byte: 0,
                end_byte: 0,
                continuation_indent: 0,
            }],
            total_rows: Some(1),
        };
    }

    let mut ranges = Vec::new();
    let checkpoint = checkpoints
        .as_deref()
        .map(|checkpoints| checkpoints.start_for(row_start))
        .unwrap_or(WrapCheckpoint {
            row: 0,
            start_byte: 0,
            start_char: 0,
        });
    let mut start_byte = checkpoint.start_byte;
    let mut start_char = checkpoint.start_char;
    let mut row = checkpoint.row;
    let target_end = row_start.saturating_add(max_rows);
    while start_byte < line.len() {
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            checkpoints.remember(WrapCheckpoint {
                row,
                start_byte,
                start_char,
            });
        }
        let continuation = row > 0;
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let (end_byte, end_char) = next_wrap_end(line, start_byte, start_char, row_width);
        if row >= row_start && row < target_end {
            ranges.push(WrapRange {
                start_char,
                end_char,
                start_byte,
                end_byte,
                continuation_indent: indent,
            });
        }
        start_byte = end_byte.max(start_byte + 1).min(line.len());
        start_char = end_char.max(start_char + 1);
        row = row.saturating_add(1);
        if row >= target_end && start_byte < line.len() {
            return WrapWindow {
                ranges,
                total_rows: None,
            };
        }
    }

    WrapWindow {
        ranges,
        total_rows: Some(row.max(1)),
    }
}

pub(super) fn next_wrap_end(
    line: &str,
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let hard_byte = start_byte.saturating_add(row_width.max(1)).min(line.len());
    if line.as_bytes()[start_byte..hard_byte].is_ascii() {
        return next_wrap_end_ascii(line.as_bytes(), start_byte, start_char, row_width);
    }

    let min_end = (row_width / 2).max(1);
    let mut consumed = 0_usize;
    let mut hard_end = None;
    let mut best_end = None;

    for (offset, ch) in line[start_byte..].char_indices() {
        if consumed >= row_width {
            break;
        }
        consumed += 1;
        let byte_end = start_byte + offset + ch.len_utf8();
        let char_end = start_char + consumed;
        hard_end = Some((byte_end, char_end));
        if consumed >= min_end && (ch.is_whitespace() || matches!(ch, ',' | '>' | '}' | ']' | ';'))
        {
            best_end = Some((byte_end, char_end));
        }
    }

    let Some(hard_end) = hard_end else {
        return (start_byte, start_char);
    };
    if hard_end.0 >= line.len() {
        return hard_end;
    }
    best_end.unwrap_or(hard_end)
}

pub(super) fn next_wrap_end_ascii(
    bytes: &[u8],
    start_byte: usize,
    start_char: usize,
    row_width: usize,
) -> (usize, usize) {
    let row_width = row_width.max(1);
    let hard_byte = start_byte.saturating_add(row_width).min(bytes.len());
    if hard_byte <= start_byte {
        return (start_byte, start_char);
    }
    if hard_byte >= bytes.len() {
        return (bytes.len(), start_char + (bytes.len() - start_byte));
    }

    let min_byte = start_byte + (row_width / 2).max(1).min(hard_byte - start_byte);
    for index in (min_byte..hard_byte).rev() {
        if is_ascii_wrap_boundary(bytes[index]) {
            let end_byte = index + 1;
            return (end_byte, start_char + (end_byte - start_byte));
        }
    }

    (hard_byte, start_char + (hard_byte - start_byte))
}

pub(super) fn is_ascii_wrap_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b',' | b'>' | b'}' | b']' | b';')
}

pub(super) fn continuation_indent(line: &str, width: usize) -> usize {
    if width < 8 {
        return 0;
    }

    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        + 2;
    indent.min(24).min(width / 2)
}

pub(super) fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    if end <= start {
        return Vec::new();
    }

    let mut sliced = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = char_count(text);
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let overlap_start = start.max(span_start);
        let overlap_end = end.min(span_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = slice_chars(text, overlap_start - span_start, overlap_end - span_start);
        push_styled_span(&mut sliced, text, span.style);
    }

    sliced
}

fn push_styled_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    let style = if style == plain_style() {
        Style::default()
    } else {
        style
    };

    if text.is_empty() {
        return;
    }

    if let Some(previous) = spans.last_mut()
        && previous.style == style
    {
        previous.content.to_mut().push_str(&text);
        return;
    }

    spans.push(Span::styled(text, style));
}

pub(super) fn slice_chars(text: &str, start: usize, end: usize) -> String {
    if end <= start {
        return String::new();
    }

    if text.is_ascii() {
        let start = start.min(text.len());
        let end = end.min(text.len());
        if end <= start {
            return String::new();
        }
        return text[start..end].to_owned();
    }

    text.chars().skip(start).take(end - start).collect()
}

pub(super) fn char_count(text: &str) -> usize {
    if text.is_ascii() {
        text.len()
    } else {
        text.chars().count()
    }
}

pub(super) fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len()
}

pub(super) fn viewer_progress_percent(
    file: &dyn ViewFile,
    context: RenderContext,
    logical_bottom: usize,
    viewport_bottom: Option<ViewportBottom>,
) -> usize {
    if !context.wrap {
        return progress_percent(logical_bottom, file.line_count());
    }

    let bottom = viewport_bottom
        .map(|bottom| viewport_bottom_byte_offset(file, bottom))
        .unwrap_or(0);
    byte_progress_percent(bottom, file.byte_len())
}

pub(super) fn viewport_bottom_byte_offset(file: &dyn ViewFile, bottom: ViewportBottom) -> u64 {
    if bottom.line_end {
        if bottom.line_index + 1 >= file.line_count() {
            return file.byte_len();
        }
        return file.byte_offset_for_line(bottom.line_index + 1);
    }

    file.byte_offset_for_line(bottom.line_index)
        .saturating_add(bottom.byte_end as u64)
}

pub(super) fn byte_index_for_char(line: &str, char_index: usize) -> usize {
    if line.is_ascii() {
        return char_index.min(line.len());
    }

    line.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

pub(super) fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

pub(super) fn byte_progress_percent(position: u64, total: u64) -> usize {
    if total == 0 {
        return 100;
    }

    position
        .min(total)
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(100) as usize
}
