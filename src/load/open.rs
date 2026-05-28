use anyhow::Result;

use crate::{
    input::InputSource,
    profile::TypeProfile,
    transform::{self, FormatOptions},
};

use super::{IndexedTempFile, LazyTransformedRecordsFile, LoadPlan, ViewFile};

pub fn open_view_file(
    input: &InputSource,
    options: &FormatOptions,
    profile: TypeProfile,
) -> Result<Box<dyn ViewFile>> {
    match profile.load {
        LoadPlan::LazyTransformedRecords => {
            Ok(Box::new(LazyTransformedRecordsFile::new(input, *options)?))
        }
        LoadPlan::EagerTransformedDocument | LoadPlan::EagerIndexedSource => {
            let formatted = transform::transform_source_to_temp(input, options, profile.transform)?;
            Ok(Box::new(IndexedTempFile::new(
                input.label().to_owned(),
                formatted,
            )?))
        }
    }
}
