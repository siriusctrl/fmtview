use std::{
    cell::RefCell,
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    rc::Rc,
    time::Instant,
};

use fmtview_core::{
    FileRecordTimeline, FileViewer, FormatKind, FormatOptions, InputEvent, KeyCode, KeyModifiers,
    MouseEventKind, RecordId, RecordLoadLimit, RecordTimeline, RecordTimelineViewFile,
    TimelineRead, TimelineRecord, TimelineRefresh, TimelineResetReason, TimelineSnapshot, ViewFile,
    ViewerCommand, render_frame_to_buffer,
};
use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
};
use tempfile::NamedTempFile;

const JSONL: FormatOptions = FormatOptions {
    kind: FormatKind::Jsonl,
    indent: 2,
};

#[test]
fn fake_timeline_distinguishes_pending_from_terminal_end() {
    let (handle, mut timeline) = fake_timeline([]);

    assert_eq!(
        timeline.load_newer(RecordLoadLimit::new(8, 1024)).unwrap(),
        TimelineRead::Pending
    );
    handle.state.borrow_mut().terminal = true;
    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::End(_)
    ));
    assert_eq!(
        timeline.load_newer(RecordLoadLimit::new(8, 1024)).unwrap(),
        TimelineRead::End
    );
}

#[test]
fn follow_tail_advances_detaches_and_reattaches_headlessly() {
    let (handle, timeline) = fake_timeline((0..6).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);

    let first = viewer.render(size, None).unwrap();
    assert!(
        first.footer_text.contains("follow:on"),
        "{}",
        first.footer_text
    );
    let first_text = frame_text(first);
    assert!(first_text.contains("record-5"), "{first_text:?}");

    handle.append(record(6));
    assert!(viewer.preload().unwrap());
    let appended = viewer.render(size, None).unwrap();
    let appended_text = frame_text(appended);
    assert!(appended_text.contains("record-6"), "{appended_text:?}");

    let action = viewer.handle_event(key(KeyCode::Up), FileViewer::page_for_size(size));
    assert!(action.dirty);
    let detached = viewer.render(size, None).unwrap();
    let detached_top = detached.position.top;
    assert!(detached.footer_text.contains("follow:detached"));

    handle.append(record(7));
    assert!(viewer.preload().unwrap());
    let stayed = viewer.render(size, None).unwrap();
    assert_eq!(stayed.position.top, detached_top);

    viewer.handle_event(
        InputEvent::Command(ViewerCommand::FollowTail),
        FileViewer::page_for_size(size),
    );
    let reattached = viewer.render(size, None).unwrap();
    assert!(
        reattached.footer_text.contains("follow:on"),
        "title={} footer={} position={:?}",
        reattached.title,
        reattached.footer_text,
        reattached.position
    );
    assert!(frame_text(reattached).contains("record-7"));
}

