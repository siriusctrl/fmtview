#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPlan {
    LazyTransformedRecords,
    LazyDelimitedRecords,
    EagerTransformedDocument,
    EagerIndexedSource,
}
