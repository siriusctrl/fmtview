use std::{io, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::line_index::IndexedTempFile;

#[derive(Debug, Clone, Copy)]
pub enum ViewMode {
    Plain,
    Diff,
}

pub fn run(file: IndexedTempFile, mode: ViewMode) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    let result = run_loop(&mut terminal, &file, mode);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    file: &IndexedTempFile,
    mode: ViewMode,
) -> Result<()> {
    let mut top = 0_usize;
    let mut x = 0_usize;

    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                let [body, footer] =
                    Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
                let visible_height = usize::from(body.height.saturating_sub(2));
                let visible_width = usize::from(body.width.saturating_sub(2));
                let gutter_digits = line_number_digits(file.line_count());
                let gutter_width = gutter_digits + 3;
                let content_width = visible_width.saturating_sub(gutter_width);
                let max_top = file.line_count().saturating_sub(visible_height.max(1));
                top = top.min(max_top);

                let lines = file.read_window(top, visible_height).unwrap_or_else(|error| {
                    vec![format!("failed to read window: {error:#}")]
                });
                let styled = lines
                    .iter()
                    .enumerate()
                    .map(|(index, line)| {
                        styled_line(line, top + index + 1, gutter_digits, x, content_width, mode)
                    })
                    .collect::<Vec<_>>();

                let current = if file.line_count() == 0 { 0 } else { top + 1 };
                let bottom = top.saturating_add(visible_height).min(file.line_count());
                let title = format!(
                    " {} | {} lines | {}-{} | {:>3}% | x:{} ",
                    file.label(),
                    file.line_count(),
                    current,
                    bottom,
                    progress_percent(bottom, file.line_count()),
                    x
                );
                let paragraph = Paragraph::new(styled).block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                frame.render_widget(paragraph, body);

                let footer_text =
                    " q/Esc quit | j/k/↑/↓ scroll | PgUp/PgDn page | g/G top/end | h/l/←/→ horizontal | redirect with > to write ";
                frame.render_widget(
                    Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray)),
                    footer,
                );
            })
            .context("failed to draw terminal frame")?;

        if !event::poll(Duration::from_millis(250)).context("failed to poll terminal event")? {
            continue;
        }

        let Event::Key(key) = event::read().context("failed to read terminal event")? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        let page = terminal
            .size()
            .map(|size| usize::from(size.height.saturating_sub(4)).max(1))
            .unwrap_or(20);

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Down | KeyCode::Char('j') => {
                top = top
                    .saturating_add(1)
                    .min(file.line_count().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                top = top.saturating_sub(1);
            }
            KeyCode::PageDown => {
                top = top
                    .saturating_add(page)
                    .min(file.line_count().saturating_sub(1));
            }
            KeyCode::PageUp => {
                top = top.saturating_sub(page);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                top = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                top = file.line_count().saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                x = x.saturating_add(4);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                x = x.saturating_sub(4);
            }
            _ => {}
        }
    }

    Ok(())
}

fn styled_line(
    line: &str,
    line_number: usize,
    gutter_digits: usize,
    x: usize,
    width: usize,
    mode: ViewMode,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled(
        format!("{line_number:>gutter_digits$} │ "),
        gutter_style(),
    ));

    let clipped = clip(line, x, width);
    spans.extend(highlight_content(&clipped, mode));
    Line::from(spans)
}

fn clip(line: &str, x: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    line.chars().skip(x).take(width).collect()
}

fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len()
}

fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    match mode {
        ViewMode::Plain => highlight_structured(line),
        ViewMode::Diff if line.starts_with("@@") => vec![Span::styled(
            line.to_owned(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )],
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            vec![Span::styled(
                line.to_owned(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]
        }
        ViewMode::Diff if line.starts_with('+') => highlight_diff_payload(line, Color::Green),
        ViewMode::Diff if line.starts_with('-') => highlight_diff_payload(line, Color::Red),
        ViewMode::Diff => highlight_structured(line),
    }
}

fn highlight_diff_payload(line: &str, color: Color) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        line[..1].to_owned(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    spans.extend(highlight_structured(&line[1..]));
    spans
}

fn highlight_structured(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        highlight_xml(line)
    } else {
        highlight_json_like(line)
    }
}

fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;

    while index < line.len() {
        let rest = &line[index..];
        let ch = rest.chars().next().expect("index should point to a char");

        if ch.is_whitespace() {
            let end = take_while(line, index, char::is_whitespace);
            push_span(&mut spans, &line[index..end], Style::default());
            index = end;
            continue;
        }

        if ch == '"' {
            let end = json_string_end(line, index);
            let style = if json_string_is_key(line, end) {
                key_style()
            } else {
                string_style()
            };
            push_span(&mut spans, &line[index..end], style);
            index = end;
            continue;
        }

        if ch == '-' || ch.is_ascii_digit() {
            let end = take_while(line, index, is_json_number_char);
            push_span(&mut spans, &line[index..end], number_style());
            index = end;
            continue;
        }

        if let Some((word, style)) = json_keyword(rest) {
            push_span(&mut spans, word, style);
            index += word.len();
            continue;
        }

        if "{}[]:,".contains(ch) {
            push_span(
                &mut spans,
                &line[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        push_span(
            &mut spans,
            &line[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

fn highlight_xml(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;

    while index < line.len() {
        let rest = &line[index..];
        if rest.starts_with('<') {
            let end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(line.len());
            spans.extend(highlight_xml_tag(&line[index..end]));
            index = end;
        } else {
            let end = rest
                .find('<')
                .map(|position| index + position)
                .unwrap_or(line.len());
            push_span(&mut spans, &line[index..end], Style::default());
            index = end;
        }
    }

    spans
}

fn highlight_xml_tag(tag: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut saw_name = false;

    while index < tag.len() {
        let rest = &tag[index..];
        let ch = rest.chars().next().expect("index should point to a char");

        if ch.is_whitespace() {
            let end = take_while(tag, index, char::is_whitespace);
            push_span(&mut spans, &tag[index..end], Style::default());
            index = end;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let end = quoted_end(tag, index, ch);
            push_span(&mut spans, &tag[index..end], string_style());
            index = end;
            continue;
        }

        if "<>/=?!".contains(ch) {
            push_span(
                &mut spans,
                &tag[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        if is_xml_name_char(ch) {
            let end = take_while(tag, index, is_xml_name_char);
            let style = if saw_name { attr_style() } else { key_style() };
            saw_name = true;
            push_span(&mut spans, &tag[index..end], style);
            index = end;
            continue;
        }

        push_span(
            &mut spans,
            &tag[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

fn json_string_end(line: &str, start: usize) -> usize {
    let mut escaped = false;
    for (offset, ch) in line[start + 1..].char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    line.len()
}

fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..].trim_start().starts_with(':')
}

fn is_json_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')
}

fn json_keyword(rest: &str) -> Option<(&str, Style)> {
    for keyword in ["true", "false"] {
        if rest.starts_with(keyword) && keyword_boundary(rest, keyword.len()) {
            return Some((keyword, bool_style()));
        }
    }

    if rest.starts_with("null") && keyword_boundary(rest, "null".len()) {
        Some(("null", null_style()))
    } else {
        None
    }
}

fn keyword_boundary(rest: &str, end: usize) -> bool {
    rest[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}

fn push_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(text.to_owned(), style));
    }
}

fn gutter_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn punctuation_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn key_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn attr_style() -> Style {
    Style::default().fg(Color::Yellow)
}

fn string_style() -> Style {
    Style::default().fg(Color::Green)
}

fn number_style() -> Style {
    Style::default().fg(Color::Magenta)
}

fn bool_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn null_style() -> Style {
    Style::default().fg(Color::Blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_by_character_not_byte() {
        assert_eq!(clip("a路径b", 1, 2), "路径");
    }

    #[test]
    fn styled_line_keeps_a_gutter() {
        let line = styled_line(r#"  "name": "fmtview","#, 12, 3, 0, 80, ViewMode::Plain);
        assert_eq!(span_text(&line.spans), r#" 12 │   "name": "fmtview","#);
    }

    #[test]
    fn json_highlight_preserves_visible_text() {
        let spans = highlight_json_like(r#"  "ok": true, "n": 42, "none": null"#);
        assert_eq!(span_text(&spans), r#"  "ok": true, "n": 42, "none": null"#);
    }

    #[test]
    fn xml_highlight_preserves_visible_text() {
        let spans = highlight_xml(r#"<root id="1"><child>value</child></root>"#);
        assert_eq!(
            span_text(&spans),
            r#"<root id="1"><child>value</child></root>"#
        );
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }
}
