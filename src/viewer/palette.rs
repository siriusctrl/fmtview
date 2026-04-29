use ratatui::style::{Color, Style};

use crate::diff::DiffIntensity;

pub(crate) const PALETTE_TEXT: Color = Color::Indexed(145);
pub(crate) const PALETTE_MUTED: Color = Color::Indexed(59);
pub(crate) const PALETTE_BLUE: Color = Color::Indexed(75);
pub(crate) const PALETTE_CYAN: Color = Color::Indexed(73);
pub(crate) const PALETTE_GREEN: Color = Color::Indexed(114);
pub(crate) const PALETTE_PURPLE: Color = Color::Indexed(176);
pub(crate) const PALETTE_RED: Color = Color::Indexed(168);
pub(crate) const PALETTE_YELLOW: Color = Color::Indexed(180);
pub(crate) const PALETTE_ORANGE: Color = Color::Indexed(173);
pub(crate) const PALETTE_SEARCH_MATCH: Color = Color::Indexed(58);

pub(crate) fn style_fg(color: Color) -> Style {
    Style::default().fg(color)
}

pub(crate) fn plain_style() -> Style {
    style_fg(PALETTE_TEXT)
}

pub(crate) fn gutter_style() -> Style {
    style_fg(PALETTE_MUTED)
}

pub(crate) fn punctuation_style() -> Style {
    style_fg(PALETTE_MUTED)
}

pub(crate) fn key_style() -> Style {
    style_fg(PALETTE_BLUE)
}

pub(crate) fn xml_depth_style(depth: usize) -> Style {
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

pub(crate) fn attr_style() -> Style {
    style_fg(PALETTE_YELLOW)
}

pub(crate) fn string_style() -> Style {
    style_fg(PALETTE_GREEN)
}

pub(crate) fn escape_style() -> Style {
    style_fg(PALETTE_PURPLE)
}

pub(crate) fn number_style() -> Style {
    style_fg(PALETTE_ORANGE)
}

pub(crate) fn bool_style() -> Style {
    style_fg(PALETTE_YELLOW)
}

pub(crate) fn null_style() -> Style {
    style_fg(PALETTE_BLUE)
}

pub(crate) fn error_style() -> Style {
    style_fg(PALETTE_RED)
}

pub(crate) fn search_match_bg() -> Color {
    PALETTE_SEARCH_MATCH
}

pub(crate) fn diff_added_style() -> Style {
    style_fg(PALETTE_GREEN)
}

pub(crate) fn diff_removed_style() -> Style {
    style_fg(PALETTE_RED)
}

pub(crate) fn diff_added_line_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(24, 43, 35),
        DiffIntensity::Medium => Color::Rgb(33, 58, 45),
        DiffIntensity::High => Color::Rgb(43, 73, 56),
    }
}

pub(crate) fn diff_added_inline_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(44, 75, 58),
        DiffIntensity::Medium => Color::Rgb(55, 90, 67),
        DiffIntensity::High => Color::Rgb(68, 108, 79),
    }
}

pub(crate) fn diff_removed_line_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(49, 36, 40),
        DiffIntensity::Medium => Color::Rgb(64, 45, 50),
        DiffIntensity::High => Color::Rgb(78, 55, 60),
    }
}

pub(crate) fn diff_removed_inline_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(77, 55, 60),
        DiffIntensity::Medium => Color::Rgb(94, 65, 70),
        DiffIntensity::High => Color::Rgb(112, 78, 82),
    }
}
