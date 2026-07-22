use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use fmtview_core::{
    ContentShape, FormatKind, FormatOptions, TypeProfile, diff_sources, diff_view,
    open_follow_view_file, open_view_file, open_view_file_with_fallback, transform_source_to_temp,
};

use crate::{
    input,
    shell_alias::{AliasCommandOptions, AliasShell, run_alias_command},
    viewer,
};

#[derive(Debug, Parser)]
#[command(
    name = "fmtview",
    version,
    about = "Fast formatter, diff tool, and terminal viewer for JSON, JSONL, XML, HTML, Markdown, TOML, plain text, and Jinja templates",
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

    /// Treat input as this type instead of auto-detecting.
    #[arg(short = 't', long = "type", value_enum, default_value_t = CliFormatKind::Auto)]
    kind: CliFormatKind,

    /// Format this literal string instead of reading INPUT/stdin.
    #[arg(long, value_name = "STRING")]
    literal: Option<String>,

    /// Number of spaces used when pretty-printing JSON, XML, and HTML.
    #[arg(long, default_value_t = 2)]
    indent: usize,

    /// Open a JSONL/NDJSON file at its tail and follow committed appended records.
    #[arg(short = 'F', long)]
    follow: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print or install a short shell alias such as `fv`.
    Alias(AliasCommand),

    /// Format both inputs and show a unified diff.
    Diff(DiffCommand),
}

#[derive(Debug, Args)]
struct AliasCommand {
    /// Shell syntax to generate.
    #[arg(value_enum)]
    shell: AliasShell,

    /// Install the alias into the shell's startup file instead of printing it.
    #[arg(short = 'i', long)]
    install: bool,

    /// Alias name to generate.
    #[arg(long, default_value = "fv")]
    name: String,
}

#[derive(Debug, Args)]
struct DiffCommand {
    /// Left input file. Use '-' to read stdin.
    left: String,

    /// Right input file.
    right: String,

    /// Treat both inputs as this type instead of auto-detecting each one.
    #[arg(short = 't', long = "type", value_enum, default_value_t = CliFormatKind::Auto)]
    kind: CliFormatKind,

    /// Number of spaces used when pretty-printing JSON, XML, and HTML.
    #[arg(long, default_value_t = 2)]
    indent: usize,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Alias(command)) => run_alias(command),
        Some(Command::Diff(command)) => run_diff(command),
        None => run_format(cli.format),
    }
}

fn run_alias(command: AliasCommand) -> Result<()> {
    let mut stdout = io::stdout().lock();
    run_alias_command(
        &AliasCommandOptions {
            shell: command.shell,
            install: command.install,
            name: command.name,
        },
        &mut stdout,
    )
}

fn run_format(command: FormatCommand) -> Result<()> {
    validate_indent(command.indent)?;

    if command.follow && (command.literal.is_some() || command.input == "-") {
        bail!("--follow requires a file path; stdin and --literal are not live sources");
    }
    if command.follow && !should_view() {
        bail!("--follow requires an interactive terminal on stdout");
    }

    let input = input::from_arg(&command.input, command.literal.as_deref())
        .context("failed to open input")?;
    let kind = FormatKind::from(command.kind);
    let options = FormatOptions {
        kind,
        indent: command.indent,
    };
    let profile = TypeProfile::resolve(&input, &options)?;
    let resolved_options = profile.format_options(command.indent);

    if command.follow {
        if profile.content_shape() != ContentShape::RecordStream
            || profile.content_kind() != FormatKind::Jsonl
        {
            bail!("--follow currently supports JSONL/NDJSON record streams only");
        }
        let opened = open_follow_view_file(&input, &resolved_options)?;
        return viewer::run(opened.file, opened.content, opened.notice);
    }

    if should_view() {
        let opened = if kind == FormatKind::Auto {
            open_view_file_with_fallback(&input, &resolved_options, profile, true)?
        } else {
            open_view_file(&input, &resolved_options, profile)?
        };
        viewer::run(opened.file, opened.content, opened.notice)
    } else {
        let formatted = transform_source_to_temp(&input, &resolved_options, profile)?;
        copy_temp_to_stdout(&formatted)
    }
}

fn run_diff(command: DiffCommand) -> Result<()> {
    validate_indent(command.indent)?;

    let left = input::from_arg(&command.left, None).context("failed to open left input")?;
    let right = input::from_arg(&command.right, None).context("failed to open right input")?;
    let options = FormatOptions {
        kind: command.kind.into(),
        indent: command.indent,
    };

    if should_view() {
        let model = diff_view(&left, &right, &options)?;
        viewer::run_diff(model)
    } else {
        let diffed = diff_sources(&left, &right, &options, false)?;
        copy_temp_to_stdout(&diffed)
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFormatKind {
    Auto,
    Json,
    Jsonl,
    Xml,
    Html,
    Toml,
    Markdown,
    Plain,
    Jinja,
}

impl From<CliFormatKind> for FormatKind {
    fn from(kind: CliFormatKind) -> Self {
        match kind {
            CliFormatKind::Auto => Self::Auto,
            CliFormatKind::Json => Self::Json,
            CliFormatKind::Jsonl => Self::Jsonl,
            CliFormatKind::Xml => Self::Xml,
            CliFormatKind::Html => Self::Html,
            CliFormatKind::Toml => Self::Toml,
            CliFormatKind::Markdown => Self::Markdown,
            CliFormatKind::Plain => Self::Plain,
            CliFormatKind::Jinja => Self::Jinja,
        }
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
