use std::{
    cell::{Cell, RefCell},
    io::{self, Write},
    rc::Rc,
    time::{Duration, Instant},
};

use fmtview_core::{RenderFrame, ScrollPosition};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
};

use super::{screen::ViewerTerminal, terminal_writer::draw_cells};

#[test]
fn terminal_writer_writes_compact_indexed_colors() {
    let mut cell = ratatui::buffer::Cell::EMPTY;
    cell.set_symbol("x").set_fg(Color::Indexed(75));
    let mut output = Vec::new();

    draw_cells(&mut output, vec![(0, 0, &cell)]).unwrap();

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("\x1b[38;5;75m"));
    assert!(!output.contains("\x1b[49m"));
}

#[test]
fn terminal_renderer_applies_plain_style_to_default_spans() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let backend = CrosstermBackend::new(CapturingWriter::new(Rc::clone(&output)));
    let mut terminal = ViewerTerminal::new(backend);

    terminal.draw(frame(false, 0)).unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    let begin = output.find("\x1b[?2026h").unwrap();
    let content = output.find("plain text").unwrap();
    let end = output.rfind("\x1b[?2026l").unwrap();
    assert!(begin < content && content < end, "{output:?}");
    assert!(output.contains("\x1b[38;5;145mplain text"));
}

#[test]
fn selection_mode_draws_body_without_frame() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let backend = CrosstermBackend::new(CapturingWriter::new(Rc::clone(&output)));
    let mut terminal = ViewerTerminal::new(backend);

    terminal.draw(frame(true, 0)).unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    assert!(output.contains("plain text"));
    assert!(!output.contains("┌"));
    assert!(!output.contains("│"));
}

#[test]
fn selection_mode_change_forces_full_redraw_even_with_scroll_hint() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let backend = CrosstermBackend::new(CapturingWriter::new(Rc::clone(&output)));
    let mut terminal = ViewerTerminal::new(backend);

    terminal.draw(frame(true, 0)).unwrap();
    let mut next = frame(false, 1);
    next.scroll_hint = terminal.scroll_hint(next.position);
    assert!(next.scroll_hint.is_some());
    terminal.draw(next).unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    assert!(output.matches("\x1b[2J").count() >= 2);
}

#[test]
#[ignore = "performance smoke; run benches/viewer-performance.sh"]
fn perf_terminal_scroll_draw_bytes() {
    let byte_count = Rc::new(Cell::new(0_usize));
    let backend = CrosstermBackend::new(CountingWriter::new(Rc::clone(&byte_count)));
    let mut terminal = ViewerTerminal::new(backend);
    let started = Instant::now();
    let mut rendered_rows = 0;

    for top in 0..400 {
        let position = ScrollPosition { top, row_offset: 0 };
        let rows = terminal_rows(top);
        rendered_rows += rows.len();
        let frame = RenderFrame {
            area: Rect::new(0, 0, 120, 35),
            styled: rows,
            sticky: Vec::new(),
            selection_mode: false,
            title: " perf ".to_owned(),
            footer_text: " q/Esc quit ".to_owned(),
            footer_style: Style::default(),
            position,
            scroll_hint: terminal.scroll_hint(position),
        };
        terminal.draw(frame).unwrap();
    }

    let elapsed = started.elapsed();
    eprintln!(
        "terminal scroll draw: {elapsed:?}, rows={rendered_rows}, bytes={}, background_cells=0",
        byte_count.get()
    );
    assert!(byte_count.get() > 0);
    assert!(elapsed < Duration::from_millis(1_500));
}

#[test]
#[ignore = "performance smoke; run benches/viewer-performance.sh"]
fn perf_terminal_visual_row_scroll_bytes() {
    let byte_count = Rc::new(Cell::new(0_usize));
    let backend = CrosstermBackend::new(CountingWriter::new(Rc::clone(&byte_count)));
    let mut terminal = ViewerTerminal::new(backend);
    let started = Instant::now();
    let mut rendered_rows = 0;

    for row_offset in 0..400 {
        let position = ScrollPosition { top: 0, row_offset };
        let rows = terminal_rows(row_offset);
        rendered_rows += rows.len();
        let frame = RenderFrame {
            area: Rect::new(0, 0, 120, 35),
            styled: rows,
            sticky: Vec::new(),
            selection_mode: false,
            title: " perf ".to_owned(),
            footer_text: " q/Esc quit ".to_owned(),
            footer_style: Style::default(),
            position,
            scroll_hint: terminal.scroll_hint(position),
        };
        terminal.draw(frame).unwrap();
    }

    let elapsed = started.elapsed();
    eprintln!(
        "terminal visual row scroll: {elapsed:?}, rows={rendered_rows}, bytes={}, background_cells=0",
        byte_count.get()
    );
    assert!(byte_count.get() > 0);
    assert!(elapsed < Duration::from_millis(1_500));
}

fn frame(selection_mode: bool, row_offset: usize) -> RenderFrame {
    RenderFrame {
        area: Rect::new(0, 0, 24, 5),
        styled: vec![Line::from(Span::raw("plain text"))],
        sticky: Vec::new(),
        selection_mode,
        title: " test ".to_owned(),
        footer_text: " footer ".to_owned(),
        footer_style: Style::default(),
        position: ScrollPosition { top: 0, row_offset },
        scroll_hint: None,
    }
}

fn terminal_rows(seed: usize) -> Vec<Line<'static>> {
    (0..32)
        .map(|row| Line::from(format!("row {row:02} seed {seed:03} {}", "x".repeat(80))))
        .collect()
}

struct CapturingWriter {
    output: Rc<RefCell<Vec<u8>>>,
}

impl CapturingWriter {
    fn new(output: Rc<RefCell<Vec<u8>>>) -> Self {
        Self { output }
    }
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

struct CountingWriter {
    bytes: Rc<Cell<usize>>,
}

impl CountingWriter {
    fn new(bytes: Rc<Cell<usize>>) -> Self {
        Self { bytes }
    }
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
