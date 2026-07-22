use crate::{
    formats::{StructureAnchor, StructureCandidateKind},
    transform::FormatKind,
    viewer::file::input::SearchTarget,
};

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

pub(super) fn select_structure_candidate(
    candidates: &[StructureCandidate],
    format: FormatKind,
    anchor: Option<StructureAnchor>,
) -> Option<StructureCandidate> {
    if matches!(format, FormatKind::Json | FormatKind::Jsonl)
        && candidates
            .iter()
            .any(|candidate| candidate.kind == StructureCandidateKind::JsonChatMessage)
    {
        return candidates
            .iter()
            .copied()
            .filter(|candidate| candidate.kind == StructureCandidateKind::JsonChatMessage)
            .min_by_key(|candidate| structure_candidate_rank(*candidate, format, anchor));
    }

    candidates
        .iter()
        .copied()
        .min_by_key(|candidate| structure_candidate_rank(*candidate, format, anchor))
}

fn structure_candidate_rank(
    candidate: StructureCandidate,
    format: FormatKind,
    anchor: Option<StructureAnchor>,
) -> (usize, usize, usize) {
    let distance = anchor
        .map(|anchor| anchor.line.abs_diff(candidate.line))
        .unwrap_or(candidate.line);
    if !matches!(format, FormatKind::Json | FormatKind::Jsonl) {
        return (0, distance, 0);
    }

    let Some(anchor) = anchor else {
        return (0, distance, json_candidate_priority(candidate.kind));
    };

    match anchor.kind {
        Some(StructureCandidateKind::JsonChatMessage) => {
            let scope = usize::from(candidate.kind != StructureCandidateKind::JsonChatMessage);
            (scope, distance, json_candidate_priority(candidate.kind))
        }
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
        StructureCandidateKind::JsonChatMessage => 0,
        StructureCandidateKind::JsonArrayItemStart => 1,
        StructureCandidateKind::JsonRootStart => 2,
        StructureCandidateKind::JsonRecordStart => 3,
        StructureCandidateKind::JsonCompositeField => 4,
        StructureCandidateKind::XmlStartTag => 5,
        StructureCandidateKind::MarkdownHeading
        | StructureCandidateKind::TomlTable
        | StructureCandidateKind::JinjaBlock
        | StructureCandidateKind::PlainParagraph => 6,
    }
}
