use ratatui::text::Line;

use crate::syntax::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct ViewPosition {
    pub(in crate::viewer) top: usize,
    pub(in crate::viewer) row_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct RenderRequest {
    pub(in crate::viewer) context: RenderContext,
    pub(in crate::viewer) row_limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct RenderContext {
    pub(in crate::viewer) gutter_digits: usize,
    pub(in crate::viewer) x: usize,
    pub(in crate::viewer) width: usize,
    pub(in crate::viewer) wrap: bool,
    pub(in crate::viewer) mode: SyntaxKind,
}

#[derive(Debug)]
pub(in crate::viewer) struct RenderedViewport {
    pub(in crate::viewer) lines: Vec<Line<'static>>,
    pub(in crate::viewer) last_line_number: Option<usize>,
    pub(in crate::viewer) bottom: Option<ViewportBottom>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::viewer) struct ViewportBottom {
    pub(in crate::viewer) line_index: usize,
    pub(in crate::viewer) byte_end: usize,
    pub(in crate::viewer) line_end: bool,
}