#[test]
fn follow_can_pause_at_bottom_without_an_append_jump() {
    let (handle, timeline) = fake_timeline((0..6).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    let first = viewer.render(size, None).unwrap();
    let first_top = first.position.top;

    viewer.handle_event(key(KeyCode::Char('f')), FileViewer::page_for_size(size));
    let paused = viewer.render(size, None).unwrap();
    assert!(paused.footer_text.contains("follow:off"));

    handle.append(record(6));
    assert!(viewer.preload().unwrap());
    let after_append = viewer.render(size, None).unwrap();
    assert_eq!(after_append.position.top, first_top);
    assert!(after_append.footer_text.contains("follow:off"));
}

#[test]
fn follow_search_prompt_accepts_the_f_character() {
    let (_handle, timeline) = fake_timeline([b"{\"message\":\"find-me\"}\n".to_vec()]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    viewer.render(size, None).unwrap();

    viewer.handle_event(key(KeyCode::Char('/')), FileViewer::page_for_size(size));
    for ch in "fgjb".chars() {
        viewer.handle_event(key(KeyCode::Char(ch)), FileViewer::page_for_size(size));
    }
    let frame = viewer.render(size, None).unwrap();

    assert!(
        frame.footer_text.contains("follow:on"),
        "{}",
        frame.footer_text
    );
    assert!(
        frame.footer_text.contains("search: fgjb"),
        "{}",
        frame.footer_text
    );
}

#[test]
fn end_navigation_does_not_resume_explicitly_paused_follow() {
    let (_handle, timeline) = fake_timeline((0..10).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    viewer.render(size, None).unwrap();
    viewer.handle_event(key(KeyCode::Char('f')), FileViewer::page_for_size(size));

    for code in [KeyCode::End, KeyCode::Char('G')] {
        viewer.handle_event(key(code), FileViewer::page_for_size(size));
        let frame = viewer.render(size, None).unwrap();
        assert!(
            frame.footer_text.contains("follow:off"),
            "{}",
            frame.footer_text
        );
    }
}

#[test]
fn shifted_wheel_horizontal_scroll_does_not_reattach_follow() {
    let (_handle, timeline) = fake_timeline((0..20).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(32, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    viewer.render(size, None).unwrap();
    viewer.handle_event(key(KeyCode::Up), FileViewer::page_for_size(size));
    viewer.handle_event(key(KeyCode::Char('w')), FileViewer::page_for_size(size));
    assert!(
        viewer
            .render(size, None)
            .unwrap()
            .footer_text
            .contains("follow:detached")
    );

    viewer.handle_event(
        InputEvent::Mouse {
            kind: MouseEventKind::ScrollDown,
            modifiers: KeyModifiers::SHIFT,
        },
        FileViewer::page_for_size(size),
    );
    let frame = viewer.render(size, None).unwrap();

    assert!(
        frame.footer_text.contains("follow:detached"),
        "{}",
        frame.footer_text
    );
}

#[test]
fn detached_follow_reattaches_only_after_scrolling_back_to_bottom() {
    let (handle, timeline) = fake_timeline((0..10).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    viewer.render(size, None).unwrap();

    viewer.handle_event(key(KeyCode::PageUp), FileViewer::page_for_size(size));
    let detached = viewer.render(size, None).unwrap();
    assert!(detached.footer_text.contains("follow:detached"));
    handle.append(record(10));
    viewer.preload().unwrap();
    let stayed = viewer.render(size, None).unwrap();
    assert!(stayed.footer_text.contains("follow:detached"));

    let mut reattached = viewer.render(size, None).unwrap();
    for _ in 0..16 {
        viewer.handle_event(key(KeyCode::PageDown), FileViewer::page_for_size(size));
        reattached = viewer.render(size, None).unwrap();
        if reattached.footer_text.contains("follow:on") {
            break;
        }
    }
    assert!(
        reattached.footer_text.contains("follow:on"),
        "title={} footer={} position={:?}",
        reattached.title,
        reattached.footer_text,
        reattached.position
    );
    assert!(frame_text(reattached).contains("record-10"));

    handle.append(record(11));
    viewer.preload().unwrap();
    assert!(frame_text(viewer.render(size, None).unwrap()).contains("record-11"));
}

#[test]
fn prepending_older_records_preserves_the_viewport_anchor() {
    let (_handle, timeline) = fake_timeline((0..100).map(record));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(2, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 8);
    let before = viewer.render(size, None).unwrap();
    let before_top = before.position.top;
    let before_text = frame_text(before);
    assert!(before_text.contains("record-99"), "{before_text:?}");

    assert!(viewer.preload().unwrap());
    let after = viewer.render(size, None).unwrap();
    assert!(after.position.top > before_top);
    assert!(frame_text(after).contains("record-99"));
}

#[test]
fn replacement_tail_overlap_is_not_duplicated() {
    let (handle, timeline) = fake_timeline([record(1), record(2)]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();

    handle.replace([record(1), record(2), record(3)]);
    let change = file.refresh_records(16, 4096).unwrap();
    assert!(change.reset);
    let text = file.read_window(0, file.line_count()).unwrap().join("\n");
    assert_eq!(text.matches("record-1").count(), 1);
    assert_eq!(text.matches("record-2").count(), 1);
    assert_eq!(text.matches("record-3").count(), 1);
}

#[test]
fn legitimate_adjacent_duplicate_records_remain_visible() {
    let duplicate = b"{\"message\":\"same\"}\n".to_vec();
    let (handle, timeline) = fake_timeline([duplicate.clone(), duplicate.clone()]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    handle.append(duplicate.clone());
    file.refresh_records(16, 4096).unwrap();
    let text = file.read_window(0, file.line_count()).unwrap().join("\n");
    assert_eq!(text.matches("\"same\"").count(), 3);

    handle.replace([
        duplicate.clone(),
        duplicate,
        b"{\"message\":\"after-reset\"}\n".to_vec(),
    ]);
    file.refresh_records(16, 4096).unwrap();
    let reset_text = file.read_window(0, file.line_count()).unwrap().join("\n");
    assert_eq!(reset_text.matches("\"same\"").count(), 3);
    assert_eq!(reset_text.matches("after-reset").count(), 1);
}

#[test]
fn reset_overlap_compares_a_huge_record_from_the_disk_spool() {
    let large = format!("{{\"payload\":\"{}\"}}\n", "x".repeat(5 * 1024 * 1024)).into_bytes();
    let (handle, timeline) = fake_timeline([large.clone()]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(2, 8 * 1024 * 1024),
    )
    .unwrap();
    assert_eq!(file.line_count(), 3);

    handle.replace([large, b"{\"message\":\"after-reset\"}\n".to_vec()]);
    let change = file.refresh_records(2, 8 * 1024 * 1024).unwrap();
    assert!(change.reset);
    assert_eq!(file.line_count(), 6);
}

#[test]
fn older_loads_after_reset_stay_at_the_new_epoch_boundary() {
    let (handle, timeline) = fake_timeline([record(1), record(2)]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();
    handle.replace([
        b"{\"message\":\"new-0\"}\n".to_vec(),
        b"{\"message\":\"new-1\"}\n".to_vec(),
        b"{\"message\":\"new-2\"}\n".to_vec(),
    ]);
    file.refresh_records(2, 4096).unwrap();
    file.load_older_records(2, 4096).unwrap();
    let text = file.read_window(0, file.line_count()).unwrap().join("\n");
    let old = text.find("record-2").unwrap();
    let new_0 = text.find("new-0").unwrap();
    let new_1 = text.find("new-1").unwrap();
    assert!(old < new_0 && new_0 < new_1, "{text}");
}

#[test]
fn search_reaches_lazily_loaded_older_and_newer_records() {
    let (handle, timeline) = fake_timeline((0..100).map(|index| {
        if index == 0 {
            b"{\"message\":\"needle-old\"}\n".to_vec()
        } else {
            record(index)
        }
    }));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(2, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 10);
    viewer.render(size, None).unwrap();

    enter_search(&mut viewer, size, "needle-old");
    advance_until_idle(&mut viewer);
    let older = viewer.render(size, None).unwrap();
    assert!(frame_text(older).contains("needle-old"));
    viewer.handle_event(key(KeyCode::Esc), FileViewer::page_for_size(size));
    let detached = viewer.render(size, None).unwrap();
    assert!(detached.footer_text.contains("follow:detached"));
    let detached_top = detached.position.top;

    handle.append(b"{\"message\":\"needle-new\"}\n".to_vec());
    viewer.preload().unwrap();
    assert_eq!(
        viewer.render(size, None).unwrap().position.top,
        detached_top
    );
    enter_search(&mut viewer, size, "needle-new");
    advance_until_idle(&mut viewer);
    let newer = viewer.render(size, None).unwrap();
    assert!(frame_text(newer).contains("needle-new"));
}

#[test]
fn repeated_forward_search_wraps_into_lazily_loaded_older_records() {
    let (_handle, timeline) = fake_timeline((0..100).map(|index| match index {
        0 => b"{\"message\":\"needle-old\"}\n".to_vec(),
        99 => b"{\"message\":\"needle-tail\"}\n".to_vec(),
        _ => record(index),
    }));
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(2, 4096),
    )
    .unwrap();
    let mut viewer = FileViewer::new(Box::new(file), FormatKind::Jsonl, None);
    let size = Size::new(60, 10);
    viewer.render(size, None).unwrap();

    enter_search(&mut viewer, size, "needle");
    advance_until_idle(&mut viewer);
    assert!(frame_text(viewer.render(size, None).unwrap()).contains("needle-tail"));

    viewer.handle_event(key(KeyCode::Char('n')), FileViewer::page_for_size(size));
    advance_until_idle(&mut viewer);
    let wrapped = frame_text(viewer.render(size, None).unwrap());
    assert!(wrapped.contains("needle-old"), "{wrapped}");
}

#[test]
fn file_timeline_preserves_crlf_empty_records_and_ignores_incomplete_eof() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"a\":1}\r\n\r\n{\"b\":2}\n{\"pending\":true}")
        .unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "edge.jsonl").unwrap();

    let TimelineRead::Records(records) = timeline
        .load_older(RecordLoadLimit::new(16, 1024 * 1024))
        .unwrap()
    else {
        panic!("expected committed tail records");
    };
    assert_eq!(
        records
            .iter()
            .map(|record| record.raw.as_slice())
            .collect::<Vec<_>>(),
        vec![b"{\"a\":1}\r\n".as_slice(), b"\r\n", b"{\"b\":2}\n"]
    );
    assert_eq!(timeline.snapshot().pending_bytes, 16);

    temp.write_all(b"\n").unwrap();
    temp.flush().unwrap();
    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Appended(_)
    ));
    let TimelineRead::Records(appended) = timeline
        .load_newer(RecordLoadLimit::new(16, 1024 * 1024))
        .unwrap()
    else {
        panic!("expected committed appended record");
    };
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].raw, b"{\"pending\":true}\n");
    assert_eq!(
        timeline
            .load_newer(RecordLoadLimit::new(16, 1024 * 1024))
            .unwrap(),
        TimelineRead::Pending
    );
}

#[test]
fn file_timeline_reverse_scan_handles_a_very_large_single_record() {
    let mut temp = NamedTempFile::new().unwrap();
    let payload = "x".repeat(512 * 1024);
    writeln!(temp, "{{\"payload\":\"{payload}\"}}").unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "large-record.jsonl").unwrap();

    let TimelineRead::Records(records) = timeline
        .load_older(RecordLoadLimit::new(8, 64 * 1024))
        .unwrap()
    else {
        panic!("expected the large committed record");
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].raw.len(), payload.len() + 15);
    assert!(timeline.instrumentation().read_operations > 4);
}

