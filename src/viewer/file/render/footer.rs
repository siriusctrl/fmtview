use crate::load::ViewFile;
use crate::tui::palette::{error_style, gutter_style, warning_style};

use super::super::input::{FooterMessageKind, ViewState};
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
    if state.search_active {
        format!(
            " search: {} | Enter find | Backspace edit | Esc cancel ",
            state.search_buffer
        )
    } else if !state.jump_buffer.is_empty() {
        format!(
            " go to line: {} / {} | Enter jump | Backspace edit | Esc cancel ",
            state.jump_buffer,
            line_count_text(file)
        )
    } else if let Some(message) = state.visible_footer_message() {
        format!(
            " {}{}{} | / search | n/N | Esc clear ",
            footer_message_label(message.kind),
            message.text,
            search_count_suffix(state)
        )
    } else {
        idle_footer_text(state)
    }
}

pub(in crate::viewer) fn file_footer_style(state: &ViewState) -> Style {
    match state.visible_footer_message().map(|message| message.kind) {
        Some(FooterMessageKind::Error) => error_style(),
        Some(FooterMessageKind::Warning) => warning_style(),
        Some(FooterMessageKind::Info) | None => gutter_style(),
    }
}

pub(in crate::viewer) fn idle_footer_text(state: &ViewState) -> String {
    if !state.mouse_capture {
        return " selection mode | m restore mouse ".to_owned();
    }

    let wrap_hint = if state.wrap { "w unwrap" } else { "w wrap" };
    let position = wrap_position_text(state)
        .map(|position| format!("{position} | "))
        .unwrap_or_default();
    if let Some(count) = search_count_text(state) {
        return format!(
            " {position}search: {count} | n/N next/prev | Esc clear search | / new search | {wrap_hint} | ]/[ structure "
        );
    }

    format!(
        " {position}{wrap_hint} | / search | ]/[ structure | 123 Enter line | m select | Space/f,b "
    )
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
