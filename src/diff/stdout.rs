use std::io::Write;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use crate::{format::FormatOptions, input::InputSource};

use super::external::{format_diff_inputs, run_external_diff};

pub fn diff_sources(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
    show_equal_message: bool,
) -> Result<NamedTempFile> {
    let formatted = format_diff_inputs(left, right, options)?;

    let mut output = NamedTempFile::new().context("failed to create diff temp file")?;
    run_external_diff(
        left,
        right,
        &formatted.left,
        &formatted.right,
        &mut output,
        show_equal_message,
    )?;
    output.flush().context("failed to flush diff temp file")?;
    Ok(output)
}
