use crate::formats::json::tool_links::ToolLinkStatus;
use crate::load::ViewFile;
use crate::tui::palette::{error_style, gutter_style, warning_style};

use super::super::input::{FollowState, FooterMessageKind, ViewState};
use super::{format_count, line_number_digits};
use ratatui::style::Style;

pub(in crate::viewer) fn file_title_text(
    file: &dyn ViewFile,
    state: &ViewState,
    current: usize,
    bottom: usize,
    progress: usize,
) -> String {
    format!(
        " {} | {} lines | {}-{} | {:>3}% | {} ",
        file.label(),
        line_count_text(file),
        current,
        bottom,
        progress,
        display_mode_text(state)
    )
}

pub(in crate::viewer) fn file_footer_text(file: &dyn ViewFile, state: &ViewState) -> String {
    let follow = follow_status(state);
    if state.search_active {
        format!(
            " {follow}search: {} | Enter find | Backspace edit | Esc cancel ",
            state.search_buffer
        )
    } else if !state.jump_buffer.is_empty() {
        format!(
            " {follow}go to line: {} / {} | Enter jump | Backspace edit | Esc cancel ",
            state.jump_buffer,
            line_count_text(file)
        )
    } else if let Some(message) = state.visible_footer_message() {
        format!(
            " {follow}{}{}{} | / search | n/N | Esc clear ",
            footer_message_label(message.kind),
            message.text,
            search_count_suffix(state)
        )
    } else if state.tool_context.is_some() {
        tool_context_footer_text(state)
    } else {
        idle_footer_text(state)
    }
}

pub(in crate::viewer) fn file_footer_style(state: &ViewState) -> Style {
    match state.visible_footer_message().map(|message| message.kind) {
        Some(FooterMessageKind::Error) => error_style(),
        Some(FooterMessageKind::Warning) => warning_style(),
        Some(FooterMessageKind::Info) => gutter_style(),
        None => match state.tool_context.as_ref().map(|link| link.status) {
            Some(ToolLinkStatus::Ambiguous) => error_style(),
            Some(ToolLinkStatus::Unmatched) => warning_style(),
            Some(ToolLinkStatus::Matched) | None => gutter_style(),
        },
    }
}

pub(in crate::viewer) fn idle_footer_text(state: &ViewState) -> String {
    if !state.mouse_capture {
        return " selection mode | m restore mouse ".to_owned();
    }

    let wrap_hint = if state.wrap { "w unwrap" } else { "w wrap" };
    let follow_hint = follow_hint(state);
    let page_hint = if state.follow.is_some() {
        "Space,b"
    } else {
        "Space/f,b"
    };
    let position = wrap_position_text(state)
        .map(|position| format!("{position} | "))
        .unwrap_or_default();
    if let Some(count) = search_count_text(state) {
        return format!(
            " {position}{follow_hint}search: {count} | n/N next/prev | Esc clear search | / new search | {wrap_hint} | ]/[ structure "
        );
    }

    format!(
        " {position}{follow_hint}{wrap_hint} | / search | ]/[ structure | t tool pair | 123 Enter line | m select | {page_hint} "
    )
}

fn follow_hint(state: &ViewState) -> &'static str {
    match state.follow {
        Some(FollowState::Following) => "follow:on | f pause | ",
        Some(FollowState::Detached) => "follow:detached | f pause | ",
        Some(FollowState::Paused) => "follow:off | f follow | ",
        None => "",
    }
}

fn follow_status(state: &ViewState) -> &'static str {
    match state.follow {
        Some(FollowState::Following) => "follow:on | ",
        Some(FollowState::Detached) => "follow:detached | ",
        Some(FollowState::Paused) => "follow:off | ",
        None => "",
    }
}

fn tool_context_footer_text(state: &ViewState) -> String {
    let Some(link) = state.tool_context.as_ref() else {
        return idle_footer_text(state);
    };
    let id = compact_tool_id(link.id.as_ref());
    let text = match (link.status, link.call_line) {
        (ToolLinkStatus::Matched, Some(call_line)) => {
            let at_call = state.tool_context_line == Some(call_line);
            if at_call {
                format!(
                    " tool call ↓ result line {} | id: {id} | t jump | ]/[ structure ",
                    link.result_line.saturating_add(1)
                )
            } else {
                format!(
                    " tool result ↑ call line {} | id: {id} | t jump | ]/[ structure ",
                    call_line.saturating_add(1)
                )
            }
        }
        (ToolLinkStatus::Ambiguous, _) => {
            format!(" ambiguous tool result | id: {id} | multiple earlier calls ")
        }
        _ => format!(" unmatched tool result | id: {id} | no earlier call "),
    };
    let follow = follow_status(state);
    if follow.is_empty() {
        text
    } else {
        format!(" {follow}{}", text.trim_start())
    }
}

fn compact_tool_id(id: &str) -> String {
    const MAX_CHARS: usize = 32;
    let mut chars = id.chars();
    let prefix = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn footer_message_label(kind: FooterMessageKind) -> &'static str {
    match kind {
        FooterMessageKind::Info => "info: ",
        FooterMessageKind::Warning => "warning: ",
        FooterMessageKind::Error => "error: ",
    }
}

pub(in crate::viewer) fn search_count_suffix(state: &ViewState) -> String {
    search_count_text(state)
        .map(|count| format!(" | {count}"))
        .unwrap_or_default()
}

pub(in crate::viewer) fn search_count_text(state: &ViewState) -> Option<String> {
    let index = state.search_index.as_ref()?;
    if index.query != state.search_query {
        return None;
    }

    let suffix = if index.exact { "" } else { "+" };
    let matches = state
        .search_match_ordinal
        .map(|ordinal| index.matches.max(ordinal))
        .unwrap_or(index.matches);
    let noun = if matches == 1 { "match" } else { "matches" };
    if let Some(ordinal) = state.search_match_ordinal {
        return Some(format!(
            "{}/{}{suffix} {noun}",
            format_count(ordinal),
            format_count(matches)
        ));
    }

    Some(format!("{}{suffix} {noun}", format_count(matches)))
}

pub(in crate::viewer) fn display_mode_text(state: &ViewState) -> String {
    if state.wrap {
        return wrap_position_text(state)
            .map(|position| format!("wrap {position}"))
            .unwrap_or_else(|| "wrap".to_owned());
    }

    format!("nowrap x:{}", state.x)
}

pub(in crate::viewer) fn line_count_text(file: &dyn ViewFile) -> String {
    let count = file.line_count();
    if file.line_count_exact() {
        count.to_string()
    } else {
        format!("{count}+")
    }
}

pub(in crate::viewer) fn gutter_digits(file: &dyn ViewFile, selection_mode: bool) -> usize {
    if selection_mode {
        0
    } else if file.line_count_exact() {
        line_number_digits(file.line_count())
    } else {
        line_number_digits(file.line_count()).max(4)
    }
}

fn wrap_position_text(state: &ViewState) -> Option<String> {
    if !state.wrap || state.top_row_offset == 0 {
        return None;
    }

    Some(format!("+{} rows", format_count(state.top_row_offset)))
}
