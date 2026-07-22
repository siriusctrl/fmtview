use super::*;

#[test]
fn mouse_wheel_scrolls_one_row() {
    let mut state = ViewState::default();
    let action = handle_event(
        mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert!(!action.quit);
    assert_eq!(state.top, MOUSE_SCROLL_LINES);

    let action = handle_event(
        mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.top, 0);
}

#[test]
fn down_scrolls_inside_overflowing_wrapped_line_first() {
    let mut state = ViewState {
        top_max_row_offset: 2,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, 1);
    assert!(!state.wrap_bounds_stale);

    state.top_row_offset = state.top_max_row_offset;
    let action = handle_key_event(KeyCode::Down, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 1);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn batched_scroll_stops_after_crossing_to_unmeasured_wrapped_line() {
    let mut state = ViewState::default();

    assert!(scroll_down_by(&mut state, 10, 3));

    assert_eq!(state.top, 1);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.wrap_bounds_stale);
}

#[test]
fn up_from_logical_line_targets_previous_lines_last_row() {
    let mut state = ViewState {
        top: 1,
        ..ViewState::default()
    };

    let action = handle_key_event(KeyCode::Up, KeyModifiers::NONE, &mut state, 3, 5);

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.top_row_offset, LAST_ROW_OFFSET);
    assert!(state.wrap_bounds_stale);
}
