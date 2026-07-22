use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
        KeyModifiers as CrosstermModifiers, MouseEventKind as CrosstermMouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use fmtview_core::{
    DiffView, DiffViewer, FileViewer, FormatKind, InputEvent, KeyCode, KeyModifiers,
    MouseEventKind, ViewFile, ViewerAction,
};
use ratatui::backend::CrosstermBackend;

use crate::tui::screen::ViewerTerminal;

const EVENT_DRAIN_BUDGET: Duration = Duration::from_millis(8);
const EVENT_DRAIN_LIMIT: usize = 512;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) fn run(file: Box<dyn ViewFile>, mode: FormatKind, notice: Option<String>) -> Result<()> {
    run_terminal(|terminal| run_file_loop(terminal, FileViewer::new(file, mode, notice)))
}

pub(crate) fn run_diff(view: DiffView) -> Result<()> {
    run_terminal(|terminal| {
        let size = terminal.size().context("failed to read terminal size")?;
        run_diff_loop(terminal, DiffViewer::new(view, size)?)
    })
}

fn run_file_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    mut viewer: FileViewer,
) -> Result<()> {
    let mut dirty = true;
    loop {
        dirty |= viewer.advance(Instant::now())?;
        if dirty {
            let size = terminal.size().context("failed to read terminal size")?;
            let frame = viewer.render(size, terminal.previous_position())?;
            terminal
                .draw(frame)
                .context("failed to draw terminal frame")?;
            dirty = false;
            if !event::poll(Duration::ZERO)
                .context("failed to poll terminal event before prewarming")?
            {
                viewer.prewarm();
            }
        }

        let poll_interval = if viewer.needs_immediate_advance() {
            Duration::ZERO
        } else {
            EVENT_POLL_INTERVAL
        };
        if !event::poll(poll_interval).context("failed to poll terminal event")? {
            dirty |= viewer.preload()?;
            continue;
        }

        let size = terminal.size().context("failed to read terminal size")?;
        let action = drain_file_events(&mut viewer, FileViewer::page_for_size(size))?;
        if action.quit {
            break;
        }
        if let Some(enabled) = action.mouse_capture {
            apply_mouse_capture(terminal, enabled)?;
        }
        dirty |= action.dirty;
    }
    Ok(())
}

fn run_diff_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    mut viewer: DiffViewer,
) -> Result<()> {
    let mut dirty = true;
    loop {
        if dirty {
            let size = terminal.size().context("failed to read terminal size")?;
            let frame = viewer.render(size, terminal.previous_position());
            terminal
                .draw(frame)
                .context("failed to draw terminal frame")?;
            dirty = false;
        }

        if !event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal event")? {
            dirty |= viewer.preload()?;
            continue;
        }

        let size = terminal.size().context("failed to read terminal size")?;
        let action = drain_diff_events(&mut viewer, size)?;
        if action.quit {
            break;
        }
        dirty |= action.dirty;
        if !dirty {
            dirty |= viewer.preload()?;
        }
    }
    Ok(())
}

fn drain_file_events(viewer: &mut FileViewer, page: usize) -> Result<ViewerAction> {
    drain_events(|event| {
        let action = viewer.handle_event(event, page);
        let stop = action.dirty && viewer.needs_layout();
        (action, stop)
    })
}

fn drain_diff_events(viewer: &mut DiffViewer, size: ratatui::layout::Size) -> Result<ViewerAction> {
    drain_events(|event| (viewer.handle_event(event, size), false))
}

fn drain_events(
    mut handle: impl FnMut(InputEvent) -> (ViewerAction, bool),
) -> Result<ViewerAction> {
    let started = Instant::now();
    let mut action = ViewerAction::default();
    let mut processed = 0;
    loop {
        let event = event::read().context("failed to read terminal event")?;
        let (next, stop) = handle(adapt_event(event));
        action.merge(next);
        processed += 1;
        if action.quit
            || stop
            || processed >= EVENT_DRAIN_LIMIT
            || started.elapsed() >= EVENT_DRAIN_BUDGET
            || !event::poll(Duration::ZERO).context("failed to poll terminal event")?
        {
            break;
        }
    }
    Ok(action)
}

