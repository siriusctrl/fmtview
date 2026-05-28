pub(crate) mod highlight;
pub(crate) mod structure;
pub(crate) mod transform;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["xml", "html", "htm", "xhtml"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Xml,
    extensions: EXTENSIONS,
    shape: ContentShape::WholeDocument,
    load: LoadPlan::EagerTransformedDocument,
    transform: TransformStrategy::PrettyPrint,
};
