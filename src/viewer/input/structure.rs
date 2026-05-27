use std::time::Duration;

use anyhow::Result;

use crate::{load::ViewFile, syntax::SyntaxKind};

use super::{search::SearchTarget, state::ViewState};

const STRUCTURE_CHUNK_LINES: usize = 4096;
const STRUCTURE_PRELOAD_RECORDS: usize = 64;
const STRUCTURE_PRELOAD_BUDGET: Duration = Duration::from_millis(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum StructureDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct StructureTask {
    direction: StructureDirection,
    next_line: usize,
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
        state.search_message = Some(no_block_message(direction).to_owned());
        return true;
    }

    let anchor = state.structure_cursor.unwrap_or(state.top);
    let Some(next_line) = structure_start_line(anchor, line_count, line_count_exact, direction)
    else {
        state.search_message = Some(no_block_message(direction).to_owned());
        return true;
    };

    state.search_message = None;
    state.structure_task = Some(StructureTask {
        direction,
        next_line,
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
        state.search_message = Some(no_block_message(task.direction).to_owned());
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
        StructureDirection::Forward => "no next block",
        StructureDirection::Backward => "no previous block",
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
        StructureDirection::Forward => scan_structure_forward(file, task.next_line, syntax),
        StructureDirection::Backward => scan_structure_backward(file, task.next_line, syntax),
    }
}

fn scan_structure_forward(
    file: &dyn ViewFile,
    mut next_line: usize,
    syntax: SyntaxKind,
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

    let mut scanned = 0_usize;
    for offset in next_line - read_start..lines.len() {
        let line_number = read_start + offset;
        if is_structure_point(
            syntax,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            return Ok(StructureStep {
                found: Some(SearchTarget {
                    line: line_number,
                    byte_index: first_non_ws_byte(&lines[offset]),
                }),
                next_line: line_number,
                scanned: scanned.saturating_add(1),
            });
        }
        scanned = scanned.saturating_add(1);
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
    let read_count = count.saturating_add(start.saturating_sub(read_start));
    let lines = file.read_window(read_start, read_count)?;
    if lines.is_empty() {
        return Ok(StructureStep {
            found: None,
            next_line: usize::MAX,
            scanned: 0,
        });
    }

    for offset in (start - read_start..lines.len()).rev() {
        let line_number = read_start + offset;
        if is_structure_point(
            syntax,
            lines.get(offset).map(String::as_str).unwrap_or_default(),
            lines.get(offset.saturating_sub(1)).map(String::as_str),
        ) {
            return Ok(StructureStep {
                found: Some(SearchTarget {
                    line: line_number,
                    byte_index: first_non_ws_byte(&lines[offset]),
                }),
                next_line: line_number,
                scanned: next_line.saturating_sub(line_number).saturating_add(1),
            });
        }
    }

    Ok(StructureStep {
        found: None,
        next_line: start.checked_sub(1).unwrap_or(usize::MAX),
        scanned: count,
    })
}

pub(in crate::viewer) fn is_structure_point(
    syntax: SyntaxKind,
    line: &str,
    previous_line: Option<&str>,
) -> bool {
    match syntax {
        SyntaxKind::Structured => is_structured_point(line),
        SyntaxKind::Markdown => is_markdown_heading(line),
        SyntaxKind::Toml => is_toml_table(line),
        SyntaxKind::Jinja => is_jinja_block(line),
        SyntaxKind::Plain => is_paragraph_start(line, previous_line),
    }
}

fn is_structured_point(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        return is_xml_start_tag(trimmed);
    }
    is_json_composite_point(trimmed)
}

fn is_json_composite_point(trimmed: &str) -> bool {
    let Some(first) = trimmed.as_bytes().first().copied() else {
        return false;
    };
    if matches!(first, b'{' | b'[') {
        return true;
    }
    let Some(after_colon) = json_value_after_key(trimmed) else {
        return false;
    };
    after_colon.starts_with('{') || after_colon.starts_with('[')
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
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace)
}

fn is_toml_table(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('[') && trimmed.contains(']') && !trimmed.starts_with("[]")
}

fn is_jinja_block(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("{%") else {
        return false;
    };
    let keyword = rest.split_whitespace().next().unwrap_or_default();
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
            | "with"
            | "endwith"
    )
}

fn is_paragraph_start(line: &str, previous_line: Option<&str>) -> bool {
    !line.trim().is_empty() && previous_line.is_none_or(|previous| previous.trim().is_empty())
}

fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}
