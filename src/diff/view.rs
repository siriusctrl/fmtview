use anyhow::Result;

use crate::{
    input::InputSource,
    load::{self, LoadPlan},
    transform::FormatOptions,
};

use super::{
    DiffModel, LazyRecordDiff,
    external::{format_diff_inputs, run_external_diff_view_model},
};

pub(crate) enum DiffView {
    Eager(DiffModel),
    Lazy(Box<LazyRecordDiff>),
}

impl DiffView {
    pub(crate) fn model(&self) -> &DiffModel {
        match self {
            Self::Eager(model) => model,
            Self::Lazy(diff) => diff.model(),
        }
    }

    pub(crate) fn preload(
        &mut self,
        max_records: usize,
        budget: std::time::Duration,
    ) -> Result<bool> {
        match self {
            Self::Eager(_) => Ok(false),
            Self::Lazy(diff) => diff.preload(max_records, budget),
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Eager(_) => true,
            Self::Lazy(diff) => diff.is_complete(),
        }
    }
}

pub(crate) fn diff_view(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<DiffView> {
    if should_use_lazy_record_diff(left, right, options)? {
        return Ok(DiffView::Lazy(Box::new(LazyRecordDiff::new(
            left, right, *options,
        )?)));
    }

    let formatted = format_diff_inputs(left, right, options)?;
    Ok(DiffView::Eager(run_external_diff_view_model(
        left,
        right,
        &formatted.left,
        &formatted.right,
    )?))
}

fn should_use_lazy_record_diff(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<bool> {
    Ok(load::load_plan(left, options)? == LoadPlan::LazyRecords
        && load::load_plan(right, options)? == LoadPlan::LazyRecords)
}
