use super::*;

#[test]
fn syntax_palette_uses_muted_indexed_colors() {
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
            title: " test ".to_owned(),
            footer_text: " footer ".to_owned(),
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
