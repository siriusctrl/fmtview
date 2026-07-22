use std::io::Write;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::{IndexedTempFile, StructureViewport};
use crate::viewer::{InputEvent, KeyModifiers, MouseEventKind};

pub(super) fn span_text(spans: &[Span<'static>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

pub(super) fn background_cell_count(lines: &[Line<'static>]) -> usize {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.style.bg.is_some())
        .map(|span| span.content.chars().count())
        .sum()
}

pub(super) fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
    spans
        .iter()
        .filter(|span| span.content.as_ref() == text)
        .map(|span| span.style)
        .collect()
}

pub(super) fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> InputEvent {
    InputEvent::Mouse { kind, modifiers }
}

pub(super) fn indexed_lines(lines: &[&str]) -> IndexedTempFile {
    indexed_file(lines)
}

pub(super) fn indexed_file(lines: &[&str]) -> IndexedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(temp, "{line}").unwrap();
    }
    temp.flush().unwrap();
    IndexedTempFile::new("test".to_owned(), temp).unwrap()
}

pub(super) fn structure_viewport(top: usize, bottom: usize) -> StructureViewport {
    StructureViewport {
        top,
        top_row_offset: 0,
        bottom,
        bottom_line_end: true,
        x: 0,
        width: 80,
        wrap: true,
    }
}
