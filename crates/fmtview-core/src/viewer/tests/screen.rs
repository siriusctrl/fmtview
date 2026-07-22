use super::*;

#[test]
fn highlight_palette_uses_muted_indexed_colors() {
    assert_eq!(plain_style().fg, Some(PALETTE_TEXT));
    assert_eq!(plain_style().bg, None);
    assert_eq!(gutter_style().fg, Some(PALETTE_MUTED));
    assert_eq!(punctuation_style().fg, Some(PALETTE_PUNCTUATION));
    assert_ne!(punctuation_style().fg, gutter_style().fg);
    assert_ne!(punctuation_style().fg, plain_style().fg);
    assert_eq!(key_style().fg, Some(PALETTE_BLUE));
    assert_eq!(string_style().fg, Some(PALETTE_GREEN));
    assert_eq!(number_style().fg, Some(PALETTE_ORANGE));
    assert_eq!(error_style().fg, Some(PALETTE_RED));
    assert_eq!(search_match_bg(), PALETTE_SEARCH_MATCH);
    assert_eq!(search_inactive_match_bg(), PALETTE_SEARCH_MATCH_DIM);
}

#[test]
fn shifted_wheel_scrolls_horizontally_in_nowrap() {
    let mut state = ViewState {
        wrap: false,
        ..ViewState::default()
    };
    let action = handle_event(
        mouse_event(MouseEventKind::ScrollDown, KeyModifiers::SHIFT),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.top, 0);
    assert_eq!(state.x, MOUSE_HORIZONTAL_COLUMNS);

    let action = handle_event(
        mouse_event(MouseEventKind::ScrollUp, KeyModifiers::SHIFT),
        &mut state,
        10,
        5,
    );

    assert!(action.dirty);
    assert_eq!(state.x, 0);
}

#[test]
fn mouse_capture_toggle_reports_terminal_mode_change() {
    let mut state = ViewState::default();

    let action = handle_key_event(KeyCode::Char('m'), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert_eq!(action.mouse_capture, Some(false));
    assert!(!state.mouse_capture);

    let action = handle_key_event(KeyCode::Char('m'), KeyModifiers::NONE, &mut state, 10, 5);
    assert!(action.dirty);
    assert_eq!(action.mouse_capture, Some(true));
    assert!(state.mouse_capture);
}
