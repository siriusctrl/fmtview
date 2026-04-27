use ratatui::style::{Color, Style};

pub(super) const PALETTE_BACKGROUND: Color = Color::Rgb(40, 44, 52);
pub(super) const PALETTE_TEXT: Color = Color::Rgb(171, 178, 191);
pub(super) const PALETTE_MUTED: Color = Color::Rgb(92, 99, 112);
pub(super) const PALETTE_BLUE: Color = Color::Rgb(97, 175, 239);
pub(super) const PALETTE_CYAN: Color = Color::Rgb(86, 182, 194);
pub(super) const PALETTE_GREEN: Color = Color::Rgb(152, 195, 121);
pub(super) const PALETTE_PURPLE: Color = Color::Rgb(198, 120, 221);
pub(super) const PALETTE_RED: Color = Color::Rgb(224, 108, 117);
pub(super) const PALETTE_YELLOW: Color = Color::Rgb(229, 192, 123);
pub(super) const PALETTE_ORANGE: Color = Color::Rgb(209, 154, 102);
pub(super) const PALETTE_SELECTION: Color = Color::Rgb(62, 68, 81);

pub(super) fn style_fg(color: Color) -> Style {
    Style::default().fg(color).bg(PALETTE_BACKGROUND)
}

pub(super) fn plain_style() -> Style {
    style_fg(PALETTE_TEXT)
}

pub(super) fn gutter_style() -> Style {
    style_fg(PALETTE_MUTED)
}

pub(super) fn punctuation_style() -> Style {
    style_fg(PALETTE_MUTED)
}

pub(super) fn key_style() -> Style {
    style_fg(PALETTE_BLUE)
}

pub(super) fn xml_depth_style(depth: usize) -> Style {
    const COLORS: [Color; 6] = [
        PALETTE_CYAN,
        PALETTE_PURPLE,
        PALETTE_YELLOW,
        PALETTE_GREEN,
        PALETTE_BLUE,
        PALETTE_ORANGE,
    ];

    style_fg(COLORS[depth % COLORS.len()])
}

pub(super) fn attr_style() -> Style {
    style_fg(PALETTE_YELLOW)
}

pub(super) fn string_style() -> Style {
    style_fg(PALETTE_GREEN)
}

pub(super) fn escape_style() -> Style {
    style_fg(PALETTE_PURPLE)
}

pub(super) fn number_style() -> Style {
    style_fg(PALETTE_ORANGE)
}

pub(super) fn bool_style() -> Style {
    style_fg(PALETTE_YELLOW)
}

pub(super) fn null_style() -> Style {
    style_fg(PALETTE_BLUE)
}

pub(super) fn error_style() -> Style {
    style_fg(PALETTE_RED)
}

pub(super) fn search_match_bg() -> Color {
    PALETTE_SELECTION
}

pub(super) fn diff_hunk_style() -> Style {
    style_fg(PALETTE_CYAN)
}

pub(super) fn diff_file_style() -> Style {
    style_fg(PALETTE_YELLOW)
}

pub(super) fn diff_added_style() -> Style {
    style_fg(PALETTE_GREEN)
}

pub(super) fn diff_removed_style() -> Style {
    style_fg(PALETTE_RED)
}
