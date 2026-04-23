use std::{
    fs::File,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

pub struct InputSource {
    path: PathBuf,
    label: String,
    _temp: Option<NamedTempFile>,
}

impl InputSource {
    pub fn from_arg(arg: &str, literal: Option<&str>) -> Result<Self> {
        if let Some(literal) = literal {
            let mut temp =
                NamedTempFile::new().context("failed to create temp file for literal")?;
            temp.write_all(literal.as_bytes())
                .context("failed to write literal to temp file")?;
            temp.flush().context("failed to flush literal temp file")?;
            return Ok(Self {
                path: temp.path().to_owned(),
                label: "<literal>".to_owned(),
                _temp: Some(temp),
            });
        }

        if arg == "-" {
            if io::stdin().is_terminal() {
                bail!("stdin is a TTY; provide a file, pipe data, or use --literal");
            }

            let mut temp = NamedTempFile::new().context("failed to create temp file for stdin")?;
            let mut stdin = io::stdin().lock();
            io::copy(&mut stdin, temp.as_file_mut()).context("failed to copy stdin")?;
            temp.flush().context("failed to flush stdin temp file")?;
            return Ok(Self {
                path: temp.path().to_owned(),
                label: "<stdin>".to_owned(),
                _temp: Some(temp),
            });
        }

        let path = PathBuf::from(arg);
        if !path.exists() {
            bail!("input does not exist: {}", path.display());
        }
        if !path.is_file() {
            bail!("input is not a file: {}", path.display());
        }

        Ok(Self {
            label: arg.to_owned(),
            path,
            _temp: None,
        })
    }

    pub fn open(&self) -> Result<File> {
        File::open(&self.path).with_context(|| format!("failed to open {}", self.label))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}
