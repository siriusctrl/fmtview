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
    style::{Color, Style},
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
                let max_top = file.line_count().saturating_sub(visible_height.max(1));
                top = top.min(max_top);

                let lines = file.read_window(top, visible_height).unwrap_or_else(|error| {
                    vec![format!("failed to read window: {error:#}")]
                });
                let styled = lines
                    .iter()
                    .map(|line| styled_line(line, x, visible_width, mode))
                    .collect::<Vec<_>>();

                let title = format!(
                    " {} | lines: {} | at: {} | x: {} ",
                    file.label(),
                    file.line_count(),
                    if file.line_count() == 0 { 0 } else { top + 1 },
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
                    " q/Esc quit | j/k or ↑/↓ scroll | PgUp/PgDn | g/G top/end | h/l or ←/→ horizontal ";
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

fn styled_line<'a>(line: &'a str, x: usize, width: usize, mode: ViewMode) -> Line<'a> {
    let clipped = clip(line, x, width);
    let style = match mode {
        ViewMode::Plain => Style::default(),
        ViewMode::Diff if line.starts_with('+') && !line.starts_with("+++") => {
            Style::default().fg(Color::Green)
        }
        ViewMode::Diff if line.starts_with('-') && !line.starts_with("---") => {
            Style::default().fg(Color::Red)
        }
        ViewMode::Diff if line.starts_with("@@") => Style::default().fg(Color::Cyan),
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            Style::default().fg(Color::Yellow)
        }
        ViewMode::Diff => Style::default(),
    };
    Line::from(Span::styled(clipped, style))
}

fn clip(line: &str, x: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    line.chars().skip(x).take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_by_character_not_byte() {
        assert_eq!(clip("a路径b", 1, 2), "路径");
    }
}
