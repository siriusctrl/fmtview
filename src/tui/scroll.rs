use std::io::{self, Write};

use ratatui::{
    buffer::{Buffer, Cell},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier},
};

use super::ansi::draw_cells_with_buffer;

#[derive(Debug, Clone, Copy)]
pub(super) enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollHint {
    pub(super) amount: u16,
    pub(super) direction: ScrollDirection,
}

impl ScrollHint {
    pub(crate) fn up(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Up,
        }
    }

    pub(crate) fn down(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Down,
        }
    }
}

pub(super) fn draw_buffer_delta<B>(
    backend: &mut B,
    previous: &Buffer,
    current: &Buffer,
    scroll_hint: Option<ScrollHint>,
    sticky_rows: usize,
    output: &mut Vec<u8>,
) -> io::Result<()>
where
    B: Write,
{
    if let Some(hint) = scroll_hint {
        let scroll = BodyScroll {
            area: scrollable_body_area(current.area, sticky_rows),
            amount: hint.amount,
            direction: hint.direction,
        };
        if scroll.area.height > scroll.amount {
            let scrolled_updates = scroll.updates(previous, current);
            scroll.emit(backend)?;
            return draw_cells_with_buffer(backend, scrolled_updates, output);
        }
    }

    draw_cells_with_buffer(backend, previous.diff(current), output)
}

#[derive(Debug, Clone, Copy)]
struct BodyScroll {
    area: Rect,
    amount: u16,
    direction: ScrollDirection,
}

impl BodyScroll {
    fn emit<B>(self, backend: &mut B) -> io::Result<()>
    where
        B: Write,
    {
        if self.amount == 0 || self.area.height == 0 {
            return Ok(());
        }

        let top = self.area.y.saturating_add(1);
        let bottom = self.area.y.saturating_add(self.area.height);
        let command = match self.direction {
            ScrollDirection::Up => 'S',
            ScrollDirection::Down => 'T',
        };
        write!(
            backend,
            "\x1b[{top};{bottom}r\x1b[{}{command}\x1b[r",
            self.amount
        )
    }

    fn updates<'a>(self, previous: &Buffer, current: &'a Buffer) -> Vec<(u16, u16, &'a Cell)> {
        let mut updates = Vec::new();
        self.push_static_row_updates(previous, current, &mut updates);
        for y in self.entering_rows() {
            self.push_entering_row(current, y, &mut updates);
        }

        updates
    }

    fn entering_rows(self) -> std::ops::Range<u16> {
        match self.direction {
            ScrollDirection::Up => {
                self.area.y.saturating_add(self.area.height - self.amount)
                    ..self.area.y.saturating_add(self.area.height)
            }
            ScrollDirection::Down => self.area.y..self.area.y.saturating_add(self.amount),
        }
    }

    fn push_entering_row<'a>(
        self,
        current: &'a Buffer,
        y: u16,
        updates: &mut Vec<(u16, u16, &'a Cell)>,
    ) {
        for x in self.area.x..self.area.x.saturating_add(self.area.width) {
            let cell = &current[(x, y)];
            if !cell.skip && !is_visually_empty_cell(cell) {
                updates.push((x, y, cell));
            }
        }
    }

    fn push_static_row_updates<'a>(
        self,
        previous: &Buffer,
        current: &'a Buffer,
        updates: &mut Vec<(u16, u16, &'a Cell)>,
    ) {
        for y in 0..current.area.height {
            if y >= self.area.y && y < self.area.y.saturating_add(self.area.height) {
                continue;
            }
            for x in self.area.x..self.area.x.saturating_add(self.area.width) {
                let cell = &current[(x, y)];
                if !cell.skip && cell != &previous[(x, y)] {
                    updates.push((x, y, cell));
                }
            }
        }
    }
}

fn is_visually_empty_cell(cell: &Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset && cell.modifier == Modifier::empty()
}

fn scrollable_body_area(area: Rect, sticky_rows: usize) -> Rect {
    let [body, _footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    Rect {
        x: 0,
        y: body.y.saturating_add(1).saturating_add(sticky_rows as u16),
        width: body.width,
        height: body
            .height
            .saturating_sub(2)
            .saturating_sub(sticky_rows as u16),
    }
}
