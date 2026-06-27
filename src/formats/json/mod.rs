pub(crate) mod chat;
pub(crate) mod highlight;
pub(crate) mod path;
pub(crate) mod structure;
pub(crate) mod transform;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["json"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Json,
    extensions: EXTENSIONS,
    shape: ContentShape::WholeDocument,
    load: LoadPlan::EagerTransformedDocument,
    transform: TransformStrategy::PrettyPrint,
};
