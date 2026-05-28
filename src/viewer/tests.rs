use std::{
    cell::{Cell, RefCell},
    io::{self, Write},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};
use tempfile::NamedTempFile;

use super::input::*;
use super::navigation::*;
use super::render::*;
use super::*;
use crate::{
    input::InputSource,
    load::LazyTransformedRecordsFile,
    syntax::{SyntaxKind, highlight_json_like, highlight_xml_line},
    transform::{FormatKind, FormatOptions},
};

// Correctness tests run by default and should avoid wall-clock assertions.

mod cache;
mod input;
mod navigation;
mod perf;
mod render;
mod screen;
mod search;
mod structure;
mod syntax;
mod viewport;

fn span_text(spans: &[Span<'static>]) -> String {
    spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn background_cell_count(lines: &[Line<'static>]) -> usize {
    lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| span.style.bg.is_some())
        .map(|span| span.content.chars().count())
        .sum()
}

struct CountingWriter {
    bytes: Rc<Cell<usize>>,
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

struct CapturingWriter {
    output: Rc<RefCell<Vec<u8>>>,
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

fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
    spans
        .iter()
        .filter(|span| span.content.as_ref() == text)
        .map(|span| span.style)
        .collect()
}

fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> Event {
    Event::Mouse(crossterm::event::MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers,
    })
}

fn indexed_lines(lines: &[&str]) -> IndexedTempFile {
    let mut temp = NamedTempFile::new().unwrap();
    for line in lines {
        writeln!(temp, "{line}").unwrap();
    }
    IndexedTempFile::new("test".to_owned(), temp).unwrap()
}