fn adapt_event(event: Event) -> InputEvent {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => InputEvent::Ignore,
        Event::Key(key) => adapt_key_code(key.code)
            .map(|code| InputEvent::Key {
                code,
                modifiers: adapt_modifiers(key.modifiers),
            })
            .unwrap_or(InputEvent::Ignore),
        Event::Mouse(mouse) => InputEvent::Mouse {
            kind: match mouse.kind {
                CrosstermMouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
                CrosstermMouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
                CrosstermMouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
                CrosstermMouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
                _ => MouseEventKind::Other,
            },
            modifiers: adapt_modifiers(mouse.modifiers),
        },
        Event::Resize(_, _) => InputEvent::Resize,
        _ => InputEvent::Ignore,
    }
}

fn adapt_key_code(code: crossterm::event::KeyCode) -> Option<KeyCode> {
    Some(match code {
        crossterm::event::KeyCode::Char(ch) => KeyCode::Char(ch),
        crossterm::event::KeyCode::Enter => KeyCode::Enter,
        crossterm::event::KeyCode::Esc => KeyCode::Esc,
        crossterm::event::KeyCode::Backspace => KeyCode::Backspace,
        crossterm::event::KeyCode::Up => KeyCode::Up,
        crossterm::event::KeyCode::Down => KeyCode::Down,
        crossterm::event::KeyCode::Left => KeyCode::Left,
        crossterm::event::KeyCode::Right => KeyCode::Right,
        crossterm::event::KeyCode::PageUp => KeyCode::PageUp,
        crossterm::event::KeyCode::PageDown => KeyCode::PageDown,
        crossterm::event::KeyCode::Home => KeyCode::Home,
        crossterm::event::KeyCode::End => KeyCode::End,
        _ => return None,
    })
}

fn adapt_modifiers(modifiers: CrosstermModifiers) -> KeyModifiers {
    let mut adapted = KeyModifiers::NONE;
    if modifiers.contains(CrosstermModifiers::SHIFT) {
        adapted = adapted.union(KeyModifiers::SHIFT);
    }
    if modifiers.contains(CrosstermModifiers::CONTROL) {
        adapted = adapted.union(KeyModifiers::CONTROL);
    }
    if modifiers.contains(CrosstermModifiers::ALT) {
        adapted = adapted.union(KeyModifiers::ALT);
    }
    if modifiers.contains(CrosstermModifiers::SUPER) {
        adapted = adapted.union(KeyModifiers::SUPER);
    }
    if modifiers.contains(CrosstermModifiers::HYPER) {
        adapted = adapted.union(KeyModifiers::HYPER);
    }
    if modifiers.contains(CrosstermModifiers::META) {
        adapted = adapted.union(KeyModifiers::META);
    }
    adapted
}

fn apply_mouse_capture(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    enabled: bool,
) -> Result<()> {
    if enabled {
        execute!(terminal.backend_mut(), EnableMouseCapture)
            .context("failed to enable mouse capture")?;
    } else {
        execute!(terminal.backend_mut(), DisableMouseCapture)
            .context("failed to disable mouse capture")?;
    }
    Ok(())
}

fn run_terminal<F>(run_loop: F) -> Result<()>
where
    F: FnOnce(&mut ViewerTerminal<CrosstermBackend<io::Stdout>>) -> Result<()>,
{
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut cleanup = TerminalCleanup::active();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ViewerTerminal::new(backend);
    let result = run_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )
    .ok();
    cleanup.disarm();
    terminal.show_cursor().ok();

    result
}

struct TerminalCleanup {
    active: bool,
}

impl TerminalCleanup {
    fn active() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        disable_raw_mode().ok();
        let mut stdout = io::stdout();
        execute!(stdout, DisableMouseCapture, LeaveAlternateScreen).ok();
        stdout.flush().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_adapter_preserves_all_key_modifiers() {
        let modifiers = CrosstermModifiers::SHIFT
            | CrosstermModifiers::CONTROL
            | CrosstermModifiers::ALT
            | CrosstermModifiers::SUPER
            | CrosstermModifiers::HYPER
            | CrosstermModifiers::META;

        let adapted = adapt_modifiers(modifiers);

        for expected in [
            KeyModifiers::SHIFT,
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SUPER,
            KeyModifiers::HYPER,
            KeyModifiers::META,
        ] {
            assert!(adapted.contains(expected));
        }
    }
}
