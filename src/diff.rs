use std::{
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::{
    format::{self, FormatOptions},
    input::InputSource,
};

pub fn diff_sources(
    left: &InputSource,
    right: &InputSource,
    options: &FormatOptions,
    show_equal_message: bool,
) -> Result<NamedTempFile> {
    let left_formatted = format::format_source_to_temp(left, options)
        .with_context(|| format!("failed to format left input {}", left.label()))?;
    let right_formatted = format::format_source_to_temp(right, options)
        .with_context(|| format!("failed to format right input {}", right.label()))?;

    let mut output = NamedTempFile::new().context("failed to create diff temp file")?;
    run_external_diff(
        left,
        right,
        &left_formatted,
        &right_formatted,
        &mut output,
        show_equal_message,
    )?;
    output.flush().context("failed to flush diff temp file")?;
    Ok(output)
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
