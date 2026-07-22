use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use fmtview_core::InputSource;
use tempfile::NamedTempFile;

pub(crate) fn from_arg(arg: &str, literal: Option<&str>) -> Result<InputSource> {
    if let Some(literal) = literal {
        let mut temp = NamedTempFile::new().context("failed to create temp file for literal")?;
        temp.write_all(literal.as_bytes())
            .context("failed to write literal to temp file")?;
        temp.flush().context("failed to flush literal temp file")?;
        return Ok(InputSource::from_temp(temp, "<literal>"));
    }

    if arg == "-" {
        if io::stdin().is_terminal() {
            bail!("stdin is a TTY; provide a file, pipe data, or use --literal");
        }

        let mut temp = NamedTempFile::new().context("failed to create temp file for stdin")?;
        let mut stdin = io::stdin().lock();
        io::copy(&mut stdin, temp.as_file_mut()).context("failed to copy stdin")?;
        temp.flush().context("failed to flush stdin temp file")?;
        return Ok(InputSource::from_temp(temp, "<stdin>"));
    }

    InputSource::from_path(arg, arg)
}
