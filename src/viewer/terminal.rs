use std::io::{self, Write};

use ratatui::{
    backend::Backend,
    buffer::{Buffer, Cell},
    layout::{Constraint, Layout, Rect, Size},
    style::{Color, Modifier},
    text::Line,
};

use super::{
    palette::{gutter_style, plain_style},
    render::ViewPosition,
};

pub(super) struct TerminalFrame {
    pub(super) area: Rect,
    pub(super) styled: Vec<Line<'static>>,
    pub(super) sticky: Vec<Line<'static>>,
    pub(super) title: String,
    pub(super) footer_text: String,
    pub(super) position: ViewPosition,
    pub(super) scroll_hint: Option<ScrollHint>,
}

pub(super) struct ViewerTerminal<B> {
    backend: B,
    previous: Option<Buffer>,
    scratch: Option<Buffer>,
    previous_position: Option<ViewPosition>,
    previous_sticky_rows: usize,
    output: Vec<u8>,
}

impl<B> ViewerTerminal<B>
where
    B: Backend<Error = io::Error> + Write,
{
    pub(super) fn new(backend: B) -> Self {
        Self {
            backend,
            previous: None,
            scratch: None,
            previous_position: None,
            previous_sticky_rows: 0,
            output: Vec::with_capacity(16 * 1024),
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

    pub(super) fn draw(&mut self, frame: TerminalFrame) -> io::Result<()> {
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
            frame.title,
            frame.footer_text,
        );
        match self.previous.take() {
            Some(previous)
                if previous.area == current.area && self.previous_sticky_rows == sticky_rows =>
            {
                draw_diff(
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
                self.scratch = previous;
            }
        }
        self.backend.hide_cursor()?;
        Backend::flush(&mut self.backend)?;
        self.previous = Some(current);
        self.previous_position = Some(frame.position);
        self.previous_sticky_rows = sticky_rows;
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

    pub(super) fn previous_position(&self) -> Option<ViewPosition> {
        self.previous_position
    }
}

fn draw_diff<B>(
    backend: &mut B,
    previous: &Buffer,
    current: &Buffer,
    scroll_hint: Option<ScrollHint>,
    sticky_rows: usize,
    output: &mut Vec<u8>,
) -> io::Result<()>
where
    B: Backend<Error = io::Error> + Write,
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

#[cfg(test)]
pub(super) fn draw_cells<'a, B, I>(backend: &mut B, content: I) -> io::Result<()>
where
    B: Write,
    I: IntoIterator<Item = (u16, u16, &'a Cell)>,
{
    let mut output = Vec::with_capacity(16 * 1024);
    draw_cells_with_buffer(backend, content, &mut output)
}

fn draw_cells_with_buffer<'a, B, I>(
    backend: &mut B,
    content: I,
    output: &mut Vec<u8>,
) -> io::Result<()>
where
    B: Write,
    I: IntoIterator<Item = (u16, u16, &'a Cell)>,
{
    output.clear();
    let mut fg = Color::Reset;
    let mut bg = Color::Reset;
    let mut modifier = Modifier::empty();
    let mut last_pos: Option<(u16, u16)> = None;

    for (x, y, cell) in content {
        if !matches!(last_pos, Some((last_x, last_y)) if x == last_x.saturating_add(1) && y == last_y)
        {
            push_cursor_move(output, x, y);
        }
        last_pos = Some((x, y));

        if cell.modifier != modifier {
            output.extend_from_slice(b"\x1b[0m");
            fg = Color::Reset;
            bg = Color::Reset;
            write_modifier(output, cell.modifier);
            modifier = cell.modifier;
        }
        if cell.fg != fg {
            write_fg(output, cell.fg);
            fg = cell.fg;
        }
        if cell.bg != bg {
            write_bg(output, cell.bg);
            bg = cell.bg;
        }
        output.extend_from_slice(cell.symbol().as_bytes());
    }

    output.extend_from_slice(b"\x1b[0m");
    backend.write_all(output)
}

fn push_cursor_move(output: &mut Vec<u8>, x: u16, y: u16) {
    output.extend_from_slice(b"\x1b[");
    push_u16(output, y.saturating_add(1));
    output.push(b';');
    push_u16(output, x.saturating_add(1));
    output.push(b'H');
}

fn write_modifier(output: &mut Vec<u8>, modifier: Modifier) {
    if modifier.contains(Modifier::BOLD) {
        output.extend_from_slice(b"\x1b[1m");
    }
    if modifier.contains(Modifier::DIM) {
        output.extend_from_slice(b"\x1b[2m");
    }
    if modifier.contains(Modifier::ITALIC) {
        output.extend_from_slice(b"\x1b[3m");
    }
    if modifier.contains(Modifier::UNDERLINED) {
        output.extend_from_slice(b"\x1b[4m");
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        output.extend_from_slice(b"\x1b[5m");
    }
    if modifier.contains(Modifier::RAPID_BLINK) {
        output.extend_from_slice(b"\x1b[6m");
    }
    if modifier.contains(Modifier::REVERSED) {
        output.extend_from_slice(b"\x1b[7m");
    }
    if modifier.contains(Modifier::HIDDEN) {
        output.extend_from_slice(b"\x1b[8m");
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        output.extend_from_slice(b"\x1b[9m");
    }
}

fn write_fg(output: &mut Vec<u8>, color: Color) {
    write_color(output, 38, 39, 30, 90, color)
}

fn write_bg(output: &mut Vec<u8>, color: Color) {
    write_color(output, 48, 49, 40, 100, color)
}

fn write_color(
    output: &mut Vec<u8>,
    extended_prefix: u8,
    reset: u8,
    base: u8,
    bright_base: u8,
    color: Color,
) {
    match color {
        Color::Reset => push_sgr(output, reset),
        Color::Black => push_sgr(output, base),
        Color::Red => push_sgr(output, base + 1),
        Color::Green => push_sgr(output, base + 2),
        Color::Yellow => push_sgr(output, base + 3),
        Color::Blue => push_sgr(output, base + 4),
        Color::Magenta => push_sgr(output, base + 5),
        Color::Cyan => push_sgr(output, base + 6),
        Color::Gray => push_sgr(output, base + 7),
        Color::DarkGray => push_sgr(output, bright_base),
        Color::LightRed => push_sgr(output, bright_base + 1),
        Color::LightGreen => push_sgr(output, bright_base + 2),
        Color::LightYellow => push_sgr(output, bright_base + 3),
        Color::LightBlue => push_sgr(output, bright_base + 4),
        Color::LightMagenta => push_sgr(output, bright_base + 5),
        Color::LightCyan => push_sgr(output, bright_base + 6),
        Color::White => push_sgr(output, bright_base + 7),
        Color::Indexed(index) => {
            output.extend_from_slice(b"\x1b[");
            push_u8(output, extended_prefix);
            output.extend_from_slice(b";5;");
            push_u8(output, index);
            output.push(b'm');
        }
        Color::Rgb(red, green, blue) => {
            output.extend_from_slice(b"\x1b[");
            push_u8(output, extended_prefix);
            output.extend_from_slice(b";2;");
            push_u8(output, red);
            output.push(b';');
            push_u8(output, green);
            output.push(b';');
            push_u8(output, blue);
            output.push(b'm');
        }
    }
}

fn push_sgr(output: &mut Vec<u8>, code: u8) {
    output.extend_from_slice(b"\x1b[");
    push_u8(output, code);
    output.push(b'm');
}

fn push_u8(output: &mut Vec<u8>, value: u8) {
    push_u16(output, u16::from(value));
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    let mut buf = [0_u8; 5];
    let mut index = buf.len();
    let mut value = value;
    loop {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    output.extend_from_slice(&buf[index..]);
}

fn render_frame(
    buffer: &mut Buffer,
    styled: Vec<Line<'static>>,
    sticky: Vec<Line<'static>>,
    title: String,
    footer_text: String,
) {
    let area = buffer.area;
    if area.width == 0 || area.height == 0 {
        return;
    }

    let footer_y = area.y.saturating_add(area.height - 1);
    let body = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    if body.height > 0 {
        render_body(buffer, body, styled, sticky, &title);
    }

    buffer.set_stringn(
        area.x,
        footer_y,
        footer_text,
        usize::from(area.width),
        gutter_style(),
    );
}

fn render_body(
    buffer: &mut Buffer,
    area: Rect,
    styled: Vec<Line<'static>>,
    sticky: Vec<Line<'static>>,
    title: &str,
) {
    if area.width < 2 || area.height < 2 {
        for (row, line) in styled
            .into_iter()
            .take(usize::from(area.height))
            .enumerate()
        {
            set_line_fast(
                buffer,
                area.x,
                area.y.saturating_add(row as u16),
                &line,
                area.width,
            );
        }
        return;
    }

    render_border(buffer, area, title);
    let content_width = area.width.saturating_sub(2);
    let content_height = area.height.saturating_sub(2);
    let sticky_rows = sticky.len().min(usize::from(content_height));
    for (row, line) in sticky.into_iter().take(sticky_rows).enumerate() {
        set_line_fast(
            buffer,
            area.x.saturating_add(1),
            area.y.saturating_add(1).saturating_add(row as u16),
            &line,
            content_width,
        );
    }
    for (row, line) in styled
        .into_iter()
        .take(usize::from(content_height).saturating_sub(sticky_rows))
        .enumerate()
    {
        set_line_fast(
            buffer,
            area.x.saturating_add(1),
            area.y
                .saturating_add(1)
                .saturating_add(sticky_rows as u16)
                .saturating_add(row as u16),
            &line,
            content_width,
        );
    }
}

fn set_line_fast(buffer: &mut Buffer, mut x: u16, y: u16, line: &Line<'_>, max_width: u16) {
    let mut remaining = max_width;
    for span in &line.spans {
        if remaining == 0 {
            break;
        }

        let style = plain_style().patch(line.style).patch(span.style);
        let text = span.content.as_ref();
        if text.is_ascii() {
            for byte in text.bytes().take(usize::from(remaining)) {
                if byte.is_ascii_control() {
                    continue;
                }
                buffer[(x, y)].set_char(char::from(byte)).set_style(style);
                x = x.saturating_add(1);
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    break;
                }
            }
        } else {
            let next = buffer.set_stringn(x, y, text, usize::from(remaining), style);
            let written = next.0.saturating_sub(x);
            x = next.0;
            remaining = remaining.saturating_sub(written);
        }
    }
}

fn render_border(buffer: &mut Buffer, area: Rect, title: &str) {
    let style = gutter_style();
    let left = area.x;
    let right = area.x.saturating_add(area.width - 1);
    let top = area.y;
    let bottom = area.y.saturating_add(area.height - 1);

    buffer[(left, top)].set_symbol("┌").set_style(style);
    buffer[(right, top)].set_symbol("┐").set_style(style);
    buffer[(left, bottom)].set_symbol("└").set_style(style);
    buffer[(right, bottom)].set_symbol("┘").set_style(style);

    for x in left.saturating_add(1)..right {
        buffer[(x, top)].set_symbol("─").set_style(style);
        buffer[(x, bottom)].set_symbol("─").set_style(style);
    }
    for y in top.saturating_add(1)..bottom {
        buffer[(left, y)].set_symbol("│").set_style(style);
        buffer[(right, y)].set_symbol("│").set_style(style);
    }

    let title_width = area.width.saturating_sub(2);
    if title_width > 0 {
        buffer.set_stringn(
            left.saturating_add(1),
            top,
            title,
            usize::from(title_width),
            style,
        );
    }
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

impl ScrollHint {
    pub(super) fn up(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Up,
        }
    }

    pub(super) fn down(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Down,
        }
    }
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
