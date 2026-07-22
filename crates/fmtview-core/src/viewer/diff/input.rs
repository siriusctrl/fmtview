use crate::{
    diff::DiffModel,
    viewer::{InputEvent, KeyCode, KeyModifiers, MouseEventKind, ViewerAction},
};

const MOUSE_SCROLL_LINES: usize = 1;
const MOUSE_HORIZONTAL_COLUMNS: usize = 4;

use super::DiffViewState;

#[cfg(test)]
pub(super) use super::navigation::change_block_starts;
pub(super) use super::navigation::{DiffJump, clamp_top, jump_change, scroll_by};

pub(super) fn handle_event(
    event: InputEvent,
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
    width: usize,
) -> ViewerAction {
    match event {
        InputEvent::Key { code, modifiers } => {
            handle_key_event(code, modifiers, model, state, page, visible_height, width)
        }
        InputEvent::Mouse { kind, modifiers } => {
            handle_mouse_event(kind, modifiers, model, state, visible_height, width)
        }
        InputEvent::Resize => ViewerAction {
            dirty: true,
            ..ViewerAction::default()
        },
        InputEvent::Ignore => ViewerAction::default(),
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
) -> ViewerAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return ViewerAction {
            quit: true,
            ..ViewerAction::default()
        };
    }

    let line_count = model.row_count(state.layout);
    let dirty = match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            return ViewerAction {
                quit: true,
                ..ViewerAction::default()
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
            super::navigation::set_top(state, 0, 0, line_count)
        }
        KeyCode::End | KeyCode::Char('G') if plain_key(modifiers) => {
            state.change_cursor = None;
            super::navigation::set_tail_top(state, model, visible_height, width)
        }
        KeyCode::Right | KeyCode::Char('l') if plain_key(modifiers) && !state.wrap => {
            super::navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if plain_key(modifiers) && !state.wrap => {
            super::navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    ViewerAction {
        dirty,
        ..ViewerAction::default()
    }
}

fn handle_mouse_event(
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    model: &DiffModel,
    state: &mut DiffViewState,
    visible_height: usize,
    width: usize,
) -> ViewerAction {
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            super::navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            super::navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
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
            super::navigation::scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollLeft if !state.wrap => {
            super::navigation::scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    ViewerAction {
        dirty,
        ..ViewerAction::default()
    }
}

fn plain_key(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}
