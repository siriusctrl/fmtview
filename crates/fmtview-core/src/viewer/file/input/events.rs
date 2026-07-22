use crate::viewer::{InputEvent, KeyCode, KeyModifiers, MouseEventKind, ViewerAction};

use crate::viewer::file::{MOUSE_HORIZONTAL_COLUMNS, MOUSE_SCROLL_LINES};

use super::super::structure::{StructureDirection, start_structure_navigation};
use super::{
    jump::{handle_jump_input_key, push_jump_digit},
    keys::{accepts_jump_digit, plain_key},
    scroll::{
        page_down, page_up, reset_top_row_offset, scroll_down, scroll_down_by, scroll_up,
        scroll_up_by, scroll_x_by, set_file_end, set_top,
    },
    search::{
        SearchDirection, clear_footer_message, clear_search_session, handle_search_input_key,
        start_repeat_search, start_search_prompt,
    },
    state::ViewState,
};

#[cfg(test)]
pub(in crate::viewer) fn handle_event(
    event: InputEvent,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> ViewerAction {
    handle_event_with_count(event, state, line_count, true, page)
}

pub(in crate::viewer) fn handle_event_with_count(
    event: InputEvent,
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
    page: usize,
) -> ViewerAction {
    match event {
        InputEvent::Key { code, modifiers } => {
            handle_key_event_with_count(code, modifiers, state, line_count, line_count_exact, page)
        }
        InputEvent::Mouse { kind, modifiers } if !state.has_active_prompt() => {
            handle_mouse_event(kind, modifiers, state, line_count)
        }
        InputEvent::Resize => ViewerAction {
            dirty: true,
            ..ViewerAction::default()
        },
        InputEvent::Mouse { .. } | InputEvent::Command(_) | InputEvent::Ignore => {
            ViewerAction::default()
        }
    }
}

#[cfg(test)]
pub(in crate::viewer) fn handle_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
    page: usize,
) -> ViewerAction {
    handle_key_event_with_count(code, modifiers, state, line_count, true, page)
}

pub(in crate::viewer) fn handle_key_event_with_count(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
    page: usize,
) -> ViewerAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return ViewerAction {
            quit: true,
            ..ViewerAction::default()
        };
    }

    if state.search_active {
        return ViewerAction {
            dirty: handle_search_input_key(code, modifiers, state, line_count),
            ..ViewerAction::default()
        };
    }

    if !state.jump_buffer.is_empty() {
        return ViewerAction {
            dirty: handle_jump_input_key(code, modifiers, state, line_count, line_count_exact),
            ..ViewerAction::default()
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
        KeyCode::Char(']') if plain_key(modifiers) => start_structure_navigation(
            state,
            line_count,
            line_count_exact,
            StructureDirection::Forward,
        ),
        KeyCode::Char('[') if plain_key(modifiers) => start_structure_navigation(
            state,
            line_count,
            line_count_exact,
            StructureDirection::Backward,
        ),
        KeyCode::Enter => false,
        KeyCode::Esc if state.has_search_session() => clear_search_session(state),
        KeyCode::Esc if state.footer_message.is_some() => clear_footer_message(state),
        KeyCode::Char('q') | KeyCode::Esc => {
            return ViewerAction {
                quit: true,
                ..ViewerAction::default()
            };
        }
        KeyCode::Char('m') if plain_key(modifiers) => {
            state.mouse_capture = !state.mouse_capture;
            return ViewerAction {
                dirty: true,
                mouse_capture: Some(state.mouse_capture),
                ..ViewerAction::default()
            };
        }
        KeyCode::Char('w') => {
            state.wrap = !state.wrap;
            reset_top_row_offset(state);
            true
        }
        KeyCode::Char('t') if plain_key(modifiers) => state.toggle_tool_pair(),
        KeyCode::Down | KeyCode::Char('j') => {
            let dirty = scroll_down(state, line_count);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let dirty = scroll_up(state, line_count);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
            let dirty = page_down(state, line_count, page);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::PageUp | KeyCode::Char('b') => {
            let dirty = page_up(state, line_count, page);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::Home | KeyCode::Char('g') => {
            let dirty = set_top(state, 0);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::End | KeyCode::Char('G') => {
            let dirty = set_file_end(state, line_count);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        KeyCode::Right | KeyCode::Char('l') if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
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
    state: &mut ViewState,
    line_count: usize,
) -> ViewerAction {
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) && !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        MouseEventKind::ScrollDown => {
            let dirty = scroll_down_by(state, line_count, MOUSE_SCROLL_LINES);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        MouseEventKind::ScrollUp => {
            let dirty = scroll_up_by(state, line_count, MOUSE_SCROLL_LINES);
            clear_structure_cursor_if_dirty(state, dirty)
        }
        MouseEventKind::ScrollRight if !state.wrap => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollLeft if !state.wrap => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    ViewerAction {
        dirty,
        ..ViewerAction::default()
    }
}

fn clear_structure_cursor_if_dirty(state: &mut ViewState, dirty: bool) -> bool {
    if dirty {
        state.structure_cursor = None;
        state.preserve_tail_on_next_draw = false;
        state.clear_tool_navigation();
    }
    dirty
}
