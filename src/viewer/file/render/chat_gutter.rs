use ratatui::text::Span;

use crate::{formats::json::chat::ChatRole, tui::palette::gutter_style};

pub(in crate::viewer) const CHAT_ROLE_GUTTER_WIDTH: usize = 12;

pub(in crate::viewer) fn chat_role_gutter(role: Option<ChatRole>, enabled: bool) -> Span<'static> {
    if !enabled {
        return Span::raw("");
    }

    match role {
        Some(role) => Span::styled(format!("{:<9} │ ", role.label()), role.style()),
        None => Span::styled(format!("{:<9} │ ", ""), gutter_style()),
    }
}
