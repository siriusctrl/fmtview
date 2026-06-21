pub(crate) mod transform;

use crate::{
    formats::{ContentShape, FormatSpec},
    load::LoadPlan,
    transform::{FormatKind, TransformStrategy},
};

const EXTENSIONS: &[&str] = &["html", "htm"];

pub(crate) const SPEC: FormatSpec = FormatSpec {
    kind: FormatKind::Html,
    extensions: EXTENSIONS,
    // HTML5 parsing is document-level: optional close tags and implied
    // structure depend on parser state, so the transformed document is
    // produced before the viewer indexes it, like JSON and XML.
    shape: ContentShape::WholeDocument,
    load: LoadPlan::EagerTransformedDocument,
    transform: TransformStrategy::PrettyPrint,
};
