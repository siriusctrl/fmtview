use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["jsonl", "ndjson"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Jsonl,
    extensions: EXTENSIONS,
    shape: ContentShape::RecordStream,
    load: LoadPlan::LazyTransformedRecords,
    transform: TransformStrategy::RecordPrettyPrint,
};
