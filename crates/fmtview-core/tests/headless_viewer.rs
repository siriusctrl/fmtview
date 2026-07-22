use std::io::Write;

use fmtview_core::{
    ContentShape, DiffViewer, FileViewer, FormatKind, FormatOptions, InputEvent, InputSource,
    KeyCode, KeyModifiers, LoadPlan, TypeProfile, diff_view, open_view_file,
    render_frame_to_buffer,
};
use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
};
use tempfile::NamedTempFile;

#[test]
fn file_engine_renders_and_searches_without_a_terminal() {
    let source = source(
        "headless.json",
        "{\"items\":[{\"name\":\"alpha\"},{\"name\":\"beta\"}]}\n",
    );
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };
    let profile = TypeProfile::resolve(&source, &options).unwrap();
    assert_eq!(profile.content_kind(), FormatKind::Json);
    assert_eq!(profile.content_shape(), ContentShape::WholeDocument);
    assert_eq!(profile.load_plan(), LoadPlan::EagerTransformedDocument);
    let opened = open_view_file(&source, &options, profile).unwrap();
    let mut viewer = FileViewer::new(opened.file, opened.content, opened.notice);
    let size = Size::new(60, 12);

    let first = viewer.render(size, None).unwrap();
    assert!(first.title.contains("headless.json"));
    assert!(buffer_text(first).contains("alpha"));

    for code in [
        KeyCode::Char('/'),
        KeyCode::Char('b'),
        KeyCode::Char('e'),
        KeyCode::Char('t'),
        KeyCode::Char('a'),
        KeyCode::Enter,
    ] {
        viewer.handle_event(
            InputEvent::Key {
                code,
                modifiers: KeyModifiers::NONE,
            },
            FileViewer::page_for_size(size),
        );
    }
    while viewer.advance(std::time::Instant::now()).unwrap() {}

    let searched = viewer.render(size, None).unwrap();
    assert!(
        searched.footer_text.contains("1/1 match"),
        "unexpected search footer: {}",
        searched.footer_text
    );
    assert!(buffer_text(searched).contains("beta"));
}

#[test]
fn file_engine_navigation_changes_backend_neutral_frame_position() {
    let source = source("lines.txt", "zero\none\ntwo\nthree\nfour\n");
    let options = FormatOptions {
        kind: FormatKind::Plain,
        indent: 2,
    };
    let profile = TypeProfile::resolve(&source, &options).unwrap();
    let opened = open_view_file(&source, &options, profile).unwrap();
    let mut viewer = FileViewer::new(opened.file, opened.content, opened.notice);
    let size = Size::new(40, 6);
    let first = viewer.render(size, None).unwrap();

    let action = viewer.handle_event(
        InputEvent::Key {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
        },
        FileViewer::page_for_size(size),
    );
    assert!(action.dirty);
    let next = viewer.render(size, Some(first.position)).unwrap();
    assert_eq!(next.position.top, 1);
}

#[test]
fn diff_engine_renders_and_navigates_without_a_terminal() {
    let left = source("left.json", "{\"value\":1}\n");
    let right = source("right.json", "{\"value\":2}\n");
    let options = FormatOptions {
        kind: FormatKind::Json,
        indent: 2,
    };
    let size = Size::new(100, 12);
    let mut viewer = DiffViewer::new(diff_view(&left, &right, &options).unwrap(), size).unwrap();

    let first = viewer.render(size, None);
    assert!(first.title.contains("diff"));
    assert!(buffer_text(first).contains("value"));

    let action = viewer.handle_event(
        InputEvent::Key {
            code: KeyCode::Char('s'),
            modifiers: KeyModifiers::NONE,
        },
        size,
    );
    assert!(action.dirty);
    let next = viewer.render(size, None);
    assert!(!next.styled.is_empty());
}

fn source(label: &str, text: &str) -> InputSource {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(text.as_bytes()).unwrap();
    temp.flush().unwrap();
    InputSource::from_temp(temp, label)
}

fn buffer_text(frame: fmtview_core::RenderFrame) -> String {
    let mut buffer = Buffer::empty(Rect::new(0, 0, frame.area.width, frame.area.height));
    render_frame_to_buffer(&mut buffer, frame);
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}
