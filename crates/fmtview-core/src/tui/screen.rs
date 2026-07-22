use ratatui::{layout::Rect, style::Style, text::Line};

/// Logical position represented by a render frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollPosition {
    pub top: usize,
    pub row_offset: usize,
}

/// Direction for a backend scroll-region optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Optional hint that an outer terminal adapter can use for a compact repaint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollHint {
    pub amount: u16,
    pub direction: ScrollDirection,
}

impl ScrollHint {
    pub fn up(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Up,
        }
    }

    pub fn down(amount: u16) -> Self {
        Self {
            amount,
            direction: ScrollDirection::Down,
        }
    }
}

/// Backend-neutral output produced by a headless viewer engine.
pub struct RenderFrame {
    pub area: Rect,
    pub styled: Vec<Line<'static>>,
    pub sticky: Vec<Line<'static>>,
    pub selection_mode: bool,
    pub title: String,
    pub footer_text: String,
    pub footer_style: Style,
    pub position: ScrollPosition,
    pub scroll_hint: Option<ScrollHint>,
}
