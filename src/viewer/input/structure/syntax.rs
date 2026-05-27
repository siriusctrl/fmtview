use crate::syntax::SyntaxKind;

use super::candidate::{StructureAnchor, StructureCandidateKind};

mod jinja;
mod json;
mod markdown;
mod plain;
mod toml;
mod xml;

pub(super) fn structure_anchor(
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

pub(super) fn structure_candidate_kind(
    syntax: SyntaxKind,
    line: &str,
    previous_line: Option<&str>,
) -> Option<StructureCandidateKind> {
    match syntax {
        SyntaxKind::Structured => structured_candidate_kind(line),
        SyntaxKind::Markdown => {
            markdown::is_heading(line).then_some(StructureCandidateKind::MarkdownHeading)
        }
        SyntaxKind::Toml => toml::is_table(line).then_some(StructureCandidateKind::TomlTable),
        SyntaxKind::Jinja => jinja::is_block(line).then_some(StructureCandidateKind::JinjaBlock),
        SyntaxKind::Plain => plain::is_paragraph_start(line, previous_line)
            .then_some(StructureCandidateKind::PlainParagraph),
    }
}

pub(super) fn structure_block_end(
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
            markdown::block_end(lines, read_start, start_offset, viewport_bottom)
        }
        SyntaxKind::Toml => toml::block_end(lines, read_start, start_offset, viewport_bottom),
        SyntaxKind::Jinja => jinja::block_end(lines, read_start, start_offset, viewport_bottom),
        SyntaxKind::Plain => plain::block_end(lines, read_start, start_offset, viewport_bottom),
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

pub(super) fn leading_indent(line: &str) -> usize {
    line.chars().take_while(|ch| ch.is_whitespace()).count()
}

pub(super) fn first_non_ws_byte(line: &str) -> usize {
    line.char_indices()
        .find_map(|(index, ch)| (!ch.is_whitespace()).then_some(index))
        .unwrap_or(0)
}

fn structured_candidate_kind(line: &str) -> Option<StructureCandidateKind> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        return xml::is_start_tag(trimmed).then_some(StructureCandidateKind::XmlStartTag);
    }
    json::candidate_kind(line)
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
        return xml::block_end(lines, read_start, start_offset, viewport_bottom)
            .or_else(|| indent_block_end(lines, read_start, start_offset, viewport_bottom));
    }
    json::block_end(lines, read_start, start_offset, viewport_bottom)
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

fn is_same_indent_closing_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('}')
        || trimmed.starts_with(']')
        || trimmed.starts_with("</")
        || jinja::keyword(trimmed).is_some_and(|keyword| keyword.starts_with("end"))
}
