pub(crate) mod highlight;
pub(crate) mod structure;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Markdown,
    extensions: EXTENSIONS,
    shape: ContentShape::LineIndexed,
    load: LoadPlan::EagerIndexedSource,
    transform: TransformStrategy::Passthrough,
};