#[test]
fn million_record_tail_open_has_bounded_instrumented_work() {
    let mut temp = NamedTempFile::new().unwrap();
    for index in 0..1_000_000_u32 {
        writeln!(temp, "{{\"index\":{index}}}").unwrap();
    }
    temp.flush().unwrap();
    let file_bytes = temp.as_file().metadata().unwrap().len();
    let mut timeline = FileRecordTimeline::open(temp.path(), "million.jsonl").unwrap();
    let TimelineRead::Records(records) = timeline
        .load_older(RecordLoadLimit::new(128, 4 * 1024 * 1024))
        .unwrap()
    else {
        panic!("expected tail records");
    };

    let stats = timeline.instrumentation();
    assert_eq!(records.len(), 128);
    assert!(records[0].raw.starts_with(b"{\"index\":999872}"));
    assert!(
        stats.bytes_read < 256 * 1024,
        "unexpected tail work: {stats:?}"
    );
    assert!(stats.bytes_read < file_bytes / 50);
}

#[test]
fn large_append_refresh_finds_the_tail_with_bounded_reverse_work() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"index\":0}\n").unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "burst.jsonl").unwrap();
    let before = timeline.instrumentation();

    for index in 1..=200_000_u32 {
        writeln!(temp, "{{\"index\":{index}}}").unwrap();
    }
    temp.flush().unwrap();
    let appended_bytes = temp
        .as_file()
        .metadata()
        .unwrap()
        .len()
        .saturating_sub(timeline.snapshot().observed_end);

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Appended(_)
    ));
    let after_refresh = timeline.instrumentation();
    let refresh_bytes = after_refresh.bytes_read - before.bytes_read;
    assert!(appended_bytes > 2 * 1024 * 1024);
    assert!(
        refresh_bytes < 128 * 1024,
        "refresh read {refresh_bytes} bytes for a {appended_bytes}-byte burst"
    );

    let TimelineRead::Records(records) =
        timeline.load_newer(RecordLoadLimit::new(4, 4096)).unwrap()
    else {
        panic!("expected bounded newer records");
    };
    assert_eq!(records.len(), 4);
    assert_eq!(records[0].raw, b"{\"index\":1}\n");
    assert!(timeline.instrumentation().bytes_read - before.bytes_read < 256 * 1024);
}

