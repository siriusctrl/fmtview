use ratatui::text::Span;

use crate::{
    formats::json::chat::ChatRole, load::ViewFile, transform::FormatKind,
    tui::palette::gutter_style,
};

use super::footer::gutter_digits;

const WRAP_GUTTER_MINOR_TICK_ROWS: usize = 8;
const WRAP_GUTTER_MAJOR_TICK_ROWS: usize = 64;
const COMPACT_CHAT_GUTTER_WIDTH: usize = 4;
const MIN_CONTENT_WIDTH_WITH_CHAT_GUTTER: usize = 58;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatGutterMode {
    Compact,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct GutterLayout {
    line_digits: usize,
    chat_mode: ChatGutterMode,
}

impl GutterLayout {
    pub(in crate::viewer) fn new(line_digits: usize, chat_role: bool) -> Self {
        Self {
            line_digits,
            chat_mode: if chat_role && line_digits > 0 {
                ChatGutterMode::Compact
            } else {
                ChatGutterMode::Off
            },
        }
    }

    pub(in crate::viewer) fn for_view(
        file: &dyn ViewFile,
        selection_mode: bool,
        mode: FormatKind,
        visible_width: usize,
    ) -> Self {
        let line_digits = gutter_digits(file, selection_mode);
        let mut layout = Self::new(
            line_digits,
            matches!(mode, FormatKind::Json | FormatKind::Jsonl),
        );
        if layout.chat_role_width() > 0
            && visible_width.saturating_sub(layout.width()) < MIN_CONTENT_WIDTH_WITH_CHAT_GUTTER
        {
            layout.chat_mode = ChatGutterMode::Off;
        }
        layout
    }

    pub(in crate::viewer) fn width(self) -> usize {
        self.line_number_width()
            .saturating_add(self.chat_role_width())
    }

    pub(in crate::viewer) fn content_start(self) -> usize {
        self.width()
    }

    pub(in crate::viewer) fn line_number(self, line_number: usize) -> Span<'static> {
        if self.line_digits == 0 {
            return Span::raw("");
        }

        Span::styled(
            format!("{line_number:>width$} │ ", width = self.line_digits),
            gutter_style(),
        )
    }

    pub(in crate::viewer) fn continuation(self, row_index: usize) -> Span<'static> {
        if self.line_digits == 0 {
            return Span::raw("");
        }

        let marker = continuation_gutter_marker(row_index);
        Span::styled(
            format!("{:>width$} {marker} ", "", width = self.line_digits),
            gutter_style(),
        )
    }

    pub(in crate::viewer) fn chat_role(
        self,
        role: Option<ChatRole>,
        show_label: bool,
        color_guide: bool,
    ) -> [Span<'static>; 2] {
        if self.chat_mode == ChatGutterMode::Off {
            return [Span::raw(""), Span::raw("")];
        }

        let label = match role {
            Some(role) if show_label => {
                Span::styled(format!("{} ", role.compact_label()), role.style())
            }
            _ => Span::styled("  ".to_owned(), gutter_style()),
        };
        let guide = match role {
            Some(role) if color_guide => Span::styled("│ ".to_owned(), role.style()),
            _ => Span::styled("│ ".to_owned(), gutter_style()),
        };
        [label, guide]
    }

    pub(in crate::viewer) fn chat_role_enabled(self) -> bool {
        self.chat_mode != ChatGutterMode::Off
    }

    fn line_number_width(self) -> usize {
        if self.line_digits == 0 {
            0
        } else {
            self.line_digits + 3
        }
    }

    fn chat_role_width(self) -> usize {
        match self.chat_mode {
            ChatGutterMode::Compact => COMPACT_CHAT_GUTTER_WIDTH,
            ChatGutterMode::Off => 0,
        }
    }
}

#[cfg(test)]
pub(in crate::viewer) fn line_number_gutter(
    line_number: usize,
    gutter_digits: usize,
) -> Span<'static> {
    GutterLayout::new(gutter_digits, false).line_number(line_number)
}

#[cfg(test)]
pub(in crate::viewer) fn continuation_gutter(
    row_index: usize,
    gutter_digits: usize,
) -> Span<'static> {
    GutterLayout::new(gutter_digits, false).continuation(row_index)
}

pub(in crate::viewer) fn continuation_gutter_marker(row_index: usize) -> char {
    if row_index > 0 && row_index % WRAP_GUTTER_MAJOR_TICK_ROWS == 0 {
        '┠'
    } else if row_index > 0 && row_index % WRAP_GUTTER_MINOR_TICK_ROWS == 0 {
        '┊'
    } else {
        '┆'
    }
}
