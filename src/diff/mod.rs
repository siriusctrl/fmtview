use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::{
    format::{self, FormatOptions},
    input::InputSource,
};

mod model;

pub(crate) use model::{
    DiffChange, DiffIntensity, DiffLayout, DiffModel, DiffRange, NumberedDiffLine, SideDiffRow,
    UnifiedDiffRow,
};

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

pub(crate) fn diff_view(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<DiffModel> {
    let formatted = format_diff_inputs(left, right, options)?;
    let mut output = NamedTempFile::new().context("failed to create diff temp file")?;
    run_external_diff(
        left,
        right,
        &formatted.left,
        &formatted.right,
        &mut output,
        false,
    )?;
    output.flush().context("failed to flush diff temp file")?;
    let patch = fs::read_to_string(output.path()).context("failed to read diff output")?;
    Ok(DiffModel::from_unified_patch(
        left.label().to_owned(),
        right.label().to_owned(),
        &patch,
    ))
}

struct FormattedDiffInputs {
    left: NamedTempFile,
    right: NamedTempFile,
}

fn format_diff_inputs(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<FormattedDiffInputs> {
    let left_formatted = format::format_source_to_temp(left, options)
        .with_context(|| format!("failed to format left input {}", left.label()))?;
    let right_formatted = format::format_source_to_temp(right, options)
        .with_context(|| format!("failed to format right input {}", right.label()))?;
    Ok(FormattedDiffInputs {
        left: left_formatted,
        right: right_formatted,
    })
}

fn run_external_diff(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
    output: &mut NamedTempFile,
    show_equal_message: bool,
) -> Result<()> {
    let mut child = Command::new("diff")
        .arg("-u")
        .arg("--label")
        .arg(left.label())
        .arg("--label")
        .arg(right.label())
        .arg(left_formatted.path())
        .arg(right_formatted.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start diff")?;

    let mut stdout = child
        .stdout
        .take()
        .context("failed to capture diff stdout")?;
    std::io::copy(&mut stdout, output).context("failed to copy diff output")?;

    let result = child
        .wait_with_output()
        .context("failed to wait for diff")?;
    match result.status.code() {
        Some(0) => {
            if show_equal_message {
                writeln!(output, "No differences").context("failed to write empty diff message")?;
            }
            Ok(())
        }
        Some(1) => Ok(()),
        _ => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!("diff exited with {}: {}", result.status, stderr.trim())
        }
    }
}
