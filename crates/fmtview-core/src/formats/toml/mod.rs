pub(crate) mod highlight;
pub(crate) mod structure;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["toml"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Toml,
    extensions: EXTENSIONS,
    shape: ContentShape::LineIndexed,
    load: LoadPlan::EagerIndexedSource,
    transform: TransformStrategy::Passthrough,
};
