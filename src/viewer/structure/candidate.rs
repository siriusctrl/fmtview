use crate::syntax::SyntaxKind;

use crate::viewer::input::SearchTarget;

const JSON_VISIBLE_COMPOSITE_LANDMARK_LINES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StructureCandidateKind {
    JsonRecordStart,
    JsonArrayItemStart,
    JsonCompositeField,
    JsonRootStart,
    XmlStartTag,
    MarkdownHeading,
    TomlTable,
    JinjaBlock,
    PlainParagraph,
}

impl StructureCandidateKind {
    pub(super) fn is_landmark_when_visible(self, line_span: Option<usize>) -> bool {
        match self {
            StructureCandidateKind::JsonRecordStart
            | StructureCandidateKind::JsonArrayItemStart
            | StructureCandidateKind::JsonRootStart
            | StructureCandidateKind::MarkdownHeading
            | StructureCandidateKind::TomlTable
            | StructureCandidateKind::JinjaBlock
            | StructureCandidateKind::PlainParagraph => true,
            StructureCandidateKind::XmlStartTag => line_span.is_none_or(|span| span > 1),
            StructureCandidateKind::JsonCompositeField => {
                line_span.is_some_and(|span| span >= JSON_VISIBLE_COMPOSITE_LANDMARK_LINES)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StructureCandidate {
    pub(super) line: usize,
    pub(super) byte_index: usize,
    pub(super) kind: StructureCandidateKind,
    pub(super) indent: usize,
}

impl StructureCandidate {
    pub(super) fn target(self) -> SearchTarget {
        SearchTarget {
            line: self.line,
            byte_index: self.byte_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StructureAnchor {
    pub(super) line: usize,
    pub(super) kind: Option<StructureCandidateKind>,
    pub(super) indent: usize,
}

pub(super) fn select_structure_candidate(
    candidates: &[StructureCandidate],
    syntax: SyntaxKind,
    anchor: Option<StructureAnchor>,
) -> Option<StructureCandidate> {
    candidates
        .iter()
        .copied()
        .min_by_key(|candidate| structure_candidate_rank(*candidate, syntax, anchor))
}

fn structure_candidate_rank(
    candidate: StructureCandidate,
    syntax: SyntaxKind,
    anchor: Option<StructureAnchor>,
) -> (usize, usize, usize) {
    let distance = anchor
        .map(|anchor| anchor.line.abs_diff(candidate.line))
        .unwrap_or(candidate.line);
    if syntax != SyntaxKind::Structured {
        return (0, distance, 0);
    }

    let Some(anchor) = anchor else {
        return (0, distance, json_candidate_priority(candidate.kind));
    };

    match anchor.kind {
        Some(StructureCandidateKind::JsonArrayItemStart) => {
            let scope = usize::from(candidate.indent > anchor.indent);
            (scope, json_candidate_priority(candidate.kind), distance)
        }
        Some(StructureCandidateKind::JsonCompositeField) => {
            let scope = usize::from(candidate.indent <= anchor.indent);
            (scope, distance, json_candidate_priority(candidate.kind))
        }
        Some(StructureCandidateKind::JsonRecordStart | StructureCandidateKind::JsonRootStart) => {
            let scope = usize::from(candidate.kind == StructureCandidateKind::JsonRecordStart);
            (scope, distance, json_candidate_priority(candidate.kind))
        }
        _ => (0, distance, json_candidate_priority(candidate.kind)),
    }
}

fn json_candidate_priority(kind: StructureCandidateKind) -> usize {
    match kind {
        StructureCandidateKind::JsonArrayItemStart => 0,
        StructureCandidateKind::JsonRootStart => 1,
        StructureCandidateKind::JsonRecordStart => 2,
        StructureCandidateKind::JsonCompositeField => 3,
        StructureCandidateKind::XmlStartTag => 4,
        StructureCandidateKind::MarkdownHeading
        | StructureCandidateKind::TomlTable
        | StructureCandidateKind::JinjaBlock
        | StructureCandidateKind::PlainParagraph => 5,
    }
}
