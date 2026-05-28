use crate::load::ViewFile;

use super::super::input::ViewState;
use super::{format_count, line_number_digits};

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
    } else if let Some(message) = &state.search_message {
        format!(
            " {message}{} | / search | n/N | Esc clear ",
            search_count_suffix(state)
        )
    } else {
        idle_footer_text(state)
    }
}

pub(in crate::viewer) fn idle_footer_text(state: &ViewState) -> String {
    let wrap_hint = if state.wrap { "w unwrap" } else { "w wrap" };
    let mouse_hint = if state.mouse_capture {
        "m select"
    } else {
        "m mouse"
    };
    let position = wrap_position_text(state)
        .map(|position| format!("{position} | "))
        .unwrap_or_default();
    let search = search_count_text(state)
        .map(|count| format!("{count} | "))
        .unwrap_or_default();
    format!(
        " {position}{search}{wrap_hint} | {mouse_hint} | / search n/N | ]/[ structure | 123 Enter jump to line | Space/f,b "
    )
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
