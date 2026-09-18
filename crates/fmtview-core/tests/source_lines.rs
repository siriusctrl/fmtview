use std::{
    io::Write,
    time::{Duration, Instant},
};

use fmtview_core::{
    FileViewer, FormatKind, FormatOptions, InputEvent, InputSource, KeyCode, KeyModifiers,
    TypeProfile, ViewFile, open_view_file,
};
use ratatui::layout::Size;
use tempfile::NamedTempFile;

fn open(text: &str, kind: FormatKind) -> Box<dyn ViewFile> {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(text.as_bytes()).unwrap();
    let source = InputSource::from_temp(temp, "source fixture");
    let options = FormatOptions { kind, indent: 2 };
    let profile = TypeProfile::resolve(&source, &options).unwrap();
    open_view_file(&source, &options, profile).unwrap().file
}

fn source_numbers(file: &dyn ViewFile) -> Vec<usize> {
    (0..file.line_count())
        .map(|line| file.source_line(line).unwrap())
        .collect()
}

#[test]
fn json_source_coordinates_preserve_blank_lines_crlf_and_token_positions() {
    let file = open(
        "\r\n{\r\n\r\n  \"a\": [1,2],\r\n  \"text\": \"escaped\\nvalue\"\r\n}\r\n\r\n",
        FormatKind::Json,
    );
    assert_eq!(file.source_line_count(), Some(7));
    assert_eq!(source_numbers(file.as_ref()), vec![2, 4, 4, 4, 4, 5, 6]);
    assert_eq!(file.display_line_for_source(3), Some(1));
    assert_eq!(file.display_line_for_source(4), Some(1));
    assert_eq!(file.display_line_for_source(999), Some(6));
    assert_eq!(file.display_line_for_source(0), Some(0));
}

#[test]
fn compact_documents_and_multiline_markup_keep_source_coordinates() {
    for (kind, input) in [
        (FormatKind::Json, "{\"items\":[1,2]}"),
        (FormatKind::Xml, "<root><a>1</a><b/></root>"),
        (FormatKind::Html, "<div><p>one<p>two<br></div>"),
    ] {
        let file = open(input, kind);
        assert!(file.line_count() > 1);
        assert!(source_numbers(file.as_ref()).iter().all(|&n| n == 1));
        assert_eq!(file.source_line_count(), Some(1));
    }
    for kind in [FormatKind::Xml, FormatKind::Html] {
        let file = open("<root>\n<a>one</a>\n\n<b>two</b>\n</root>\n", kind);
        let lines = file.read_window(0, file.line_count()).unwrap();
        let a = lines.iter().position(|line| line.contains("<a>")).unwrap();
        let b = lines.iter().position(|line| line.contains("<b>")).unwrap();
        assert_eq!(file.source_line(a), Some(2));
        assert_eq!(file.source_line(b), Some(4));
    }
    let file = open("<pre>first\n\nthird</pre>\n", FormatKind::Html);
    assert_eq!(source_numbers(file.as_ref()), vec![1, 2, 3, 3]);
}

#[test]
fn xml_bom_and_multiline_tokens_retain_their_source_positions() {
    let file = open(
        "\u{feff}<?xml version=\"1.0\"?>\n<root>\n<!-- first\nsecond -->\n<a value=\"one\ntwo\">x&amp;y</a>\n</root>",
        FormatKind::Xml,
    );
    let lines = file.read_window(0, file.line_count()).unwrap();
    for (needle, expected) in [
        ("<?xml", 1),
        ("<root>", 2),
        ("<!-- first", 3),
        ("second", 4),
        ("<a value", 5),
        ("two", 6),
        ("</root>", 7),
    ] {
        let row = lines.iter().position(|line| line.contains(needle)).unwrap();
        assert_eq!(file.source_line(row), Some(expected), "{needle}");
    }
}

#[test]
fn passthrough_and_empty_files_keep_physical_line_numbers() {
    for kind in [
        FormatKind::Plain,
        FormatKind::Markdown,
        FormatKind::Toml,
        FormatKind::Jinja,
    ] {
        let file = open("first\r\n\r\nthird", kind);
        assert_eq!(source_numbers(file.as_ref()), vec![1, 2, 3]);
        assert_eq!(file.source_line_count(), Some(3));
    }
    let empty = open("", FormatKind::Plain);
    assert_eq!(empty.source_line_count(), Some(0));
    assert_eq!(empty.display_line_for_source(50), Some(0));
}

