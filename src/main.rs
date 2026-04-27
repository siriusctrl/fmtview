mod diff;
mod format;
mod input;
mod line_index;
mod viewer;

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use format::{FormatKind, FormatOptions};
use input::InputSource;

#[derive(Debug, Parser)]
#[command(
    name = "fmtview",
    version,
    about = "Fast formatter, diff tool, and terminal viewer for JSON, JSONL, and XML-compatible markup",
    args_conflicts_with_subcommands = true,
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    format: FormatCommand,
}

#[derive(Debug, Args)]
struct FormatCommand {
    /// Input file. Use '-' or omit the argument to read stdin.
    #[arg(value_name = "INPUT", default_value = "-")]
    input: String,

    /// Treat input as this format instead of auto-detecting.
    #[arg(short = 't', long = "type", value_enum, default_value_t = FormatKind::Auto)]
    kind: FormatKind,

    /// Format this literal string instead of reading INPUT/stdin.
    #[arg(long, value_name = "STRING")]
    literal: Option<String>,

    /// Number of spaces used when pretty-printing JSON and XML-compatible markup.
    #[arg(long, default_value_t = 2)]
    indent: usize,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Format both inputs and show a unified diff.
    Diff(DiffCommand),
}

#[derive(Debug, Args)]
struct DiffCommand {
    /// Left input file. Use '-' to read stdin.
    left: String,

    /// Right input file.
    right: String,

    /// Treat both inputs as this format instead of auto-detecting each one.
    #[arg(short = 't', long = "type", value_enum, default_value_t = FormatKind::Auto)]
    kind: FormatKind,

    /// Number of spaces used when pretty-printing JSON and XML-compatible markup.
    #[arg(long, default_value_t = 2)]
    indent: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Diff(command)) => run_diff(command),
        None => run_format(cli.format),
    }
}

fn run_format(command: FormatCommand) -> Result<()> {
    validate_indent(command.indent)?;

    let input = InputSource::from_arg(&command.input, command.literal.as_deref())
        .context("failed to open input")?;
    let options = FormatOptions {
        kind: command.kind,
        indent: command.indent,
    };

    let formatted = format::format_source_to_temp(&input, &options)?;

    if should_view() {
        let indexed = line_index::IndexedTempFile::new(input.label().to_owned(), formatted)?;
        viewer::run(indexed, viewer::ViewMode::Plain)
    } else {
        copy_temp_to_stdout(&formatted)
    }
}

fn run_diff(command: DiffCommand) -> Result<()> {
    validate_indent(command.indent)?;

    let left = InputSource::from_arg(&command.left, None).context("failed to open left input")?;
    let right =
        InputSource::from_arg(&command.right, None).context("failed to open right input")?;
    let options = FormatOptions {
        kind: command.kind,
        indent: command.indent,
    };

    let view = should_view();
    let diffed = diff::diff_sources(&left, &right, &options, view)?;

    if view {
        let label = format!("{} <-> {}", left.label(), right.label());
        let indexed = line_index::IndexedTempFile::new(label, diffed)?;
        viewer::run(indexed, viewer::ViewMode::Diff)
    } else {
        copy_temp_to_stdout(&diffed)
    }
}

fn should_view() -> bool {
    io::stdout().is_terminal()
}

fn copy_temp_to_stdout(temp: &tempfile::NamedTempFile) -> Result<()> {
    let mut file = std::fs::File::open(temp.path()).context("failed to reopen formatted output")?;
    let mut stdout = io::stdout().lock();
    io::copy(&mut file, &mut stdout).context("failed to write formatted output to stdout")?;
    stdout.flush().context("failed to flush stdout")?;
    Ok(())
}

fn validate_indent(indent: usize) -> Result<()> {
    if indent == 0 || indent > 16 {
        bail!("--indent must be between 1 and 16");
    }
    Ok(())
}
