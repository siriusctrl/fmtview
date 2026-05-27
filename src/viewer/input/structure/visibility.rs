use unicode_width::UnicodeWidthStr;

use crate::syntax::SyntaxKind;

use super::{StructureViewport, candidate::StructureCandidateKind, syntax::structure_block_end};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CandidateVisibility {
    fully_observed: bool,
    end_line: Option<usize>,
}

impl CandidateVisibility {
    fn unknown() -> Self {
        Self {
            fully_observed: false,
            end_line: None,
        }
    }

    fn fully_observed(end_line: usize) -> Self {
        Self {
            fully_observed: true,
            end_line: Some(end_line),
        }
    }

    fn partially_observed(end_line: Option<usize>) -> Self {
        Self {
            fully_observed: false,
            end_line,
        }
    }

    fn line_span(self, start_line: usize) -> Option<usize> {
        self.end_line
            .map(|end_line| end_line.saturating_sub(start_line).saturating_add(1))
    }
}

pub(super) fn should_skip_candidate(
    kind: StructureCandidateKind,
    start_line: usize,
    visibility: CandidateVisibility,
) -> bool {
    visibility.fully_observed && !kind.is_landmark_when_visible(visibility.line_span(start_line))
}

pub(super) fn candidate_visibility(
    syntax: SyntaxKind,
    lines: &[String],
    read_start: usize,
    candidate_offset: usize,
    line_count: usize,
    line_count_exact: bool,
    viewport: Option<StructureViewport>,
) -> CandidateVisibility {
    let Some(viewport) = viewport else {
        return CandidateVisibility::unknown();
    };
    let start_line = read_start + candidate_offset;
    if start_line < viewport.top || start_line > viewport.bottom {
        return CandidateVisibility::unknown();
    }
    if start_line == viewport.top && viewport.top_row_offset > 0 {
        return CandidateVisibility::unknown();
    }

    let Some(end_line) = structure_block_end(
        syntax,
        lines,
        read_start,
        candidate_offset,
        viewport.bottom,
        line_count,
        line_count_exact,
    ) else {
        return CandidateVisibility::unknown();
    };
    if end_line > viewport.bottom {
        return CandidateVisibility::partially_observed(Some(end_line));
    }
    if end_line == viewport.bottom && !viewport.bottom_line_end {
        return CandidateVisibility::partially_observed(Some(end_line));
    }

    if block_is_horizontally_observed(lines, read_start, start_line, end_line, viewport) {
        CandidateVisibility::fully_observed(end_line)
    } else {
        CandidateVisibility::partially_observed(Some(end_line))
    }
}

fn block_is_horizontally_observed(
    lines: &[String],
    read_start: usize,
    start_line: usize,
    end_line: usize,
    viewport: StructureViewport,
) -> bool {
    if viewport.wrap {
        return true;
    }
    if viewport.x > 0 || viewport.width == 0 {
        return false;
    }
    let start_offset = start_line.saturating_sub(read_start);
    let end_offset = end_line.saturating_sub(read_start);
    lines
        .get(start_offset..=end_offset)
        .is_some_and(|block| block.iter().all(|line| line.width() <= viewport.width))
}