#[test]
fn append_burst_drains_across_bounded_refresh_batches_without_duplicates() {
    let (handle, timeline) = fake_timeline([record(0)]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(1, 4096),
    )
    .unwrap();
    for index in 1..=10 {
        handle.append(record(index));
    }

    let mut appended_lines = 0;
    for _ in 0..4 {
        appended_lines += file.refresh_records(3, 4096).unwrap().appended_lines;
    }
    assert_eq!(appended_lines, 30);
    assert!(file.at_newer_boundary());
    let text = file.read_window(0, file.line_count()).unwrap().join("\n");
    for index in 0..=10 {
        assert_eq!(text.matches(&format!("record-{index}\"")).count(), 1);
    }
}

#[test]
fn unchanged_large_pending_suffix_is_not_rescanned_on_each_refresh() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"id\":1}\n").unwrap();
    temp.write_all(&vec![b'x'; 4 * 1024 * 1024]).unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "pending.jsonl").unwrap();

    for _ in 0..2 {
        let before = timeline.instrumentation();
        assert!(matches!(
            timeline.refresh().unwrap(),
            TimelineRefresh::Pending(_)
        ));
        let bytes = timeline.instrumentation().bytes_read - before.bytes_read;
        assert!(bytes < 1024, "unchanged pending refresh read {bytes} bytes");
    }
}

