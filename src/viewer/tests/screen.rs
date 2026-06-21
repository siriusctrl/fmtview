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
fn ansi_draw_writes_compact_indexed_colors() {
    let mut cell = ratatui::buffer::Cell::EMPTY;
    cell.set_symbol("x").set_fg(PALETTE_BLUE);
    let mut output = Vec::new();

    draw_cells(&mut output, vec![(0, 0, &cell)]).unwrap();

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("\x1b[38;5;75m"));
    assert!(!output.contains("\x1b[49m"));
}

#[test]
fn terminal_renderer_applies_plain_style_to_default_spans() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let writer = CapturingWriter {
        output: Rc::clone(&output),
    };
    let backend = CrosstermBackend::new(writer);
    let mut terminal = ViewerTerminal::new(backend);

    terminal
        .draw(TerminalFrame {
            area: Rect::new(0, 0, 24, 4),
            styled: vec![Line::from(Span::raw("plain text"))],
            sticky: Vec::new(),
            selection_mode: false,
            title: " test ".to_owned(),
            footer_text: " footer ".to_owned(),
            footer_style: gutter_style(),
            position: ViewPosition {
                top: 0,
                row_offset: 0,
            },
            scroll_hint: None,
        })
        .unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    assert!(output.contains("\x1b[38;5;145mplain text"));
}

#[test]
fn selection_mode_draws_body_without_frame() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let writer = CapturingWriter {
        output: Rc::clone(&output),
    };
    let backend = CrosstermBackend::new(writer);
    let mut terminal = ViewerTerminal::new(backend);

    terminal
        .draw(TerminalFrame {
            area: Rect::new(0, 0, 24, 4),
            styled: vec![Line::from(Span::raw("plain text"))],
            sticky: Vec::new(),
            selection_mode: true,
            title: " test ".to_owned(),
            footer_text: " footer ".to_owned(),
            footer_style: gutter_style(),
            position: ViewPosition {
                top: 0,
                row_offset: 0,
            },
            scroll_hint: None,
        })
        .unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    assert!(output.contains("plain text"));
    assert!(!output.contains("┌"));
    assert!(!output.contains("│"));
}

#[test]
fn selection_mode_change_forces_full_redraw_even_with_scroll_hint() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let writer = CapturingWriter {
        output: Rc::clone(&output),
    };
    let backend = CrosstermBackend::new(writer);
    let mut terminal = ViewerTerminal::new(backend);

    terminal
        .draw(TerminalFrame {
            area: Rect::new(0, 0, 24, 5),
            styled: vec![Line::from(Span::raw("plain text"))],
            sticky: Vec::new(),
            selection_mode: true,
            title: " test ".to_owned(),
            footer_text: " footer ".to_owned(),
            footer_style: gutter_style(),
            position: ViewPosition {
                top: 0,
                row_offset: 0,
            },
            scroll_hint: None,
        })
        .unwrap();
    let position = ViewPosition {
        top: 0,
        row_offset: 1,
    };
    let scroll_hint = terminal.scroll_hint(position);
    assert!(scroll_hint.is_some());

    terminal
        .draw(TerminalFrame {
            area: Rect::new(0, 0, 24, 5),
            styled: vec![Line::from(Span::raw("plain text"))],
            sticky: Vec::new(),
            selection_mode: false,
            title: " test ".to_owned(),
            footer_text: " footer ".to_owned(),
            footer_style: gutter_style(),
            position,
            scroll_hint,
        })
        .unwrap();

    let output = String::from_utf8(output.borrow().clone()).unwrap();
    assert!(output.matches("\x1b[2J").count() >= 2);
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
