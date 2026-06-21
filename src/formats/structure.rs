const JSON_VISIBLE_COMPOSITE_LANDMARK_LINES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructureCandidateKind {
    JsonChatMessage,
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
    pub(crate) fn is_landmark_when_visible(self, line_span: Option<usize>) -> bool {
        match self {
            StructureCandidateKind::JsonChatMessage
            | StructureCandidateKind::JsonRecordStart
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
