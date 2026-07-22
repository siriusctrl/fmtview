pub(crate) mod highlight;
pub(crate) mod structure;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["txt", "text", "log"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Plain,
    extensions: EXTENSIONS,
    shape: ContentShape::LineIndexed,
    load: LoadPlan::EagerIndexedSource,
    transform: TransformStrategy::Passthrough,
};
