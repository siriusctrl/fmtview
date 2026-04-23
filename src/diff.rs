use std::{
    io::{Read, Write},
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
) -> Result<NamedTempFile> {
    let left_formatted = format::format_source_to_temp(left, options)
        .with_context(|| format!("failed to format left input {}", left.label()))?;
    let right_formatted = format::format_source_to_temp(right, options)
        .with_context(|| format!("failed to format right input {}", right.label()))?;

    let mut output = NamedTempFile::new().context("failed to create diff temp file")?;
    match run_external_diff(left, right, &left_formatted, &right_formatted, &mut output) {
        Ok(()) => {}
        Err(error) => {
            writeln!(
                output,
                "external diff failed ({error:#}); falling back to streaming line comparison"
            )
            .context("failed to write diff fallback header")?;
            streaming_diff(left, right, &left_formatted, &right_formatted, &mut output)?;
        }
    }
    output.flush().context("failed to flush diff temp file")?;
    Ok(output)
}

fn run_external_diff(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
    output: &mut NamedTempFile,
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
            writeln!(output, "No differences").context("failed to write empty diff message")?;
            Ok(())
        }
        Some(1) => Ok(()),
        _ => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!("diff exited with {}: {}", result.status, stderr.trim())
        }
    }
}

fn streaming_diff(
    left: &InputSource,
    right: &InputSource,
    left_formatted: &NamedTempFile,
    right_formatted: &NamedTempFile,
    output: &mut NamedTempFile,
) -> Result<()> {
    let mut left_reader = std::io::BufReader::new(std::fs::File::open(left_formatted.path())?);
    let mut right_reader = std::io::BufReader::new(std::fs::File::open(right_formatted.path())?);
    let mut left_line = String::new();
    let mut right_line = String::new();
    let mut line_number = 1_usize;
    let mut differences = 0_usize;

    writeln!(output, "--- {}", left.label()).context("failed to write diff header")?;
    writeln!(output, "+++ {}", right.label()).context("failed to write diff header")?;

    loop {
        left_line.clear();
        right_line.clear();
        let left_read = read_line(&mut left_reader, &mut left_line)?;
        let right_read = read_line(&mut right_reader, &mut right_line)?;
        if left_read == 0 && right_read == 0 {
            break;
        }

        if left_line != right_line {
            differences += 1;
            writeln!(output, "@@ line {} @@", line_number).context("failed to write diff hunk")?;
            if left_read != 0 {
                write!(output, "-{}", left_line).context("failed to write left diff line")?;
            }
            if right_read != 0 {
                write!(output, "+{}", right_line).context("failed to write right diff line")?;
            }
        }
        line_number += 1;
    }

    if differences == 0 {
        writeln!(output, "No differences").context("failed to write empty diff message")?;
    }

    Ok(())
}

fn read_line<R: Read>(reader: &mut std::io::BufReader<R>, line: &mut String) -> Result<usize> {
    use std::io::BufRead;

    reader.read_line(line).context("failed to read diff line")
}
