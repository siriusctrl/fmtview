use super::*;

fn marks(lines: &[&str]) -> Vec<ToolLineMark> {
    let lines = lines
        .iter()
        .map(|line| (*line).to_owned())
        .collect::<Vec<_>>();
    ToolLinkTracker::default().mark_lines(&lines, 0)
}

#[test]
fn links_openai_tool_call_to_later_result() {
    let marks = marks(&[
        r#"{"role":"assistant","tool_calls":[{"id":"call_7","type":"function"}]}"#,
        r#"{"role":"tool","tool_call_id":"call_7","content":"ok"}"#,
    ]);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].relation, ToolRelationMark::MatchedResult);
    let link = marks[1].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "call_7");
    assert_eq!(link.call_line, Some(0));
    assert_eq!(link.result_line, 1);
}

#[test]
fn links_tool_use_by_shared_id() {
    let marks = marks(&[
        r#"{"role":"assistant","content":[{"type":"tool_use","id":"use_9","name":"lookup"}]}"#,
        r#"{"role":"tool","tool_use_id":"use_9","content":"ok"}"#,
    ]);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].link.as_ref().unwrap().call_line, Some(0));
}

#[test]
fn links_nested_typed_tool_call_to_nested_typed_result() {
    let marks = marks(&[
        r#"{"ref":"m2","role":"assistant","content":[{"type":"tool_call","id":"call_1","name":"shell","arguments":"{\"cmd\":\"cargo test\"}"}]}"#,
        r#"{"ref":"m3","role":"tool","content":[{"type":"tool_result","call_id":"call_1","content":"ok"}]}"#,
    ]);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].relation, ToolRelationMark::MatchedResult);
    let link = marks[1].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "call_1");
    assert_eq!(link.call_line, Some(0));
    assert_eq!(link.result_line, 1);
}

#[test]
fn supports_custom_shared_id_field_without_matching_unrelated_objects() {
    let marks = marks(&[
        r#"{"request_id":"outside"}"#,
        r#"{"tool_calls":[{"request_id":"req_2","name":"lookup"}]}"#,
        r#"{"role":"tool","request_id":"req_2","content":"ok"}"#,
    ]);

    assert_eq!(marks[0], ToolLineMark::default());
    assert_eq!(marks[1].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[2].link.as_ref().unwrap().call_line, Some(1));
}

#[test]
fn custom_shared_field_matches_when_the_call_also_has_a_primary_id() {
    let marks = marks(&[
        r#"{"tool_calls":[{"id":"call_primary","request_id":"req_2"}]}"#,
        r#"{"role":"tool","request_id":"req_2","content":"ok"}"#,
    ]);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].link.as_ref().unwrap().call_line, Some(0));
    assert_eq!(marks[1].link.as_ref().unwrap().id.as_ref(), "req_2");
}

#[test]
fn more_specific_result_id_wins_regardless_of_property_order() {
    let marks = marks(&[
        r#"{"tool_calls":[{"id":"generic_match"}]}"#,
        r#"{"tool_calls":[{"id":"specific_match"}]}"#,
        r#"{"role":"tool","id":"generic_match","tool_call_id":"specific_match"}"#,
    ]);

    let link = marks[2].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "specific_match");
    assert_eq!(link.call_line, Some(1));
    assert_eq!(link.status, ToolLinkStatus::Matched);
}

#[test]
fn lower_ranked_unique_id_can_disambiguate_duplicate_primary_candidates() {
    let marks = marks(&[
        r#"{"tool_calls":[{"id":"same","request_id":"left"}]}"#,
        r#"{"tool_calls":[{"id":"same","request_id":"right"}]}"#,
        r#"{"role":"tool","tool_call_id":"same","request_id":"right"}"#,
    ]);

    let link = marks[2].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "right");
    assert_eq!(link.call_line, Some(1));
    assert_eq!(link.status, ToolLinkStatus::Matched);
}

#[test]
fn conflicting_lower_ranked_id_cannot_escape_an_ambiguous_primary_set() {
    let marks = marks(&[
        r#"{"tool_calls":[{"id":"same","request_id":"left"}]}"#,
        r#"{"tool_calls":[{"id":"same","request_id":"right"}]}"#,
        r#"{"tool_calls":[{"id":"outside","request_id":"outside"}]}"#,
        r#"{"role":"tool","tool_call_id":"same","request_id":"outside"}"#,
    ]);

    let link = marks[3].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "same");
    assert_eq!(link.call_line, None);
    assert_eq!(link.status, ToolLinkStatus::Ambiguous);
}

