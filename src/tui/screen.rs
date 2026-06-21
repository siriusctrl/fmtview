use std::io::{self, Write};

use ratatui::{
    backend::Backend,
    buffer::Buffer,
    layout::{Rect, Size},
    style::Style,
    text::Line,
};

use super::{
    ansi::draw_cells_with_buffer,
    frame::render_frame,
    scroll::{ScrollDirection, draw_buffer_delta},
};

#[cfg(test)]
pub(crate) use super::ansi::draw_cells;
pub(crate) use super::scroll::ScrollHint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollPosition {
    pub(crate) top: usize,
    pub(crate) row_offset: usize,
}

pub(crate) struct TerminalFrame {
    pub(crate) area: Rect,
    pub(crate) styled: Vec<Line<'static>>,
    pub(crate) sticky: Vec<Line<'static>>,
    pub(crate) selection_mode: bool,
    pub(crate) title: String,
    pub(crate) footer_text: String,
    pub(crate) footer_style: Style,
    pub(crate) position: ScrollPosition,
    pub(crate) scroll_hint: Option<ScrollHint>,
}

pub(crate) struct ViewerTerminal<B> {
    backend: B,
    previous: Option<Buffer>,
    scratch: Option<Buffer>,
    previous_position: Option<ScrollPosition>,
    previous_sticky_rows: usize,
    previous_selection_mode: Option<bool>,
    output: Vec<u8>,
}

impl<B> ViewerTerminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(crate) fn new(backend: B) -> Self {
        Self {
            backend,
            previous: None,
            scratch: None,
            previous_position: None,
            previous_sticky_rows: 0,
            previous_selection_mode: None,
            output: Vec::with_capacity(16 * 1024),
        }
    }

    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub(crate) fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    pub(crate) fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    pub(crate) fn draw(&mut self, frame: TerminalFrame) -> io::Result<()> {
        let mut current = self
            .scratch
            .take()
            .unwrap_or_else(|| Buffer::empty(frame.area));
        current.resize(frame.area);
        current.reset();
        let sticky_rows = frame.sticky.len();
        render_frame(
            &mut current,
            frame.styled,
            frame.sticky,
            frame.selection_mode,
            frame.title,
            frame.footer_text,
            frame.footer_style,
        );
        match self.previous.take() {
            Some(previous)
                if previous.area == current.area
                    && self.previous_sticky_rows == sticky_rows
                    && self.previous_selection_mode == Some(frame.selection_mode) =>
            {
                draw_buffer_delta(
                    &mut self.backend,
                    &previous,
                    &current,
                    frame.scroll_hint,
                    sticky_rows,
                    &mut self.output,
                )?;
                self.scratch = Some(previous);
            }
            previous => {
                self.backend.clear()?;
                let empty = Buffer::empty(frame.area);
                draw_cells_with_buffer(&mut self.backend, empty.diff(&current), &mut self.output)?;
                self.previous_position = None;
                self.previous_selection_mode = None;
                self.scratch = previous;
            }
        }
        self.backend.hide_cursor()?;
        Backend::flush(&mut self.backend)?;
        self.previous = Some(current);
        self.previous_position = Some(frame.position);
        self.previous_sticky_rows = sticky_rows;
        self.previous_selection_mode = Some(frame.selection_mode);
        Ok(())
    }

    pub(crate) fn scroll_hint(&self, position: ScrollPosition) -> Option<ScrollHint> {
        let previous = self.previous_position?;
        if previous.top != position.top {
            return None;
        }

        let delta = position.row_offset.abs_diff(previous.row_offset);
        if delta == 0 || delta > 12 {
            return None;
        }
        let amount = u16::try_from(delta).ok()?;
        let direction = if position.row_offset > previous.row_offset {
            ScrollDirection::Up
        } else {
            ScrollDirection::Down
        };
        Some(ScrollHint { amount, direction })
    }

    pub(crate) fn previous_position(&self) -> Option<ScrollPosition> {
        self.previous_position
    }
}
