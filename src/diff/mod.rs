mod external;
mod lazy_records;
mod model;
mod stdout;
mod view;

#[cfg(test)]
mod tests;

pub use stdout::diff_sources;

pub(crate) use lazy_records::LazyRecordDiff;
pub(crate) use model::{
    DiffChange, DiffIntensity, DiffLayout, DiffModel, DiffRange, NumberedDiffLine, SideDiffRow,
    UnifiedDiffRow,
};
pub(crate) use view::{DiffView, diff_view};
