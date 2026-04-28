use super::super::TAIL_ROW_OFFSET;
use super::state::ViewState;

pub(in crate::viewer) fn scroll_down(state: &mut ViewState, line_count: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset < state.top_max_row_offset {
        state.top_row_offset = state.top_row_offset.saturating_add(1);
        return true;
    }

    let old = state.top;
    state.top = state
        .top
        .saturating_add(1)
        .min(line_count.saturating_sub(1));
    if state.top != old {
        reset_top_row_offset(state);
        return true;
    }

    false
}

pub(in crate::viewer) fn scroll_up(state: &mut ViewState, line_count: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset > 0 {
        state.top_row_offset = state.top_row_offset.saturating_sub(1);
        return true;
    }

    let old = state.top;
    state.top = state.top.saturating_sub(1);
    if state.top != old {
        state.top_row_offset = if state.wrap { TAIL_ROW_OFFSET } else { 0 };
        state.top_max_row_offset = 0;
        state.wrap_bounds_stale = state.wrap;
        return true;
    }

    false
}

pub(in crate::viewer) fn scroll_down_by(
    state: &mut ViewState,
    line_count: usize,
    rows: usize,
) -> bool {
    let mut dirty = false;
    for _ in 0..rows {
        if !scroll_down(state, line_count) {
            break;
        }
        dirty = true;
        if state.wrap_bounds_stale {
            break;
        }
    }
    dirty
}

pub(in crate::viewer) fn scroll_up_by(
    state: &mut ViewState,
    line_count: usize,
    rows: usize,
) -> bool {
    let mut dirty = false;
    for _ in 0..rows {
        if !scroll_up(state, line_count) {
            break;
        }
        dirty = true;
        if state.wrap_bounds_stale {
            break;
        }
    }
    dirty
}

pub(in crate::viewer) fn page_down(state: &mut ViewState, line_count: usize, page: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset < state.top_max_row_offset {
        state.top_row_offset = state
            .top_row_offset
            .saturating_add(page)
            .min(state.top_max_row_offset);
        return true;
    }

    scroll_logical_by(state, line_count, page as isize)
}

pub(in crate::viewer) fn page_up(state: &mut ViewState, line_count: usize, page: usize) -> bool {
    if line_count == 0 {
        return false;
    }

    if state.wrap && state.top_row_offset > 0 {
        state.top_row_offset = state.top_row_offset.saturating_sub(page);
        return true;
    }

    scroll_logical_by(state, line_count, -(page as isize))
}

pub(in crate::viewer) fn scroll_logical_by(
    state: &mut ViewState,
    line_count: usize,
    delta: isize,
) -> bool {
    let old = state.top;
    if delta >= 0 {
        state.top = state
            .top
            .saturating_add(delta as usize)
            .min(line_count.saturating_sub(1));
    } else {
        state.top = state.top.saturating_sub(delta.unsigned_abs());
    }

    if state.top != old {
        reset_top_row_offset(state);
        return true;
    }

    false
}

pub(in crate::viewer) fn set_top(state: &mut ViewState, value: usize) -> bool {
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    if old_top == value && old_offset == 0 {
        return false;
    }

    state.top = value;
    reset_top_row_offset(state);
    true
}

pub(in crate::viewer) fn set_file_end(state: &mut ViewState, line_count: usize) -> bool {
    let value = line_count.saturating_sub(1);
    let target_offset = if line_count > 0 && state.wrap {
        TAIL_ROW_OFFSET
    } else {
        0
    };
    let old_top = state.top;
    let old_offset = state.top_row_offset;
    if old_top == value && old_offset == target_offset {
        return false;
    }

    state.top = value;
    state.top_row_offset = target_offset;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
    true
}

pub(in crate::viewer) fn reset_top_row_offset(state: &mut ViewState) {
    state.top_row_offset = 0;
    state.top_max_row_offset = 0;
    state.wrap_bounds_stale = state.wrap;
}

pub(in crate::viewer) fn scroll_x_by(x: &mut usize, delta: isize) -> bool {
    let old = *x;
    if delta >= 0 {
        *x = x.saturating_add(delta as usize);
    } else {
        *x = x.saturating_sub(delta.unsigned_abs());
    }
    *x != old
}
