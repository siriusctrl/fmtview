use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::diff::DiffModel;

use super::super::{
    EVENT_DRAIN_BUDGET, EVENT_DRAIN_LIMIT, MOUSE_HORIZONTAL_COLUMNS, MOUSE_SCROLL_LINES,
};
use super::DiffViewState;

mod navigation;

#[cfg(test)]
pub(super) use navigation::change_block_starts;
pub(super) use navigation::{DiffJump, clamp_top, diff_scroll_hint, jump_change, scroll_by};

pub(super) fn drain_events(
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
    width: usize,
) -> Result<DiffEventAction> {
    let started = Instant::now();
    let mut action = DiffEventAction::default();
    let mut processed = 0;

    loop {
        let event = event::read().context("failed to read terminal event")?;
        let next = handle_event(event, model, state, page, visible_height, width);
        action.merge(next);
        processed += 1;

        if action.quit
            || processed >= EVENT_DRAIN_LIMIT
            || started.elapsed() >= EVENT_DRAIN_BUDGET
            || !event::poll(Duration::ZERO).context("failed to poll terminal event")?
        {
            break;
        }
    }

    Ok(action)
}

fn handle_event(
    event: Event,
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
    width: usize,
) -> DiffEventAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => DiffEventAction::default(),
        Event::Key(key) => handle_key_event(
            key.code,
            key.modifiers,
            model,
            state,
            page,
            visible_height,
            width,
        ),
        Event::Mouse(mouse) => handle_mouse_event(
            mouse.kind,
            mouse.modifiers,
            model,
            state,
            visible_height,
            width,
        ),
        Event::Resize(_, _) => DiffEventAction {
            dirty: true,
            quit: false,
        },
        _ => DiffEventAction::default(),
    }
}

fn handle_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
    width: usize,
) -> DiffEventAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return DiffEventAction {
            dirty: false,
            quit: true,
        };
    }

    let line_count = model.row_count(state.layout);
    let dirty = match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            return DiffEventAction {
                dirty: false,
                quit: true,
            };
        }
        KeyCode::Char('s') if plain_key(modifiers) => {
            state.layout = state.layout.toggle();
            state.change_cursor = None;
            clamp_top(state, model, width);
            true
        }
        KeyCode::Char('w') if plain_key(modifiers) => {
            state.wrap = !state.wrap;
            state.x = 0;
            state.top_row_offset = 0;
            state.change_cursor = None;
            clamp_top(state, model, width);
            true
        }
        KeyCode::Char(']') if plain_key(modifiers) => {
            jump_change(model, state, DiffJump::Next, page)
        }
        KeyCode::Char('[') if plain_key(modifiers) => {
            jump_change(model, state, DiffJump::Previous, page)
        }
        KeyCode::Down | KeyCode::Char('j') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, model, visible_height, width, 1)
        }
        KeyCode::Up | KeyCode::Char('k') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, model, visible_height, width, -1)
        }
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, model, visible_height, width, page as isize)
        }
        KeyCode::PageUp | KeyCode::Char('b') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, model, visible_height, width, -(page as isize))
        }
        KeyCode::Home | KeyCode::Char('g') if plain_key(modifiers) => {
            state.change_cursor = None;
            navigation::set_top(state, 0, 0, line_count)
        }
        KeyCode::End | KeyCode::Char('G') if plain_key(modifiers) => {
            state.change_cursor = None;
            navigation::set_tail_top(state, model, visible_height, width)
        }
        KeyCode::Right | KeyCode::Char('l') if plain_key(modifiers) && !state.wrap => {
            navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if plain_key(modifiers) && !state.wrap => {
            navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    DiffEventAction { dirty, quit: false }
}

fn handle_mouse_event(
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    model: &DiffModel,
    state: &mut DiffViewState,
    visible_height: usize,
    width: usize,
) -> DiffEventAction {
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        MouseEventKind::ScrollDown => {
            state.change_cursor = None;
            scroll_by(
                state,
                model,
                visible_height,
                width,
                MOUSE_SCROLL_LINES as isize,
            )
        }
        MouseEventKind::ScrollUp => {
            state.change_cursor = None;
            scroll_by(
                state,
                model,
                visible_height,
                width,
                -(MOUSE_SCROLL_LINES as isize),
            )
        }
        MouseEventKind::ScrollRight if !state.wrap => {
            navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollLeft if !state.wrap => {
            navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    DiffEventAction { dirty, quit: false }
}

#[derive(Debug, Default)]
pub(super) struct DiffEventAction {
    pub(super) dirty: bool,
    pub(super) quit: bool,
}

impl DiffEventAction {
    fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
    }
}

fn plain_key(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}
