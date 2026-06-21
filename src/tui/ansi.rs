use std::io::{self, Write};

use ratatui::{
    buffer::Cell,
    style::{Color, Modifier},
};

#[cfg(test)]
pub(crate) fn draw_cells<'a, B, I>(backend: &mut B, content: I) -> io::Result<()>
where
    B: Write,
    I: IntoIterator<Item = (u16, u16, &'a Cell)>,
{
    let mut output = Vec::with_capacity(16 * 1024);
    draw_cells_with_buffer(backend, content, &mut output)
}

pub(super) fn draw_cells_with_buffer<'a, B, I>(
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
