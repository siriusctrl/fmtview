mod external;
mod model;
mod record_stream;
mod stdout;
mod view;

#[cfg(test)]
mod tests;

pub use stdout::diff_sources;

pub(crate) use model::{
    DiffChange, DiffIntensity, DiffLayout, DiffModel, DiffRange, NumberedDiffLine, SideDiffRow,
    UnifiedDiffRow,
};
pub(crate) use record_stream::RecordStreamDiff;
pub use view::{DiffView, diff_view};