#[test]
fn escaped_ids_match_decoded_values() {
    let marks = marks(&[
        r#"{"tool_calls":[{"id":"call_7_\ud83d\ude80"}]}"#,
        r#"{"role":"tool","tool_call_id":"call_\u0037_🚀"}"#,
    ]);

    let link = marks[1].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "call_7_🚀");
    assert_eq!(link.call_line, Some(0));
}

#[test]
fn id_candidates_are_capped_and_keep_specific_fields() {
    let custom_ids = (0..MAX_IDS_PER_CONTAINER + 4)
        .map(|index| format!(r#""custom_{index}_id":"value_{index}""#))
        .collect::<Vec<_>>()
        .join(",");
    let call = format!(r#"{{"tool_calls":[{{{custom_ids},"tool_call_id":"specific"}}]}}"#);
    let mut tracker = ToolLinkTracker::default();
    tracker.apply_line(&call, 0);

    assert_eq!(tracker.pending_calls[0].ids.len(), MAX_IDS_PER_CONTAINER);
    assert!(
        tracker.pending_calls[0]
            .ids
            .iter()
            .any(|candidate| candidate.key == "tool_call_id")
    );
}

#[test]
fn candidate_cap_keeps_a_tool_calls_canonical_plain_id() {
    let custom_ids = (0..MAX_IDS_PER_CONTAINER + 4)
        .map(|index| format!(r#""custom_{index}_id":"value_{index}""#))
        .collect::<Vec<_>>()
        .join(",");
    let call = format!(r#"{{"tool_calls":[{{"id":"canonical",{custom_ids}}}]}}"#);
    let result = r#"{"role":"tool","tool_call_id":"canonical"}"#.to_owned();
    let mut tracker = ToolLinkTracker::default();
    let lines = vec![call, result];

    let marks = tracker.mark_lines(&lines, 0);

    assert_eq!(marks[1].link.as_ref().unwrap().call_line, Some(0));
}

#[test]
fn candidate_cap_keeps_late_plain_and_canonical_ids() {
    let generic_ids = (0..MAX_IDS_PER_CONTAINER)
        .map(|index| format!(r#""tool_custom_{index}_id":"value_{index}""#))
        .collect::<Vec<_>>()
        .join(",");
    let call =
        format!(r#"{{"tool_calls":[{{{generic_ids},"id":"plain","tool_call_id":"canonical"}}]}}"#);
    let mut tracker = ToolLinkTracker::default();
    tracker.apply_line(&call, 0);

    let ids = &tracker.pending_calls[0].ids;
    assert_eq!(ids.len(), MAX_IDS_PER_CONTAINER);
    assert!(
        ids.iter()
            .any(|candidate| candidate.key == "id" && candidate.value.as_ref() == "plain")
    );
    assert!(ids.iter().any(|candidate| {
        candidate.key == "tool_call_id" && candidate.value.as_ref() == "canonical"
    }));
}

#[test]
fn unmatched_generic_tool_id_does_not_hide_a_shared_custom_id() {
    let marks = marks(&[
        r#"{"tool_calls":[{"request_id":"req_2"}]}"#,
        r#"{"role":"tool","parent_tool_id":"parent","request_id":"req_2"}"#,
    ]);

    let link = marks[1].link.as_ref().unwrap();
    assert_eq!(link.id.as_ref(), "req_2");
    assert_eq!(link.call_line, Some(0));
    assert_eq!(link.status, ToolLinkStatus::Matched);
}

#[test]
fn unmatched_and_ambiguous_results_keep_direction_marker_neutral() {
    let unmatched = marks(&[r#"{"role":"tool","tool_call_id":"missing"}"#]);
    assert_eq!(unmatched[0].relation, ToolRelationMark::None);
    assert_eq!(
        unmatched[0].link.as_ref().unwrap().status,
        ToolLinkStatus::Unmatched
    );

    let ambiguous = marks(&[
        r#"{"tool_calls":[{"id":"same"},{"id":"same"}]}"#,
        r#"{"role":"tool","tool_call_id":"same"}"#,
    ]);
    assert_eq!(ambiguous[1].relation, ToolRelationMark::None);
    assert_eq!(
        ambiguous[1].link.as_ref().unwrap().status,
        ToolLinkStatus::Ambiguous
    );
}

#[test]
fn result_context_survives_a_window_starting_inside_the_object() {
    let prefix = [
        r#"{"tool_calls":[{"id":"call_1"}]}"#.to_owned(),
        "{".to_owned(),
        r#"  "role": "tool","#.to_owned(),
        r#"  "tool_call_id": "call_1","#.to_owned(),
    ];
    let mut tracker = ToolLinkTracker::default();
    for (line, text) in prefix.iter().enumerate() {
        tracker.apply_line(text, line);
    }
    let visible = vec![r#"  "content": "ok""#.to_owned(), "}".to_owned()];

    let marks = tracker.mark_lines(&visible, 4);

    assert!(marks.iter().all(|mark| mark.link.is_some()));
    assert_eq!(marks[0].link.as_ref().unwrap().call_line, Some(0));
}

#[test]
fn provisional_result_context_survives_without_a_visible_closing_brace() {
    let prefix = [
        r#"{"tool_calls":[{"id":"call_1"}]}"#.to_owned(),
        "{".to_owned(),
        r#"  "role": "tool","#.to_owned(),
        r#"  "tool_call_id": "call_1","#.to_owned(),
    ];
    let mut tracker = ToolLinkTracker::default();
    for (line, text) in prefix.iter().enumerate() {
        tracker.apply_line(text, line);
    }
    let visible = (0..100)
        .map(|index| format!(r#"  "field_{index}": "value","#))
        .collect::<Vec<_>>();

    let marks = tracker.mark_lines(&visible, 4);

    assert!(marks.iter().all(|mark| mark.link.is_some()));
    assert!(
        marks
            .iter()
            .all(|mark| mark.link.as_ref().unwrap().call_line == Some(0))
    );
}

#[test]
fn provisional_result_context_matches_a_plain_id() {
    let prefix = [
        r#"{"tool_calls":[{"id":"call_1"}]}"#.to_owned(),
        "{".to_owned(),
        r#"  "role": "tool","#.to_owned(),
        r#"  "id": "call_1","#.to_owned(),
    ];
    let mut tracker = ToolLinkTracker::default();
    for (line, text) in prefix.iter().enumerate() {
        tracker.apply_line(text, line);
    }
    let visible = vec![r#"  "content": "still open""#.to_owned()];

    let marks = tracker.mark_lines(&visible, 4);

    assert_eq!(marks[0].link.as_ref().unwrap().call_line, Some(0));
    assert_eq!(marks[0].link.as_ref().unwrap().id.as_ref(), "call_1");
}

#[test]
fn nested_typed_result_suppresses_a_role_tool_envelope_plain_id() {
    let lines = [
        r#"{"type":"tool_call","id":"c1"}"#,
        r#"{"type":"tool_call","id":"m3"}"#,
        "{",
        r#"  "id": "m3","#,
        r#"  "role": "tool","#,
        r#"  "content": ["#,
        "    {",
        r#"      "type": "tool_result","#,
        r#"      "call_id": "c1","#,
        r#"      "content": "ok""#,
        "    }",
        "  ]",
        "}",
    ];
    let owned = lines.map(str::to_owned);
    let mut tracker = ToolLinkTracker::default();

    let marks = tracker.mark_lines(&owned, 0);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].relation, ToolRelationMark::None);
    assert!(marks[2].link.is_none(), "envelope must not retain a link");
    assert_eq!(marks[6].relation, ToolRelationMark::MatchedResult);
    assert_eq!(marks[6].link.as_ref().unwrap().call_line, Some(0));
    assert_eq!(tracker.pending_calls.len(), 1);
    assert_eq!(tracker.pending_calls[0].line, 1);
}

#[test]
fn typed_child_in_lookahead_retracts_visible_envelope_provisional_link() {
    let prefix = [
        r#"{"type":"tool_call","id":"c1"}"#,
        r#"{"type":"tool_call","id":"m3"}"#,
    ];
    let mut tracker = ToolLinkTracker::default();
    for (line, text) in prefix.iter().enumerate() {
        tracker.apply_line(text, line);
    }
    let visible = [
        "{".to_owned(),
        r#"  "id": "m3","#.to_owned(),
        r#"  "role": "tool","#.to_owned(),
        r#"  "content": ["#.to_owned(),
    ];
    let lookahead = [
        "    {".to_owned(),
        r#"      "type": "tool_result","#.to_owned(),
        r#"      "call_id": "c1""#.to_owned(),
        "    }".to_owned(),
        "  ]".to_owned(),
        "}".to_owned(),
    ];

    let marks = tracker.mark_lines_with_lookahead(&visible, &lookahead, prefix.len());

    assert!(marks.iter().all(|mark| mark.link.is_none()));
}

#[test]
fn explicit_envelope_call_id_is_a_fallback_for_an_unmatched_typed_child() {
    let lines = [
        r#"{"type":"tool_call","id":"c1"}"#,
        "{",
        r#"  "role": "tool","#,
        r#"  "tool_call_id": "c1","#,
        r#"  "content": ["#,
        "    {",
        r#"      "type": "tool_result","#,
        r#"      "id": "result-123","#,
        r#"      "content": "ok""#,
        "    }",
        "  ]",
        "}",
    ];

    let marks = marks(&lines);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].relation, ToolRelationMark::MatchedResult);
    assert!(marks[5..10].iter().all(|mark| {
        mark.link
            .as_ref()
            .is_none_or(|link| link.status == ToolLinkStatus::Matched)
    }));
}

#[test]
fn authoritative_unmatched_child_id_does_not_fall_back_to_the_envelope() {
    let lines = [
        r#"{"type":"tool_call","id":"c1"}"#,
        "{",
        r#"  "role": "tool","#,
        r#"  "tool_call_id": "c1","#,
        r#"  "content": ["#,
        r#"    {"type":"tool_result","call_id":"bad"}"#,
        "  ]",
        "}",
    ];
    let owned = lines.map(str::to_owned);
    let mut tracker = ToolLinkTracker::default();

    let marks = tracker.mark_lines(&owned, 0);

    assert_eq!(marks[0].relation, ToolRelationMark::None);
    assert!(marks[1].link.is_none());
    assert_eq!(marks[5].link.as_ref().unwrap().id.as_ref(), "bad");
    assert_eq!(
        marks[5].link.as_ref().unwrap().status,
        ToolLinkStatus::Unmatched
    );
    assert_eq!(tracker.pending_calls.len(), 1);
}

#[test]
fn ordinary_intermediate_toolish_ids_are_not_envelope_fallbacks() {
    let lines = [
        r#"{"type":"tool_call","id":"message-7"}"#,
        "{",
        r#"  "id": "message-7","#,
        r#"  "role": "tool","#,
        r#"  "content": [{"#,
        r#"    "call_id": "message-7","#,
        r#"    "result": {"type":"tool_result","id":"result-123"}"#,
        "  }]",
        "}",
    ];
    let owned = lines.map(str::to_owned);
    let mut tracker = ToolLinkTracker::default();

    let marks = tracker.mark_lines(&owned, 0);

    assert_eq!(marks[0].relation, ToolRelationMark::None);
    assert!(marks[1].link.is_none());
    assert_eq!(tracker.pending_calls.len(), 1);
}

#[test]
fn idless_typed_child_still_suppresses_an_envelope_plain_id() {
    let lines = [
        r#"{"type":"tool_call","id":"message-7"}"#,
        "{",
        r#"  "id": "message-7","#,
        r#"  "role": "tool","#,
        r#"  "content": [{"type":"tool_result","content":"ok"}]"#,
        "}",
    ];
    let owned = lines.map(str::to_owned);
    let mut tracker = ToolLinkTracker::default();

    let marks = tracker.mark_lines(&owned, 0);

    assert!(marks.iter().all(|mark| mark.link.is_none()));
    assert_eq!(tracker.pending_calls.len(), 1);
}

#[test]
fn multiple_typed_children_consume_only_their_exact_calls() {
    let lines = [
        r#"{"type":"tool_call","id":"c1"}"#,
        r#"{"type":"tool_call","id":"c2"}"#,
        r#"{"type":"tool_call","id":"message-7"}"#,
        "{",
        r#"  "id": "message-7","#,
        r#"  "role": "tool","#,
        r#"  "content": ["#,
        r#"    {"type":"tool_result","call_id":"c1"},"#,
        r#"    {"type":"tool_result","call_id":"c2"}"#,
        "  ]",
        "}",
    ];
    let owned = lines.map(str::to_owned);
    let mut tracker = ToolLinkTracker::default();

    let marks = tracker.mark_lines(&owned, 0);

    assert_eq!(marks[0].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[1].relation, ToolRelationMark::MatchedCall);
    assert_eq!(marks[2].relation, ToolRelationMark::None);
    assert_eq!(marks[7].relation, ToolRelationMark::MatchedResult);
    assert_eq!(marks[8].relation, ToolRelationMark::MatchedResult);
    assert!(marks[3].link.is_none());
    assert_eq!(tracker.pending_calls.len(), 1);
    assert_eq!(tracker.pending_calls[0].line, 2);
}
