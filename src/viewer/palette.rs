use ratatui::style::{Color, Style};

use crate::diff::DiffIntensity;

pub(super) const PALETTE_TEXT: Color = Color::Indexed(145);
pub(super) const PALETTE_MUTED: Color = Color::Indexed(59);
pub(super) const PALETTE_BLUE: Color = Color::Indexed(75);
pub(super) const PALETTE_CYAN: Color = Color::Indexed(73);
pub(super) const PALETTE_GREEN: Color = Color::Indexed(114);
pub(super) const PALETTE_PURPLE: Color = Color::Indexed(176);
pub(super) const PALETTE_RED: Color = Color::Indexed(168);
pub(super) const PALETTE_YELLOW: Color = Color::Indexed(180);
pub(super) const PALETTE_ORANGE: Color = Color::Indexed(173);
pub(super) const PALETTE_SEARCH_MATCH: Color = Color::Indexed(58);

pub(super) fn style_fg(color: Color) -> Style {
    Style::default().fg(color)
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
    PALETTE_SEARCH_MATCH
}

pub(super) fn diff_added_style() -> Style {
    style_fg(PALETTE_GREEN)
}

pub(super) fn diff_removed_style() -> Style {
    style_fg(PALETTE_RED)
}

pub(super) fn diff_added_line_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(24, 43, 35),
        DiffIntensity::Medium => Color::Rgb(33, 58, 45),
        DiffIntensity::High => Color::Rgb(43, 73, 56),
    }
}

pub(super) fn diff_added_inline_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(44, 75, 58),
        DiffIntensity::Medium => Color::Rgb(55, 90, 67),
        DiffIntensity::High => Color::Rgb(68, 108, 79),
    }
}

pub(super) fn diff_removed_line_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(49, 36, 40),
        DiffIntensity::Medium => Color::Rgb(64, 45, 50),
        DiffIntensity::High => Color::Rgb(78, 55, 60),
    }
}

pub(super) fn diff_removed_inline_bg(intensity: DiffIntensity) -> Color {
    match intensity {
        DiffIntensity::Low => Color::Rgb(77, 55, 60),
        DiffIntensity::Medium => Color::Rgb(94, 65, 70),
        DiffIntensity::High => Color::Rgb(112, 78, 82),
    }
}