#[test]
fn same_size_uncommitted_tail_rewrite_uses_change_signals() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"id\":1}\nxxxxxxxxx").unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "pending-rewrite.jsonl").unwrap();
    let committed_end = timeline.snapshot().committed_end;

    let mut writer = OpenOptions::new().write(true).open(temp.path()).unwrap();
    writer.seek(SeekFrom::Start(committed_end)).unwrap();
    writer.write_all(b"aa\nbbbbbb").unwrap();
    writer.flush().unwrap();

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Appended(_)
    ));
    let TimelineRead::Records(records) =
        timeline.load_newer(RecordLoadLimit::new(8, 4096)).unwrap()
    else {
        panic!("expected newly committed rewritten tail record");
    };
    assert_eq!(records[0].raw, b"aa\n");
}

#[test]
fn truncating_only_an_uncommitted_tail_does_not_duplicate_committed_records() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"id\":1}\n{\"partial\":").unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "rewrite.jsonl").unwrap();
    let TimelineRead::Records(initial) =
        timeline.load_older(RecordLoadLimit::new(8, 4096)).unwrap()
    else {
        panic!("expected initial record");
    };
    assert_eq!(initial.len(), 1);

    let committed_end = timeline.snapshot().committed_end;
    let mut writer = OpenOptions::new().write(true).open(temp.path()).unwrap();
    writer.set_len(committed_end).unwrap();
    writer.seek(SeekFrom::Start(committed_end)).unwrap();
    writer.write_all(b"{\"id\":2}\n").unwrap();
    writer.flush().unwrap();

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Appended(_)
    ));
    let TimelineRead::Records(appended) =
        timeline.load_newer(RecordLoadLimit::new(8, 4096)).unwrap()
    else {
        panic!("expected rewritten committed record");
    };
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].raw, b"{\"id\":2}\n");
}

#[test]
fn truncating_committed_history_starts_a_new_epoch() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n")
        .unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "truncate.jsonl").unwrap();

    let mut writer = OpenOptions::new().write(true).open(temp.path()).unwrap();
    writer.set_len(0).unwrap();
    writer.write_all(b"{\"id\":4}\n").unwrap();
    writer.flush().unwrap();

    let TimelineRefresh::Reset { reason, snapshot } = timeline.refresh().unwrap() else {
        panic!("expected truncation reset");
    };
    assert_eq!(reason, TimelineResetReason::Truncated);
    assert_eq!(snapshot.epoch, 2);
}