#[test]
fn lazy_jsonl_counts_blank_and_malformed_source_lines_and_preserves_raw_origin() {
    let file = open("{\"a\":1}\r\n\r\nbad json\r\n{\"b\":2}", FormatKind::Jsonl);
    while !file.line_count_exact() {
        file.preload(100, 10, Duration::from_secs(1)).unwrap();
    }
    assert_eq!(source_numbers(file.as_ref()), vec![1, 1, 1, 2, 3, 4, 4, 4]);
    assert_eq!(file.source_line_count(), Some(4));
    assert_eq!(file.display_line_for_source(4), Some(5));
    let raw = file.open_raw_record(6).unwrap().unwrap();
    assert_eq!(raw.source_line(0), Some(4));
    assert!(raw.label().contains("source line 4"));
    assert_eq!(raw.read_window(0, 1).unwrap(), vec!["{\"b\":2}"]);
}

fn key(viewer: &mut FileViewer, code: KeyCode) {
    viewer.handle_event(
        InputEvent::Key {
            code,
            modifiers: KeyModifiers::NONE,
        },
        8,
    );
}

#[test]
fn gutter_jump_search_and_raw_view_share_original_coordinates() {
    let file = open("{\"a\":[1,2]}\n{\"b\":2}\n{\"c\":3}\n", FormatKind::Jsonl);
    let mut viewer = FileViewer::new(file, FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    let first = viewer.render(size, None).unwrap();
    assert_eq!(first.styled[0].spans[0].content.trim(), "1 │");
    assert_eq!(first.styled[1].spans[0].content.trim(), "┆");
    for code in [KeyCode::Char('2'), KeyCode::Enter] {
        key(&mut viewer, code);
    }
    for _ in 0..100 {
        if !viewer.needs_immediate_advance() {
            break;
        }
        viewer.advance(Instant::now()).unwrap();
    }
    let jumped = viewer.render(Size::new(60, 6), None).unwrap();
    assert_eq!(jumped.position.top, 6);
    assert_eq!(jumped.styled[0].spans[0].content.trim(), "2 │");
    assert!(jumped.title.contains("2-2"), "{}", jumped.title);
    key(&mut viewer, KeyCode::Char('r'));
    let raw = viewer.render(size, None).unwrap();
    assert!(raw.title.contains("source line 2"));
    assert_eq!(raw.styled[0].spans[0].content.trim(), "2 │");
    key(&mut viewer, KeyCode::Char('r'));
    for ch in "/c".chars() {
        key(&mut viewer, KeyCode::Char(ch));
    }
    key(&mut viewer, KeyCode::Enter);
    for _ in 0..100 {
        if !viewer.needs_immediate_advance() {
            break;
        }
        viewer.advance(Instant::now()).unwrap();
    }
    let searched = viewer.render(Size::new(60, 5), None).unwrap();
    assert!(searched.title.contains("3-3"), "{}", searched.title);
}

#[test]
fn jump_to_unloaded_jsonl_source_line_is_bounded_and_cancellable() {
    let text: String = (1..=1200).map(|n| format!("{{\"n\":{n}}}\n")).collect();
    let mut viewer = FileViewer::new(open(&text, FormatKind::Jsonl), FormatKind::Jsonl, None);
    viewer.render(Size::new(60, 6), None).unwrap();
    for ch in "1000".chars() {
        key(&mut viewer, KeyCode::Char(ch));
    }
    key(&mut viewer, KeyCode::Enter);
    assert!(viewer.needs_immediate_advance());
    assert!(
        viewer
            .render(Size::new(60, 6), None)
            .unwrap()
            .footer_text
            .contains("finding source line 1000")
    );
    key(&mut viewer, KeyCode::Esc);
    assert!(!viewer.needs_immediate_advance());
    for ch in "1000".chars() {
        key(&mut viewer, KeyCode::Char(ch));
    }
    key(&mut viewer, KeyCode::Enter);
    for _ in 0..1000 {
        if !viewer.needs_immediate_advance() {
            break;
        }
        viewer.advance(Instant::now()).unwrap();
    }
    assert!(!viewer.needs_immediate_advance());
    let jumped = viewer.render(Size::new(60, 6), None).unwrap();
    assert_eq!(jumped.position.top, 999 * 3);
    assert_eq!(jumped.styled[0].spans[0].content.trim(), "1000 │");
}
