use std::io::{self, Write};

use ratatui::{
    backend::Backend,
    buffer::{Buffer, Cell},
    layout::{Constraint, Layout, Rect, Size},
    style::{Color, Modifier},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};

use super::{
    palette::{gutter_style, plain_style},
    render::ViewPosition,
};

pub(super) struct ViewerTerminal<B> {
    backend: B,
    previous: Option<Buffer>,
    previous_position: Option<ViewPosition>,
}

impl<B> ViewerTerminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(super) fn new(backend: B) -> Self {
        Self {
            backend,
            previous: None,
            previous_position: None,
        }
    }

    pub(super) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub(super) fn size(&self) -> io::Result<Size> {
        self.backend.size()
    }

    pub(super) fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    pub(super) fn draw(
        &mut self,
        area: Rect,
        styled: Vec<Line<'static>>,
        title: String,
        footer_text: String,
        position: ViewPosition,
        scroll_hint: Option<ScrollHint>,
    ) -> io::Result<()> {
        let mut current = Buffer::empty(area);
        render_frame(&mut current, styled, title, footer_text);
        match &self.previous {
            Some(previous) if previous.area == current.area => {
                draw_diff(&mut self.backend, previous, &current, scroll_hint)?;
            }
            _ => {
                self.backend.clear()?;
                let empty = Buffer::empty(area);
                draw_cells(&mut self.backend, empty.diff(&current))?;
                self.previous_position = None;
            }
        }
        self.backend.hide_cursor()?;
        Backend::flush(&mut self.backend)?;
        self.previous = Some(current);
        self.previous_position = Some(position);
        Ok(())
    }

    pub(super) fn scroll_hint(&self, position: ViewPosition) -> Option<ScrollHint> {
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
}

fn draw_diff<B>(
    backend: &mut B,
    previous: &Buffer,
    current: &Buffer,
    scroll_hint: Option<ScrollHint>,
) -> io::Result<()>
where
    B: Backend<Error = io::Error> + Write,
{
    if let Some(hint) = scroll_hint {
        let scroll = BodyScroll {
            area: scrollable_body_area(current.area),
            amount: hint.amount,
            direction: hint.direction,
        };
        if scroll.area.height > scroll.amount {
            let scrolled_updates = scroll.updates(previous, current);
            scroll.emit(backend)?;
            return draw_cells(backend, scrolled_updates);
        }
    }

    draw_cells(backend, previous.diff(current))
}

pub(super) fn draw_cells<'a, B, I>(backend: &mut B, content: I) -> io::Result<()>
where
    B: Write,
    I: IntoIterator<Item = (u16, u16, &'a Cell)>,
{
    let mut output = Vec::with_capacity(16 * 1024);
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut modifier = Modifier::empty();
    let mut last_pos: Option<(u16, u16)> = None;

    for (x, y, cell) in content {
        if !matches!(last_pos, Some((last_x, last_y)) if x == last_x.saturating_add(1) && y == last_y)
        {
            write!(
                output,
                "\x1b[{};{}H",
                y.saturating_add(1),
                x.saturating_add(1)
            )?;
        }
        last_pos = Some((x, y));

        if cell.modifier != modifier {
            write!(output, "\x1b[0m")?;
            fg = Color::Reset;
            bg = Color::Reset;
            write_modifier(&mut output, cell.modifier)?;
            modifier = cell.modifier;
        }
        if cell.fg != fg {
            write_fg(&mut output, cell.fg)?;
            fg = cell.fg;
        }
        if cell.bg != bg {
            write_bg(&mut output, cell.bg)?;
            bg = cell.bg;
        }
        output.extend_from_slice(cell.symbol().as_bytes());
    }

    output.extend_from_slice(b"\x1b[0m");
    backend.write_all(&output)
}

fn write_modifier<B>(backend: &mut B, modifier: Modifier) -> io::Result<()>
where
    B: Write,
{
    if modifier.contains(Modifier::BOLD) {
        write!(backend, "\x1b[1m")?;
    }
    if modifier.contains(Modifier::DIM) {
        write!(backend, "\x1b[2m")?;
    }
    if modifier.contains(Modifier::ITALIC) {
        write!(backend, "\x1b[3m")?;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        write!(backend, "\x1b[4m")?;
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        write!(backend, "\x1b[5m")?;
    }
    if modifier.contains(Modifier::RAPID_BLINK) {
        write!(backend, "\x1b[6m")?;
    }
    if modifier.contains(Modifier::REVERSED) {
        write!(backend, "\x1b[7m")?;
    }
    if modifier.contains(Modifier::HIDDEN) {
        write!(backend, "\x1b[8m")?;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        write!(backend, "\x1b[9m")?;
    }
    Ok(())
}

fn write_fg<B>(backend: &mut B, color: Color) -> io::Result<()>
where
    B: Write,
{
    write_color(backend, 38, 39, 30, 90, color)
}

fn write_bg<B>(backend: &mut B, color: Color) -> io::Result<()>
where
    B: Write,
{
    write_color(backend, 48, 49, 40, 100, color)
}

fn write_color<B>(
    backend: &mut B,
    extended_prefix: u8,
    reset: u8,
    base: u8,
    bright_base: u8,
    color: Color,
) -> io::Result<()>
where
    B: Write,
{
    match color {
        Color::Reset => write!(backend, "\x1b[{reset}m"),
        Color::Black => write!(backend, "\x1b[{}m", base),
        Color::Red => write!(backend, "\x1b[{}m", base + 1),
        Color::Green => write!(backend, "\x1b[{}m", base + 2),
        Color::Yellow => write!(backend, "\x1b[{}m", base + 3),
        Color::Blue => write!(backend, "\x1b[{}m", base + 4),
        Color::Magenta => write!(backend, "\x1b[{}m", base + 5),
        Color::Cyan => write!(backend, "\x1b[{}m", base + 6),
        Color::Gray => write!(backend, "\x1b[{}m", base + 7),
        Color::DarkGray => write!(backend, "\x1b[{}m", bright_base),
        Color::LightRed => write!(backend, "\x1b[{}m", bright_base + 1),
        Color::LightGreen => write!(backend, "\x1b[{}m", bright_base + 2),
        Color::LightYellow => write!(backend, "\x1b[{}m", bright_base + 3),
        Color::LightBlue => write!(backend, "\x1b[{}m", bright_base + 4),
        Color::LightMagenta => write!(backend, "\x1b[{}m", bright_base + 5),
        Color::LightCyan => write!(backend, "\x1b[{}m", bright_base + 6),
        Color::White => write!(backend, "\x1b[{}m", bright_base + 7),
        Color::Indexed(index) => write!(backend, "\x1b[{extended_prefix};5;{index}m"),
        Color::Rgb(red, green, blue) => {
            write!(backend, "\x1b[{extended_prefix};2;{red};{green};{blue}m")
        }
    }
}

fn render_frame(
    buffer: &mut Buffer,
    styled: Vec<Line<'static>>,
    title: String,
    footer_text: String,
) {
    let area = buffer.area;
    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let paragraph = Paragraph::new(styled)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(gutter_style()),
        )
        .style(plain_style());
    paragraph.render(body, buffer);
    Paragraph::new(footer_text)
        .style(gutter_style())
        .render(footer, buffer);
}

#[derive(Debug, Clone, Copy)]
enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ScrollHint {
    amount: u16,
    direction: ScrollDirection,
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

fn scrollable_body_area(area: Rect) -> Rect {
    let [body, _footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    Rect {
        x: 0,
        y: body.y.saturating_add(1),
        width: body.width,
        height: body.height.saturating_sub(2),
    }
}
