use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::{
    input::InputSource,
    profile::TypeProfile,
    transform::{self, FormatOptions},
};

use super::DiffModel;

pub(super) struct FormattedDiffInputs {
    pub(super) left: NamedTempFile,
    pub(super) right: NamedTempFile,
}

pub(super) fn format_diff_inputs(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
) -> Result<FormattedDiffInputs> {
    let left_profile = TypeProfile::resolve(left, options)?;
    let right_profile = TypeProfile::resolve(right, options)?;
    let left_options = left_profile.format_options(options.indent);
    let right_options = right_profile.format_options(options.indent);
    let left_formatted =
        transform::transform_source_to_temp(left, &left_options, left_profile.transform)
            .with_context(|| format!("failed to format left input {}", left.label()))?;
    let right_formatted =
        transform::transform_source_to_temp(right, &right_options, right_profile.transform)
            .with_context(|| format!("failed to format right input {}", right.label()))?;
    Ok(FormattedDiffInputs {
        left: left_formatted,
        right: right_formatted,
    })
}

pub(super) fn run_external_diff(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
    output: &mut NamedTempFile,
    show_equal_message: bool,
) -> Result<()> {
    let mut child = diff_command(left, right, left_formatted, right_formatted)
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

pub(super) fn run_external_diff_view_model(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
) -> Result<DiffModel> {
    let mut child = diff_command(left, right, left_formatted, right_formatted)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start diff")?;

    let stdout = child
        .stdout
        .take()
        .context("failed to capture diff stdout")?;
    let model = DiffModel::from_unified_reader(
        left.label().to_owned(),
        right.label().to_owned(),
        BufReader::new(stdout),
    )
    .context("failed to parse diff output")?;

    let result = child
        .wait_with_output()
        .context("failed to wait for diff")?;
    match result.status.code() {
        Some(0) | Some(1) => Ok(model),
        _ => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!("diff exited with {}: {}", result.status, stderr.trim())
        }
    }
}

fn diff_command(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
) -> Command {
    let mut command = Command::new("diff");
    command
        .arg("-u")
        .arg("--label")
        .arg(left.label())
        .arg("--label")
        .arg(right.label())
        .arg(left_formatted.path())
        .arg(right_formatted.path());
    command
}