#[test]
fn same_inode_same_size_copytruncate_rewrite_resets_on_sample_mismatch() {
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(b"{\"id\":1}\n").unwrap();
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "copytruncate.jsonl").unwrap();

    let mut writer = OpenOptions::new().write(true).open(temp.path()).unwrap();
    writer.set_len(0).unwrap();
    writer.write_all(b"{\"id\":2}\n").unwrap();
    writer.flush().unwrap();

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Reset {
            reason: TimelineResetReason::Replaced,
            ..
        }
    ));
}

#[test]
fn same_inode_same_size_rewrite_outside_the_tail_sample_resets() {
    let mut temp = NamedTempFile::new().unwrap();
    for index in 0..16 {
        writeln!(
            temp,
            "{{\"id\":{index:02},\"payload\":\"unchanged-width-value\"}}"
        )
        .unwrap();
    }
    temp.flush().unwrap();
    let mut timeline = FileRecordTimeline::open(temp.path(), "same-size-rewrite.jsonl").unwrap();
    let original = fs::read(temp.path()).unwrap();

    let mut rewritten = original.clone();
    let old = b"\"id\":00";
    let new = b"\"id\":99";
    let offset = rewritten
        .windows(old.len())
        .position(|window| window == old)
        .unwrap();
    rewritten[offset..offset + old.len()].copy_from_slice(new);
    assert_eq!(rewritten.len(), original.len());
    assert_eq!(
        &rewritten[rewritten.len() - 64..],
        &original[original.len() - 64..]
    );
    fs::write(temp.path(), &rewritten).unwrap();

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Reset {
            reason: TimelineResetReason::Replaced,
            ..
        }
    ));
}

#[test]
fn malformed_initial_tail_record_surfaces_a_notice() {
    let (_handle, timeline) = fake_timeline([b"not-json\n".to_vec(), b"still-not-json\n".to_vec()]);
    let file = RecordTimelineViewFile::with_initial_limit(
        Box::new(timeline),
        JSONL,
        RecordLoadLimit::new(16, 4096),
    )
    .unwrap();

    let notice = file.take_notice().unwrap();
    assert!(notice.contains("failed JSON parse"), "{notice}");
    assert!(notice.contains("and 1 more record"), "{notice}");
    assert!(file.take_notice().is_none());
    assert_eq!(
        file.read_window(0, 2).unwrap(),
        vec!["not-json", "still-not-json"]
    );
}

#[test]
fn inode_replacement_resets_with_identity_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let mut timeline = FileRecordTimeline::open(&path, "events.jsonl").unwrap();
    let rotated = directory.path().join("events.jsonl.1");
    fs::rename(&path, rotated).unwrap();
    fs::write(&path, b"{\"id\":2}\n").unwrap();

    assert!(matches!(
        timeline.refresh().unwrap(),
        TimelineRefresh::Reset {
            reason: TimelineResetReason::IdentityChanged,
            ..
        }
    ));
}

fn record(index: usize) -> Vec<u8> {
    format!("{{\"message\":\"record-{index}\"}}\n").into_bytes()
}

fn key(code: KeyCode) -> InputEvent {
    InputEvent::Key {
        code,
        modifiers: KeyModifiers::NONE,
    }
}

fn enter_search(viewer: &mut FileViewer, size: Size, query: &str) {
    viewer.handle_event(key(KeyCode::Char('/')), FileViewer::page_for_size(size));
    for ch in query.chars() {
        viewer.handle_event(key(KeyCode::Char(ch)), FileViewer::page_for_size(size));
    }
    viewer.handle_event(key(KeyCode::Enter), FileViewer::page_for_size(size));
}

fn advance_until_idle(viewer: &mut FileViewer) {
    for _ in 0..64 {
        let changed = viewer.advance(Instant::now()).unwrap();
        if !viewer.needs_immediate_advance() && !changed {
            return;
        }
    }
    panic!("viewer did not settle");
}

