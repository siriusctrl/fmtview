use std::time::Duration;

use anyhow::Result;
use unicode_width::UnicodeWidthStr;

use crate::{load::ViewFile, syntax::SyntaxKind};

use super::{search::SearchTarget, state::ViewState};

const STRUCTURE_CHUNK_LINES: usize = 4096;
const STRUCTURE_PRELOAD_RECORDS: usize = 64;
const STRUCTURE_PRELOAD_BUDGET: Duration = Duration::from_millis(6);
const JSON_VISIBLE_COMPOSITE_LANDMARK_LINES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum StructureDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct StructureTask {
    direction: StructureDirection,
    next_line: usize,
    viewport: Option<StructureViewport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct StructureViewport {
    pub(in crate::viewer) top: usize,
    pub(in crate::viewer) top_row_offset: usize,
    pub(in crate::viewer) bottom: usize,
    pub(in crate::viewer) bottom_line_end: bool,
    pub(in crate::viewer) x: usize,
    pub(in crate::viewer) width: usize,
    pub(in crate::viewer) wrap: bool,
}

impl StructureViewport {
    fn matches_state(self, state: &ViewState) -> bool {
        self.top == state.top
            && self.top_row_offset == state.top_row_offset
            && self.x == state.x
            && self.wrap == state.wrap
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructureCandidateKind {
    JsonRecordStart,
    JsonArrayItemStart,
    JsonCompositeField,
    JsonRootStart,
    XmlStartTag,
    MarkdownHeading,
    TomlTable,
    JinjaBlock,
    PlainParagraph,
}

impl StructureCandidateKind {
    fn is_landmark_when_visible(self, line_span: Option<usize>) -> bool {
        match self {
            StructureCandidateKind::JsonRecordStart
            | StructureCandidateKind::JsonArrayItemStart
            | StructureCandidateKind::JsonRootStart
            | StructureCandidateKind::MarkdownHeading
            | StructureCandidateKind::TomlTable
            | StructureCandidateKind::JinjaBlock
            | StructureCandidateKind::PlainParagraph => true,
            StructureCandidateKind::XmlStartTag => line_span.is_none_or(|span| span > 1),
            StructureCandidateKind::JsonCompositeField => {
                line_span.is_some_and(|span| span >= JSON_VISIBLE_COMPOSITE_LANDMARK_LINES)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateVisibility {
    fully_observed: bool,
    end_line: Option<usize>,
}

impl CandidateVisibility {
    fn unknown() -> Self {
        Self {
            fully_observed: false,
            end_line: None,
        }
    }

    fn fully_observed(end_line: usize) -> Self {
        Self {
            fully_observed: true,
            end_line: Some(end_line),
        }
    }

    fn partially_observed(end_line: Option<usize>) -> Self {
        Self {
            fully_observed: false,
            end_line,
        }
    }

    fn line_span(self, start_line: usize) -> Option<usize> {
        self.end_line
            .map(|end_line| end_line.saturating_sub(start_line).saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StructureCandidate {
    line: usize,
    byte_index: usize,
    kind: StructureCandidateKind,
    indent: usize,
}

impl StructureCandidate {
    fn target(self) -> SearchTarget {
        SearchTarget {
            line: self.line,
            byte_index: self.byte_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StructureAnchor {
    line: usize,
    kind: Option<StructureCandidateKind>,
    indent: usize,
}

pub(in crate::viewer) fn start_structure_navigation(
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
    direction: StructureDirection,
) -> bool {
    state.structure_task = None;
    state.structure_target = None;
    state.search_target = None;
    state.search_task = None;
    if line_count == 0 {
        set_no_block_message(state, direction);
        return true;
    }

    let anchor = state.structure_cursor.unwrap_or(state.top);
    let Some(next_line) = structure_start_line(anchor, line_count, line_count_exact, direction)
    else {
        set_no_block_message(state, direction);
        return true;
    };

    state.search_message = None;
    let viewport = state
        .structure_viewport
        .filter(|viewport| viewport.matches_state(state));
    state.structure_task = Some(StructureTask {
        direction,
        next_line,
        viewport,
    });
    true
}

pub(in crate::viewer) fn process_structure_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
    syntax: SyntaxKind,
) -> Result<bool> {
    let Some(mut task) = state.structure_task.take() else {
        return Ok(false);
    };

    let step = scan_structure_chunk(file, &task, syntax)?;
    if let Some(target) = step.found {
        state.structure_target = Some(target);
        state.structure_cursor = Some(target.line);
        state.search_message = None;
        return Ok(true);
    }

    task.next_line = step.next_line;
    if step.scanned == 0 || reached_structure_scan_end(file, &task) {
        set_no_block_message(state, task.direction);
        return Ok(true);
    }

    state.structure_task = Some(task);
    Ok(false)
}

fn structure_start_line(
    top: usize,
    line_count: usize,
    line_count_exact: bool,
    direction: StructureDirection,
) -> Option<usize> {
    match direction {
        StructureDirection::Forward => {
            let next = top.saturating_add(1);
            if next < line_count || !line_count_exact {
                Some(next)
            } else {
                None
            }
        }
        StructureDirection::Backward => top.checked_sub(1),
    }
}

fn reached_structure_scan_end(file: &dyn ViewFile, task: &StructureTask) -> bool {
    match task.direction {
        StructureDirection::Forward => {
            file.line_count_exact() && task.next_line >= file.line_count()
        }
        StructureDirection::Backward => task.next_line == usize::MAX,
    }
}

fn no_block_message(direction: StructureDirection) -> &'static str {
    match direction {
        StructureDirection::Forward => "no next structure",
        StructureDirection::Backward => "no previous structure",
    }
}

fn set_no_block_message(state: &mut ViewState, direction: StructureDirection) {
    state.search_message = Some(no_block_message(direction).to_owned());
    if state.viewport_at_tail {
        state.preserve_tail_on_next_draw = true;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StructureStep {
    found: Option<SearchTarget>,
    next_line: usize,
    scanned: usize,
}

fn scan_structure_chunk(
    file: &dyn ViewFile,
    task: &StructureTask,
    syntax: SyntaxKind,
) -> Result<StructureStep> {
    match task.direction {
        StructureDirection::Forward => {
            scan_structure_forward(file, task.next_line, syntax, task.viewport)
        }
        StructureDirection::Backward => {
            scan_structure_backward(file, task.next_line, syntax, task.viewport)
        }
    }
}

fn scan_structure_forward(
    file: &dyn ViewFile,
    mut next_line: usize,
    syntax: SyntaxKind,
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

    let anchor = structure_anchor(&lines, read_start, next_line.saturating_sub(1), syntax);
    let mut candidates = Vec::new();
    let mut scanned = 0_usize;
    for offset in next_line - read_start..lines.len() {
        let line_number = read_start + offset;
        if let Some(kind) = structure_candidate_kind(
            syntax,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            let visibility = candidate_visibility(
                syntax,
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

    if let Some(candidate) = select_structure_candidate(&candidates, syntax, anchor) {
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
    syntax: SyntaxKind,
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

    let anchor = structure_anchor(&lines, read_start, anchor_line, syntax);
    let scan_end_offset = next_line.saturating_sub(read_start).min(lines.len() - 1);
    let mut candidates = Vec::new();
    for offset in (start - read_start..=scan_end_offset).rev() {
        let line_number = read_start + offset;
        if let Some(kind) = structure_candidate_kind(
            syntax,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            let visibility = candidate_visibility(
                syntax,
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

    if let Some(candidate) = select_structure_candidate(&candidates, syntax, anchor) {
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

fn should_skip_candidate(
    kind: StructureCandidateKind,
    start_line: usize,
    visibility: CandidateVisibility,
) -> bool {
    visibility.fully_observed && !kind.is_landmark_when_visible(visibility.line_span(start_line))
}

fn structure_anchor(
    lines: &[String],
    read_start: usize,
    line: usize,
    syntax: SyntaxKind,
) -> Option<StructureAnchor> {
    let offset = line.checked_sub(read_start)?;
    let content = lines.get(offset)?;
    let previous = offset
        .checked_sub(1)
        .and_then(|previous| lines.get(previous).map(String::as_str));
    Some(StructureAnchor {
        line,
        kind: structure_candidate_kind(syntax, content, previous),
        indent: leading_indent(content),
    })
}

fn select_structure_candidate(
    candidates: &[StructureCandidate],
    syntax: SyntaxKind,
    anchor: Option<StructureAnchor>,
) -> Option<StructureCandidate> {
    candidates
        .iter()
        .copied()
        .min_by_key(|candidate| structure_candidate_rank(*candidate, syntax, anchor))
}

fn structure_candidate_rank(
    candidate: StructureCandidate,
    syntax: SyntaxKind,
    anchor: Option<StructureAnchor>,
) -> (usize, usize, usize) {
    let distance = anchor
        .map(|anchor| anchor.line.abs_diff(candidate.line))
        .unwrap_or(candidate.line);
    if syntax != SyntaxKind::Structured {
        return (0, distance, 0);
    }

    let Some(anchor) = anchor else {
        return (0, distance, json_candidate_priority(candidate.kind));
    };

    match anchor.kind {
        Some(StructureCandidateKind::JsonArrayItemStart) => {
            let scope = usize::from(candidate.indent > anchor.indent);
            (scope, json_candidate_priority(candidate.kind), distance)
        }
        Some(StructureCandidateKind::JsonCompositeField) => {
            let scope = usize::from(candidate.indent <= anchor.indent);
            (scope, distance, json_candidate_priority(candidate.kind))
        }
        Some(StructureCandidateKind::JsonRecordStart | StructureCandidateKind::JsonRootStart) => {
            let scope = usize::from(candidate.kind == StructureCandidateKind::JsonRecordStart);
            (scope, distance, json_candidate_priority(candidate.kind))
        }
        _ => (0, distance, json_candidate_priority(candidate.kind)),
    }
}

fn json_candidate_priority(kind: StructureCandidateKind) -> usize {
    match kind {
        StructureCandidateKind::JsonArrayItemStart => 0,
        StructureCandidateKind::JsonRootStart => 1,
        StructureCandidateKind::JsonRecordStart => 2,
        StructureCandidateKind::JsonCompositeField => 3,
        StructureCandidateKind::XmlStartTag => 4,
        StructureCandidateKind::MarkdownHeading
        | StructureCandidateKind::TomlTable
        | StructureCandidateKind::JinjaBlock
        | StructureCandidateKind::PlainParagraph => 5,
    }
}

fn candidate_visibility(
    syntax: SyntaxKind,
    lines: &[String],
    read_start: usize,
    candidate_offset: usize,
    line_count: usize,
    line_count_exact: bool,
    viewport: Option<StructureViewport>,
) -> CandidateVisibility {
    let Some(viewport) = viewport else {
        return CandidateVisibility::unknown();
    };
    let start_line = read_start + candidate_offset;
    if start_line < viewport.top || start_line > viewport.bottom {
        return CandidateVisibility::unknown();
    }
    if start_line == viewport.top && viewport.top_row_offset > 0 {
        return CandidateVisibility::unknown();
    }

    let Some(end_line) = structure_block_end(
        syntax,
        lines,
        read_start,
        candidate_offset,
        viewport.bottom,
        line_count,
        line_count_exact,
    ) else {
        return CandidateVisibility::unknown();
    };
    if end_line > viewport.bottom {
        return CandidateVisibility::partially_observed(Some(end_line));
    }
    if end_line == viewport.bottom && !viewport.bottom_line_end {
        return CandidateVisibility::partially_observed(Some(end_line));
    }

    if block_is_horizontally_observed(lines, read_start, start_line, end_line, viewport) {
        CandidateVisibility::fully_observed(end_line)
    } else {
        CandidateVisibility::partially_observed(Some(end_line))
    }
}

fn structure_block_end(
    syntax: SyntaxKind,
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    match syntax {
        SyntaxKind::Structured => {
            structured_block_end(lines, read_start, start_offset, viewport_bottom)
        }
        SyntaxKind::Markdown => {
            markdown_block_end(lines, read_start, start_offset, viewport_bottom)
        }
        SyntaxKind::Toml => toml_block_end(lines, read_start, start_offset, viewport_bottom),
        SyntaxKind::Jinja => jinja_block_end(lines, read_start, start_offset, viewport_bottom),
        SyntaxKind::Plain => paragraph_block_end(lines, read_start, start_offset, viewport_bottom),
    }
    .or_else(|| {
        eof_block_end(
            lines,
            read_start,
            viewport_bottom,
            line_count,
            line_count_exact,
        )
    })
}

fn max_observed_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    if lines.is_empty() || viewport_bottom < read_start {
        return None;
    }
    Some((viewport_bottom - read_start).min(lines.len() - 1))
}

fn max_boundary_offset(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    max_observed_offset(lines, read_start, viewport_bottom.saturating_add(1))
}

fn following_lines(
    lines: &[String],
    start_offset: usize,
    max_offset: usize,
) -> impl Iterator<Item = (usize, &String)> {
    lines
        .iter()
        .enumerate()
        .take(max_offset + 1)
        .skip(start_offset + 1)
}

fn eof_block_end(
    lines: &[String],
    read_start: usize,
    viewport_bottom: usize,
    line_count: usize,
    line_count_exact: bool,
) -> Option<usize> {
    if !line_count_exact || line_count == 0 {
        return None;
    }
    let eof_line = line_count - 1;
    let read_end = read_start.saturating_add(lines.len());
    (eof_line <= viewport_bottom && eof_line < read_end).then_some(eof_line)
}

fn structured_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let line = lines.get(start_offset)?;
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        return xml_block_end(lines, read_start, start_offset, viewport_bottom)
            .or_else(|| indent_block_end(lines, read_start, start_offset, viewport_bottom));
    }
    json_block_end(lines, read_start, start_offset, viewport_bottom)
}

fn json_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let mut depth = 0_usize;
    let mut started = false;
    let mut in_string = false;
    let mut escaped = false;

    for (relative, line) in lines[start_offset..=max_offset].iter().enumerate() {
        let offset = start_offset + relative;
        let start_byte = if offset == start_offset {
            first_json_open_byte(line)?
        } else {
            0
        };
        for (_, ch) in line[start_byte..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' | '[' => {
                    depth = depth.saturating_add(1);
                    started = true;
                }
                '}' | ']' if started => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(read_start + offset);
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn first_json_open_byte(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => return Some(index),
            _ => {}
        }
    }
    None
}

fn xml_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let trimmed = lines.get(start_offset)?.trim_start();
    let tag = xml_start_tag_name(trimmed)?;
    if xml_tag_is_self_contained(trimmed, &tag) {
        return Some(read_start + start_offset);
    }

    let mut depth = 1_usize;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        let line = line.as_str();
        depth = depth.saturating_add(xml_start_tag_count(line, &tag));
        let closing = xml_end_tag_count(line, &tag);
        depth = depth.saturating_sub(closing);
        if closing > 0 && depth == 0 {
            return Some(read_start + offset);
        }
    }
    None
}

fn xml_start_tag_name(trimmed: &str) -> Option<String> {
    if !is_xml_start_tag(trimmed) {
        return None;
    }
    let name_end = trimmed[1..]
        .find(|ch: char| !is_xml_name_char(ch))
        .map(|index| index + 1)
        .unwrap_or(trimmed.len());
    (name_end > 1).then(|| trimmed[1..name_end].to_owned())
}

fn xml_tag_is_self_contained(trimmed: &str, tag: &str) -> bool {
    trimmed.contains("/>") || trimmed.contains(&format!("</{tag}>"))
}

fn xml_start_tag_count(line: &str, tag: &str) -> usize {
    let mut count = 0_usize;
    let mut rest = line;
    let needle = format!("<{tag}");
    while let Some(index) = rest.find(&needle) {
        let after = &rest[index + needle.len()..];
        if after.chars().next().is_none_or(|ch| !is_xml_name_char(ch))
            && !after.trim_start().starts_with("/>")
        {
            count = count.saturating_add(1);
        }
        let advance = after.chars().next().map(char::len_utf8).unwrap_or(0);
        rest = &after[advance..];
    }
    count
}

fn xml_end_tag_count(line: &str, tag: &str) -> usize {
    line.matches(&format!("</{tag}>")).count()
}

fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}

fn markdown_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_level = markdown_heading_level(lines.get(start_offset)?)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if markdown_heading_level(line).is_some_and(|level| level <= start_level) {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn toml_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if is_toml_table(line) {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn jinja_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_observed_offset(lines, read_start, viewport_bottom)?;
    let start_keyword = jinja_keyword(lines.get(start_offset)?)?;
    let Some(close_keyword) = jinja_close_keyword(start_keyword) else {
        return Some(read_start + start_offset);
    };
    let open_keyword = jinja_open_keyword(start_keyword);
    let mut depth = 1_usize;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        let Some(keyword) = jinja_keyword(line) else {
            continue;
        };
        if keyword == open_keyword {
            depth = depth.saturating_add(1);
        } else if keyword == close_keyword {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(read_start + offset);
            }
        }
    }
    None
}

fn paragraph_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn indent_block_end(
    lines: &[String],
    read_start: usize,
    start_offset: usize,
    viewport_bottom: usize,
) -> Option<usize> {
    let max_offset = max_boundary_offset(lines, read_start, viewport_bottom)?;
    let start_indent = leading_indent(lines.get(start_offset)?);
    for (offset, line) in following_lines(lines, start_offset, max_offset) {
        if line.trim().is_empty() {
            continue;
        }
        if leading_indent(line) <= start_indent {
            if is_same_indent_closing_line(line) {
                return Some(read_start + offset);
            }
            return Some(read_start + offset.saturating_sub(1));
        }
    }
    None
}

fn leading_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn is_same_indent_closing_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.starts_with("</")
        || jinja_keyword(trimmed).is_some_and(|keyword| keyword.starts_with("end"))
}

fn block_is_horizontally_observed(
    lines: &[String],
    read_start: usize,
    start_line: usize,
    end_line: usize,
    viewport: StructureViewport,
) -> bool {
    if viewport.wrap {
        return true;
    }
    if viewport.x > 0 || viewport.width == 0 {
        return false;
    }
    let start_offset = start_line.saturating_sub(read_start);
    let end_offset = end_line.saturating_sub(read_start);
    lines
        .get(start_offset..=end_offset)
        .is_some_and(|block| block.iter().all(|line| line.width() <= viewport.width))
}

#[cfg(test)]
pub(in crate::viewer) fn is_structure_point(
    syntax: SyntaxKind,
    line: &str,
    previous_line: Option<&str>,
) -> bool {
    structure_candidate_kind(syntax, line, previous_line).is_some()
}

fn structure_candidate_kind(
    syntax: SyntaxKind,
    line: &str,
    previous_line: Option<&str>,
) -> Option<StructureCandidateKind> {
    match syntax {
        SyntaxKind::Structured => structured_candidate_kind(line),
        SyntaxKind::Markdown => {
            is_markdown_heading(line).then_some(StructureCandidateKind::MarkdownHeading)
        }
        SyntaxKind::Toml => is_toml_table(line).then_some(StructureCandidateKind::TomlTable),
        SyntaxKind::Jinja => is_jinja_block(line).then_some(StructureCandidateKind::JinjaBlock),
        SyntaxKind::Plain => is_paragraph_start(line, previous_line)
            .then_some(StructureCandidateKind::PlainParagraph),
    }
}

fn structured_candidate_kind(line: &str) -> Option<StructureCandidateKind> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        return is_xml_start_tag(trimmed).then_some(StructureCandidateKind::XmlStartTag);
    }
    json_candidate_kind(line)
}

fn json_candidate_kind(line: &str) -> Option<StructureCandidateKind> {
    let indent = leading_indent(line);
    let trimmed = line.trim_start();
    let first = trimmed.as_bytes().first().copied()?;
    if matches!(first, b'{' | b'[') {
        return Some(if indent == 0 {
            StructureCandidateKind::JsonRecordStart
        } else if first == b'{' {
            StructureCandidateKind::JsonArrayItemStart
        } else {
            StructureCandidateKind::JsonRootStart
        });
    }
    let after_colon = json_value_after_key(trimmed)?;
    if after_colon.starts_with('{') || after_colon.starts_with('[') {
        Some(StructureCandidateKind::JsonCompositeField)
    } else {
        None
    }
}

fn json_value_after_key(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('"') {
        return None;
    }

    let mut escaped = false;
    for (index, ch) in trimmed.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                return trimmed[index + ch.len_utf8()..]
                    .trim_start()
                    .strip_prefix(':')
                    .map(str::trim_start);
            }
            _ => {}
        }
    }
    None
}

fn is_xml_start_tag(trimmed: &str) -> bool {
    if trimmed.starts_with("</")
        || trimmed.starts_with("<!")
        || trimmed.starts_with("<?")
        || trimmed == "<"
    {
        return false;
    }
    trimmed
        .as_bytes()
        .get(1)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
}

fn is_markdown_heading(line: &str) -> bool {
    markdown_heading_level(line).is_some()
}

fn markdown_heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    ((1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace))
    .then_some(hashes)
}

fn is_toml_table(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('[') && trimmed.contains(']') && !trimmed.starts_with("[]")
}

fn is_jinja_block(line: &str) -> bool {
    jinja_keyword(line).is_some_and(|keyword| {
        matches!(
            keyword,
            "block"
                | "endblock"
                | "if"
                | "elif"
                | "else"
                | "endif"
                | "for"
                | "endfor"
                | "macro"
                | "endmacro"
                | "filter"
                | "endfilter"
                | "call"
                | "endcall"
                | "include"
                | "extends"
                | "set"
                | "endset"
                | "with"
                | "endwith"
        )
    })
}

fn jinja_keyword(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("{%")?;
    rest.split_whitespace().next()
}

fn jinja_open_keyword(keyword: &str) -> &str {
    match keyword {
        "elif" | "else" => "if",
        _ => keyword,
    }
}

fn jinja_close_keyword(keyword: &str) -> Option<&'static str> {
    match keyword {
        "block" => Some("endblock"),
        "if" | "elif" | "else" => Some("endif"),
        "for" => Some("endfor"),
        "macro" => Some("endmacro"),
        "filter" => Some("endfilter"),
        "call" => Some("endcall"),
        "set" => Some("endset"),
        "with" => Some("endwith"),
        _ => None,
    }
}

fn is_paragraph_start(line: &str, previous_line: Option<&str>) -> bool {
    !line.trim().is_empty() && previous_line.is_none_or(|previous| previous.trim().is_empty())
}

fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}
