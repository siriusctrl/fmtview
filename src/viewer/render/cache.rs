use std::collections::{HashMap, VecDeque, hash_map::Entry};

use anyhow::Result;
use ratatui::text::Line;

use crate::load::ViewFile;
use crate::syntax::HighlightCheckpointIndex;

use super::super::{RENDER_CACHE_MAX_LINES, WRAP_RENDER_CHUNK_ROWS, WRAP_RENDER_CHUNKS_PER_LINE};
use super::{
    line::render_logical_line_window_with_status_indexed, types::RenderRequest,
    wrap::WrapCheckpointIndex,
};

#[derive(Debug, Default)]
pub(in crate::viewer) struct LineWindowCache {
    pub(in crate::viewer) start: usize,
    pub(in crate::viewer) lines: Vec<String>,
}

pub(in crate::viewer) struct LineWindow<'a> {
    pub(in crate::viewer) lines: &'a [String],
}

impl LineWindowCache {
    pub(in crate::viewer) fn read(
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
pub(in crate::viewer) struct RenderedLineCache {
    pub(in crate::viewer) request: Option<RenderRequest>,
    pub(in crate::viewer) lines: HashMap<usize, CachedRenderedLine>,
    pub(in crate::viewer) order: VecDeque<usize>,
}

#[derive(Debug, Clone)]
pub(in crate::viewer) struct RenderedVisualRow {
    pub(in crate::viewer) line: Line<'static>,
    pub(in crate::viewer) end_byte: usize,
    pub(in crate::viewer) line_end: bool,
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct CachedRenderedLine {
    pub(in crate::viewer) chunks: VecDeque<RenderedLineChunk>,
    pub(in crate::viewer) total_rows: Option<usize>,
    pub(in crate::viewer) index: LineRenderIndex,
}

#[derive(Debug)]
pub(in crate::viewer) struct RenderedLineChunk {
    pub(in crate::viewer) start_row: usize,
    pub(in crate::viewer) rows: Vec<RenderedVisualRow>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::viewer) struct RenderedLineStatus {
    pub(in crate::viewer) known_rows: usize,
    pub(in crate::viewer) total_rows: Option<usize>,
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct LineRenderIndex {
    pub(in crate::viewer) wrap: WrapCheckpointIndex,
    pub(in crate::viewer) highlight: HighlightCheckpointIndex,
}

impl RenderedLineCache {
    pub(in crate::viewer) fn get_or_render(
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

    pub(in crate::viewer) fn get_or_render_window(
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

    pub(in crate::viewer) fn status(&self, line_number: usize) -> RenderedLineStatus {
        self.lines
            .get(&line_number)
            .map(CachedRenderedLine::status)
            .unwrap_or(RenderedLineStatus {
                known_rows: 0,
                total_rows: None,
            })
    }

    pub(in crate::viewer) fn evict_until_room(&mut self) {
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
    pub(in crate::viewer) fn render_window(
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

    pub(in crate::viewer) fn cached_window(
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

    pub(in crate::viewer) fn status(&self) -> RenderedLineStatus {
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