fn frame_text(frame: fmtview_core::RenderFrame) -> String {
    let mut buffer = Buffer::empty(Rect::new(0, 0, frame.area.width, frame.area.height));
    render_frame_to_buffer(&mut buffer, frame);
    buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[derive(Clone)]
struct FakeHandle {
    state: Rc<RefCell<FakeState>>,
}

impl FakeHandle {
    fn append(&self, record: Vec<u8>) {
        self.state.borrow_mut().records.push(record);
    }

    fn replace(&self, records: impl IntoIterator<Item = Vec<u8>>) {
        let mut state = self.state.borrow_mut();
        state.records = records.into_iter().collect();
        state.generation = state.generation.saturating_add(1);
    }
}

struct FakeState {
    records: Vec<Vec<u8>>,
    generation: u64,
    terminal: bool,
}

struct FakeTimeline {
    state: Rc<RefCell<FakeState>>,
    generation: u64,
    older_cursor: usize,
    newer_cursor: usize,
}

fn fake_timeline(records: impl IntoIterator<Item = Vec<u8>>) -> (FakeHandle, FakeTimeline) {
    let state = Rc::new(RefCell::new(FakeState {
        records: records.into_iter().collect(),
        generation: 1,
        terminal: false,
    }));
    let len = state.borrow().records.len();
    (
        FakeHandle {
            state: Rc::clone(&state),
        },
        FakeTimeline {
            state,
            generation: 1,
            older_cursor: len,
            newer_cursor: len,
        },
    )
}

impl RecordTimeline for FakeTimeline {
    fn label(&self) -> &str {
        "<fake>"
    }

    fn snapshot(&self) -> TimelineSnapshot {
        let state = self.state.borrow();
        TimelineSnapshot {
            epoch: self.generation,
            committed_end: state.records.len() as u64,
            observed_end: state.records.len() as u64,
            pending_bytes: 0,
        }
    }

    fn load_older(&mut self, limit: RecordLoadLimit) -> anyhow::Result<TimelineRead> {
        if self.older_cursor == 0 {
            return Ok(TimelineRead::End);
        }
        let start = self.older_cursor.saturating_sub(limit.max_records.max(1));
        let state = self.state.borrow();
        let records = state.records[start..self.older_cursor]
            .iter()
            .enumerate()
            .map(|(offset, raw)| TimelineRecord {
                id: RecordId {
                    epoch: self.generation,
                    start_offset: (start + offset) as u64,
                    end_offset: (start + offset + 1) as u64,
                },
                raw: raw.clone(),
            })
            .collect();
        self.older_cursor = start;
        Ok(TimelineRead::Records(records))
    }

    fn load_newer(&mut self, limit: RecordLoadLimit) -> anyhow::Result<TimelineRead> {
        let state = self.state.borrow();
        if self.newer_cursor >= state.records.len() {
            return Ok(if state.terminal {
                TimelineRead::End
            } else {
                TimelineRead::Pending
            });
        }
        let end = self
            .newer_cursor
            .saturating_add(limit.max_records.max(1))
            .min(state.records.len());
        let records = state.records[self.newer_cursor..end]
            .iter()
            .enumerate()
            .map(|(offset, raw)| TimelineRecord {
                id: RecordId {
                    epoch: self.generation,
                    start_offset: (self.newer_cursor + offset) as u64,
                    end_offset: (self.newer_cursor + offset + 1) as u64,
                },
                raw: raw.clone(),
            })
            .collect();
        self.newer_cursor = end;
        Ok(TimelineRead::Records(records))
    }

    fn refresh(&mut self) -> anyhow::Result<TimelineRefresh> {
        let state = self.state.borrow();
        if state.generation != self.generation {
            self.generation = state.generation;
            self.older_cursor = state.records.len();
            self.newer_cursor = state.records.len();
            return Ok(TimelineRefresh::Reset {
                reason: TimelineResetReason::Replaced,
                snapshot: self.snapshot(),
            });
        }
        let snapshot = self.snapshot();
        if self.newer_cursor < state.records.len() {
            Ok(TimelineRefresh::Appended(snapshot))
        } else if state.terminal {
            Ok(TimelineRefresh::End(snapshot))
        } else {
            Ok(TimelineRefresh::NoChange(snapshot))
        }
    }
}
