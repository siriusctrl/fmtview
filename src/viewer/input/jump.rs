use crossterm::event::{KeyCode, KeyModifiers};

use super::super::JUMP_BUFFER_MAX_DIGITS;
use super::{keys::accepts_jump_digit, scroll::set_top, state::ViewState};

pub(in crate::viewer) fn handle_jump_input_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
) -> bool {
    match code {
        KeyCode::Char(ch) if accepts_jump_digit(ch, modifiers) => {
            push_jump_digit(state, ch);
            true
        }
        KeyCode::Enter => jump_to_buffered_line(state, line_count, line_count_exact),
        KeyCode::Backspace => pop_jump_digit(state),
        KeyCode::Esc => clear_jump_buffer(state),
        _ => false,
    }
}

pub(in crate::viewer) fn push_jump_digit(state: &mut ViewState, ch: char) {
    if state.jump_buffer.len() < JUMP_BUFFER_MAX_DIGITS {
        state.jump_buffer.push(ch);
    }
}

pub(in crate::viewer) fn pop_jump_digit(state: &mut ViewState) -> bool {
    let old_len = state.jump_buffer.len();
    state.jump_buffer.pop();
    state.jump_buffer.len() != old_len
}

pub(in crate::viewer) fn clear_jump_buffer(state: &mut ViewState) -> bool {
    let was_active = !state.jump_buffer.is_empty();
    state.jump_buffer.clear();
    was_active
}

pub(in crate::viewer) fn jump_to_buffered_line(
    state: &mut ViewState,
    line_count: usize,
    line_count_exact: bool,
) -> bool {
    if state.jump_buffer.is_empty() {
        return false;
    }

    let requested = state.jump_buffer.parse::<usize>().unwrap_or(usize::MAX);
    state.jump_buffer.clear();
    set_top(
        state,
        target_top_for_line(requested, line_count, line_count_exact),
    );
    true
}

pub(in crate::viewer) fn target_top_for_line(
    requested: usize,
    line_count: usize,
    line_count_exact: bool,
) -> usize {
    if !line_count_exact {
        return requested.max(1).saturating_sub(1);
    }

    if line_count == 0 {
        return 0;
    }

    requested.max(1).min(line_count).saturating_sub(1)
}
