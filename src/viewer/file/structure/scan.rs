use std::time::Duration;

use anyhow::Result;

use crate::{
    formats::{first_non_ws_byte, leading_indent, structure_anchor, structure_candidate_kind},
    load::ViewFile,
    transform::FormatKind,
};

use super::{
    StructureDirection, StructureTask, StructureViewport,
    candidate::{StructureCandidate, select_structure_candidate},
    visibility::{candidate_visibility, should_skip_candidate},
};
use crate::viewer::file::input::SearchTarget;

const STRUCTURE_CHUNK_LINES: usize = 4096;
const STRUCTURE_PRELOAD_RECORDS: usize = 64;
const STRUCTURE_PRELOAD_BUDGET: Duration = Duration::from_millis(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StructureStep {
    pub(super) found: Option<SearchTarget>,
    pub(super) next_line: usize,
    pub(super) scanned: usize,
}

pub(super) fn scan_structure_chunk(
    file: &dyn ViewFile,
    task: &StructureTask,
    format: FormatKind,
) -> Result<StructureStep> {
    match task.direction {
        StructureDirection::Forward => {
            scan_structure_forward(file, task.next_line, format, task.viewport)
        }
        StructureDirection::Backward => {
            scan_structure_backward(file, task.next_line, format, task.viewport)
        }
    }
}

fn scan_structure_forward(
    file: &dyn ViewFile,
    mut next_line: usize,
    format: FormatKind,
    viewport: Option<StructureViewport>,
) -> Result<StructureStep> {
    if next_line >= file.line_count() && !file.line_count_exact() {
        file.preload(
            STRUCTURE_CHUNK_LINES,
            STRUCTURE_PRELOAD_RECORDS,
            STRUCTURE_PRELOAD_BUDGET,
        )?;
    }

    let line_count = file.line_count();
    if line_count == 0 || next_line >= line_count {
        return Ok(StructureStep {
            found: None,
            next_line,
            scanned: 0,
        });
    }

    let count = line_count
        .saturating_sub(next_line)
        .min(STRUCTURE_CHUNK_LINES);
    let read_start = next_line.saturating_sub(1);
    let read_count = count.saturating_add(next_line.saturating_sub(read_start));
    let lines = file.read_window(read_start, read_count)?;
    if lines.is_empty() {
        return Ok(StructureStep {
            found: None,
            next_line,
            scanned: 0,
        });
    }

    let anchor = structure_anchor(&lines, read_start, next_line.saturating_sub(1), format);
    let mut candidates = Vec::new();
    let mut scanned = 0_usize;
    for offset in next_line - read_start..lines.len() {
        let line_number = read_start + offset;
        if let Some(kind) = structure_candidate_kind(
            format,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            let visibility = candidate_visibility(
                format,
                &lines,
                read_start,
                offset,
                file.line_count(),
                file.line_count_exact(),
                viewport,
            );
            if should_skip_candidate(kind, line_number, visibility) {
                scanned = scanned.saturating_add(1);
                continue;
            }
            candidates.push(StructureCandidate {
                line: line_number,
                byte_index: first_non_ws_byte(&lines[offset]),
                kind,
                indent: leading_indent(&lines[offset]),
            });
        }
        scanned = scanned.saturating_add(1);
    }

    if let Some(candidate) = select_structure_candidate(&candidates, format, anchor) {
        return Ok(StructureStep {
            found: Some(candidate.target()),
            next_line: candidate.line,
            scanned: candidate.line.saturating_sub(next_line).saturating_add(1),
        });
    }

    next_line = next_line.saturating_add(scanned);
    Ok(StructureStep {
        found: None,
        next_line,
        scanned,
    })
}

fn scan_structure_backward(
    file: &dyn ViewFile,
    next_line: usize,
    format: FormatKind,
    viewport: Option<StructureViewport>,
) -> Result<StructureStep> {
    let line_count = file.line_count();
    if line_count == 0 || next_line >= line_count {
        return Ok(StructureStep {
            found: None,
            next_line: usize::MAX,
            scanned: 0,
        });
    }

    let count = next_line.saturating_add(1).min(STRUCTURE_CHUNK_LINES);
    let start = next_line + 1 - count;
    let read_start = start.saturating_sub(1);
    let mut read_end = next_line.saturating_add(1);
    let anchor_line = next_line.saturating_add(1);
    if anchor_line < line_count {
        read_end = read_end.max(anchor_line.saturating_add(1));
    }
    if let Some(viewport) = viewport {
        read_end = read_end.max(viewport.bottom.saturating_add(1).min(line_count));
    }
    let read_count = read_end.saturating_sub(read_start);
    let lines = file.read_window(read_start, read_count)?;
    if lines.is_empty() {
        return Ok(StructureStep {
            found: None,
            next_line: usize::MAX,
            scanned: 0,
        });
    }

    let anchor = structure_anchor(&lines, read_start, anchor_line, format);
    let scan_end_offset = next_line.saturating_sub(read_start).min(lines.len() - 1);
    let mut candidates = Vec::new();
    for offset in (start - read_start..=scan_end_offset).rev() {
        let line_number = read_start + offset;
        if let Some(kind) = structure_candidate_kind(
            format,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            let visibility = candidate_visibility(
                format,
                &lines,
                read_start,
                offset,
                file.line_count(),
                file.line_count_exact(),
                viewport,
            );
            if should_skip_candidate(kind, line_number, visibility) {
                continue;
            }
            candidates.push(StructureCandidate {
                line: line_number,
                byte_index: first_non_ws_byte(&lines[offset]),
                kind,
                indent: leading_indent(&lines[offset]),
            });
        }
    }

    if let Some(candidate) = select_structure_candidate(&candidates, format, anchor) {
        return Ok(StructureStep {
            found: Some(candidate.target()),
            next_line: candidate.line,
            scanned: next_line.saturating_sub(candidate.line).saturating_add(1),
        });
    }

    Ok(StructureStep {
        found: None,
        next_line: start.checked_sub(1).unwrap_or(usize::MAX),
        scanned: count,
    })
}
