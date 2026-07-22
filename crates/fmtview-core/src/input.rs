use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

/// A reopenable input prepared by the application layer.
pub struct InputSource {
    path: PathBuf,
    label: String,
    _temp: Option<NamedTempFile>,
}

impl InputSource {
    #[cfg(test)]
    pub(crate) fn from_arg(arg: &str, literal: Option<&str>) -> Result<Self> {
        assert!(literal.is_none(), "core tests construct file-backed inputs");
        Self::from_path(arg, arg)
    }

    /// Use an existing file as an input source.
    pub fn from_path(path: impl Into<PathBuf>, label: impl Into<String>) -> Result<Self> {
        let path = path.into();
        if !path.exists() {
            bail!("input does not exist: {}", path.display());
        }
        if !path.is_file() {
            bail!("input is not a file: {}", path.display());
        }

        Ok(Self {
            path,
            label: label.into(),
            _temp: None,
        })
    }

    /// Keep a temporary input alive for the lifetime of this source.
    pub fn from_temp(temp: NamedTempFile, label: impl Into<String>) -> Self {
        Self {
            path: temp.path().to_owned(),
            label: label.into(),
            _temp: Some(temp),
        }
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
