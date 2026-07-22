use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Line};

use super::palette::{gutter_style, plain_style};

pub(crate) fn render_frame(
    buffer: &mut Buffer,
    styled: Vec<Line<'static>>,
    sticky: Vec<Line<'static>>,
    selection_mode: bool,
    title: String,
    footer_text: String,
    footer_style: Style,
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
        if selection_mode {
            render_selectable_body(buffer, body, styled, sticky);
        } else {
            render_body(buffer, body, styled, sticky, &title);
        }
    }

    buffer.set_stringn(
        area.x,
        footer_y,
        footer_text,
        usize::from(area.width),
        footer_style,
    );
}

fn render_selectable_body(
    buffer: &mut Buffer,
    area: Rect,
    styled: Vec<Line<'static>>,
    sticky: Vec<Line<'static>>,
) {
    let sticky_rows = sticky.len().min(usize::from(area.height));
    for (row, line) in sticky.into_iter().take(sticky_rows).enumerate() {
        set_line_fast(
            buffer,
            area.x,
            area.y.saturating_add(row as u16),
            &line,
            area.width,
        );
    }
    for (row, line) in styled
        .into_iter()
        .take(usize::from(area.height).saturating_sub(sticky_rows))
        .enumerate()
    {
        set_line_fast(
            buffer,
            area.x,
            area.y
                .saturating_add(sticky_rows as u16)
                .saturating_add(row as u16),
            &line,
            area.width,
        );
    }
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
