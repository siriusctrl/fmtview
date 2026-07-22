use std::io::{self, Write};

use fmtview_core::{RenderFrame, ScrollPosition, render_frame_to_buffer};
#[cfg(test)]
use fmtview_core::{ScrollDirection, ScrollHint};
use ratatui::{backend::Backend, buffer::Buffer, layout::Size};

use super::{scroll::draw_buffer_delta, terminal_writer::draw_cells_with_buffer};

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

    pub(crate) fn draw(&mut self, frame: RenderFrame) -> io::Result<()> {
        let mut current = self
            .scratch
            .take()
            .unwrap_or_else(|| Buffer::empty(frame.area));
        current.resize(frame.area);
        current.reset();
        let sticky_rows = frame.sticky.len();
        let position = frame.position;
        let selection_mode = frame.selection_mode;
        let scroll_hint = frame.scroll_hint;
        render_frame_to_buffer(&mut current, frame);
        match self.previous.take() {
            Some(previous)
                if previous.area == current.area
                    && self.previous_sticky_rows == sticky_rows
                    && self.previous_selection_mode == Some(selection_mode) =>
            {
                draw_buffer_delta(
                    &mut self.backend,
                    &previous,
                    &current,
                    scroll_hint,
                    sticky_rows,
                    &mut self.output,
                )?;
                self.scratch = Some(previous);
            }
            previous => {
                self.backend.clear()?;
                let empty = Buffer::empty(current.area);
                draw_cells_with_buffer(&mut self.backend, empty.diff(&current), &mut self.output)?;
                self.previous_position = None;
                self.previous_selection_mode = None;
                self.scratch = previous;
            }
        }
        self.backend.hide_cursor()?;
        Backend::flush(&mut self.backend)?;
        self.previous = Some(current);
        self.previous_position = Some(position);
        self.previous_sticky_rows = sticky_rows;
        self.previous_selection_mode = Some(selection_mode);
        Ok(())
    }

    pub(crate) fn previous_position(&self) -> Option<ScrollPosition> {
        self.previous_position
    }

    #[cfg(test)]
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
        if position.row_offset > previous.row_offset {
            Some(ScrollHint {
                amount,
                direction: ScrollDirection::Up,
            })
        } else {
            Some(ScrollHint {
                amount,
                direction: ScrollDirection::Down,
            })
        }
    }
}
