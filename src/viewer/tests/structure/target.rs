use super::*;

#[test]
fn structure_target_moves_even_when_already_visible() {
    let mut state = ViewState {
        structure_target: Some(SearchTarget {
            line: 4,
            byte_index: 0,
        }),
        ..ViewState::default()
    };

    assert!(resolve_structure_target_position(
        &mut state,
        &[
            "{".to_owned(),
            r#"  "a": 1,"#.to_owned(),
            r#"  "b": {"#.to_owned(),
            "  },".to_owned(),
            r#"  "c": {"#.to_owned(),
        ],
        10,
        RenderContext {
            gutter_digits: 1,
            x: 0,
            width: 80,
            wrap: false,
            mode: FormatKind::Json,
        }
    ));
    assert_eq!(state.top, 4);
    assert!(state.structure_target.is_none());
}

#[test]
fn structure_target_near_eof_clamps_to_last_full_page() {
    let file = indexed_file(&[
        "{",
        r#"  "a": {"#,
        "  },",
        r#"  "b": {"#,
        "  },",
        r#"  "tail": {"#,
        "  }",
        "}",
    ]);
    let mut state = ViewState {
        structure_target: Some(SearchTarget {
            line: 5,
            byte_index: 2,
        }),
        ..ViewState::default()
    };
    let mut line_cache = LineWindowCache::default();
    let mut tail_cache = TailPositionCache::default();
    let context = RenderContext {
        gutter_digits: 1,
        x: 0,
        width: 80,
        wrap: false,
        mode: FormatKind::Json,
    };

    let tail = resolve_targets_from_view(
        &file,
        &mut state,
        &mut line_cache,
        4,
        context,
        &mut tail_cache,
    )
    .unwrap();

    assert_eq!(
        tail,
        Some(ViewPosition {
            top: 4,
            row_offset: 0
        })
    );
    assert_eq!(state.top, 4);
    assert_eq!(state.top_row_offset, 0);
    assert!(state.structure_target.is_none());
    assert_eq!(file.read_window(state.top, 4).unwrap().len(), 4);
}
