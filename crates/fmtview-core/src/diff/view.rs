use anyhow::Result;

use crate::{input::InputSource, load::LoadPlan, profile::TypeProfile, transform::FormatOptions};

use super::{
    DiffModel, RecordStreamDiff,
    external::{format_diff_inputs, run_external_diff_view_model},
};

enum DiffViewInner {
    Eager(DiffModel),
    Lazy(Box<RecordStreamDiff>),
}

pub struct DiffView(DiffViewInner);

impl DiffView {
    pub fn is_lazy(&self) -> bool {
        matches!(self.0, DiffViewInner::Lazy(_))
    }

    pub(crate) fn model(&self) -> &DiffModel {
        match &self.0 {
            DiffViewInner::Eager(model) => model,
            DiffViewInner::Lazy(diff) => diff.model(),
        }
    }

    pub(crate) fn preload(
        &mut self,
        max_records: usize,
        budget: std::time::Duration,
    ) -> Result<bool> {
        match &mut self.0 {
            DiffViewInner::Eager(_) => Ok(false),
            DiffViewInner::Lazy(diff) => diff.preload(max_records, budget),
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        match &self.0 {
            DiffViewInner::Eager(_) => true,
            DiffViewInner::Lazy(diff) => diff.is_complete(),
        }
    }
}

pub fn diff_view(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<DiffView> {
    if should_use_lazy_record_diff(left, right, options)? {
        return Ok(DiffView(DiffViewInner::Lazy(Box::new(
            RecordStreamDiff::new(left, right, *options)?,
        ))));
    }

    let formatted = format_diff_inputs(left, right, options)?;
    Ok(DiffView(DiffViewInner::Eager(
        run_external_diff_view_model(left, right, &formatted.left, &formatted.right)?,
    )))
}

fn should_use_lazy_record_diff(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<bool> {
    Ok(
        TypeProfile::resolve(left, options)?.load == LoadPlan::LazyTransformedRecords
            && TypeProfile::resolve(right, options)?.load == LoadPlan::LazyTransformedRecords,
    )
}
