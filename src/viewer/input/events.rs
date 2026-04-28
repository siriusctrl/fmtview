use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use super::super::{
    EVENT_DRAIN_BUDGET, EVENT_DRAIN_LIMIT, MOUSE_HORIZONTAL_COLUMNS, MOUSE_SCROLL_LINES,
};
use super::{
    jump::{handle_jump_input_key, push_jump_digit},
    keys::{accepts_jump_digit, plain_key},
    scroll::{
        page_down, page_up, reset_top_row_offset, scroll_down, scroll_down_by, scroll_up,
        scroll_up_by, scroll_x_by, set_file_end, set_top,
    },
    search::{
        SearchDirection, cancel_search_task, clear_search_message, handle_search_input_key,
        start_repeat_search, start_search_prompt,
    },
    state::{EventAction, ViewState},
};

pub(in crate::viewer) fn drain_events(
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> Result<EventAction> {
    let started = Instant::now();
    let mut action = EventAction::default();
    let mut processed = 0;

    loop {
        let event = event::read().context("failed to read terminal event")?;
        let next = handle_event(event, state, line_count, page);
        let needs_layout = next.dirty && state.wrap_bounds_stale;
        action.merge(next);
        processed += 1;

        if action.quit
            || needs_layout
            || processed >= EVENT_DRAIN_LIMIT
            || started.elapsed() >= EVENT_DRAIN_BUDGET
            || !event::poll(Duration::ZERO).context("failed to poll terminal event")?
        {
            break;
        }
    }

    Ok(action)
}

pub(in crate::viewer) fn handle_event(
    event: Event,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> EventAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => EventAction::default(),
        Event::Key(key) => handle_key_event(key.code, key.modifiers, state, line_count, page),
        Event::Mouse(mouse) if !state.has_active_prompt() => {
            handle_mouse_event(mouse.kind, mouse.modifiers, state, line_count)
        }
        Event::Mouse(_) => EventAction::default(),
        Event::Resize(_, _) => EventAction {
            dirty: true,
            quit: false,
        },
        _ => EventAction::default(),
    }
}

pub(in crate::viewer) fn handle_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> EventAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return EventAction {
            dirty: false,
            quit: true,
        };
    }

    if state.search_active {
        return EventAction {
            dirty: handle_search_input_key(code, modifiers, state, line_count),
            quit: false,
        };
    }

    if !state.jump_buffer.is_empty() {
        return EventAction {
            dirty: handle_jump_input_key(code, modifiers, state, line_count),
            quit: false,
        };
    }

    let dirty = match code {
        KeyCode::Char(ch) if accepts_jump_digit(ch, modifiers) => {
            push_jump_digit(state, ch);
            true
        }
        KeyCode::Char('/') if plain_key(modifiers) => start_search_prompt(state),
        KeyCode::Char('n') if plain_key(modifiers) => {
            start_repeat_search(state, line_count, SearchDirection::Forward)
        }
        KeyCode::Char('N') if plain_key(modifiers) => {
            start_repeat_search(state, line_count, SearchDirection::Backward)
        }
        KeyCode::Enter => false,
        KeyCode::Esc if state.search_task.is_some() => cancel_search_task(state),
        KeyCode::Esc if state.search_message.is_some() => clear_search_message(state),
        KeyCode::Char('q') | KeyCode::Esc => {
            return EventAction {
                dirty: false,
                quit: true,
            };
        }
        KeyCode::Char('w') => {
            state.wrap = !state.wrap;
            reset_top_row_offset(state);
            true
        }
        KeyCode::Down | KeyCode::Char('j') => scroll_down(state, line_count),
        KeyCode::Up | KeyCode::Char('k') => scroll_up(state, line_count),
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
            page_down(state, line_count, page)
        }
        KeyCode::PageUp | KeyCode::Char('b') => page_up(state, line_count, page),
        KeyCode::Home | KeyCode::Char('g') => set_top(state, 0),
        KeyCode::End | KeyCode::Char('G') => set_file_end(state, line_count),
        KeyCode::Right | KeyCode::Char('l') if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    EventAction { dirty, quit: false }
}

pub(in crate::viewer) fn handle_mouse_event(
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
) -> EventAction {
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        MouseEventKind::ScrollDown => scroll_down_by(state, line_count, MOUSE_SCROLL_LINES),
        MouseEventKind::ScrollUp => scroll_up_by(state, line_count, MOUSE_SCROLL_LINES),
        MouseEventKind::ScrollRight if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollLeft if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    EventAction { dirty, quit: false }
}
