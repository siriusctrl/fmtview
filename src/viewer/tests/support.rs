use std::{
    cell::{Cell, RefCell},
    io::{self, Write},
    rc::Rc,
};

use crossterm::event::{Event, KeyModifiers, MouseEventKind};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::{IndexedTempFile, StructureViewport};

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

pub(super) struct CountingWriter {
    pub(super) bytes: Rc<Cell<usize>>,
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.set(self.bytes.get().saturating_add(buf.len()));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct CapturingWriter {
    pub(super) output: Rc<RefCell<Vec<u8>>>,
}

impl Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
    spans
        .iter()
        .filter(|span| span.content.as_ref() == text)
        .map(|span| span.style)
        .collect()
}

pub(super) fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> Event {
    Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers,
    })
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
