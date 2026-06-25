use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

const BEGIN_MARKER: &str = "# >>> fmtview alias >>>";
const END_MARKER: &str = "# <<< fmtview alias <<<";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AliasShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug)]
pub(crate) struct AliasCommandOptions {
    pub(crate) shell: AliasShell,
    pub(crate) install: bool,
    pub(crate) name: String,
}

pub(crate) fn run_alias_command<W: io::Write>(
    options: &AliasCommandOptions,
    output: &mut W,
) -> Result<()> {
    validate_alias_name(&options.name)?;
    let snippet = alias_snippet(options.shell, &options.name);
    if options.install {
        let path = install_alias(options.shell, &options.name, &snippet)?;
        writeln!(
            output,
            "installed {} alias in {}",
            options.name,
            path.display()
        )
        .context("failed to write alias install result")?;
    } else {
        write!(output, "{snippet}").context("failed to write alias snippet")?;
    }
    Ok(())
}

fn validate_alias_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("alias name cannot be empty");
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        bail!("alias name may only contain ASCII letters, digits, '_' or '-'");
    }
    Ok(())
}

fn alias_snippet(shell: AliasShell, name: &str) -> String {
    match shell {
        AliasShell::Bash | AliasShell::Zsh => format!("alias {name}='fmtview'\n"),
        AliasShell::Fish => format!("function {name}\n    fmtview $argv\nend\n"),
    }
}

fn install_alias(shell: AliasShell, name: &str, snippet: &str) -> Result<PathBuf> {
    let path = shell_config_path(shell)?;
    let current = fs::read_to_string(&path).unwrap_or_default();
    let managed = managed_block(snippet);
    if !contains_managed_block(&current) {
        if let Some(existing) = command_in_path(name) {
            bail!(
                "{} already exists at {}; choose another alias with --name",
                name,
                existing.display()
            );
        }
        if current
            .lines()
            .any(|line| line.trim_start().starts_with(&format!("alias {name}=")))
        {
            bail!("{name} already has a shell alias in {}", path.display());
        }
    }

    let next = upsert_managed_block(&current, &managed);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, next).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn managed_block(snippet: &str) -> String {
    format!("{BEGIN_MARKER}\n{snippet}{END_MARKER}\n")
}

fn contains_managed_block(content: &str) -> bool {
    content.contains(BEGIN_MARKER) && content.contains(END_MARKER)
}

fn upsert_managed_block(content: &str, block: &str) -> String {
    if let Some(begin) = content.find(BEGIN_MARKER) {
        if let Some(relative_end) = content[begin..].find(END_MARKER) {
            let end = begin + relative_end + END_MARKER.len();
            let mut next = String::with_capacity(content.len() + block.len());
            next.push_str(&content[..begin]);
            next.push_str(block);
            if content[end..].starts_with('\n') {
                next.push_str(&content[end + 1..]);
            } else {
                next.push_str(&content[end..]);
            }
            return next;
        }
    }

    let mut next = content.to_owned();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(block);
    next
}

fn shell_config_path(shell: AliasShell) -> Result<PathBuf> {
    match shell {
        AliasShell::Bash => Ok(home_dir()?.join(".bashrc")),
        AliasShell::Zsh => Ok(home_dir()?.join(".zshrc")),
        AliasShell::Fish => {
            let config_home = match env::var_os("XDG_CONFIG_HOME") {
                Some(path) => PathBuf::from(path),
                None => home_dir()?.join(".config"),
            };
            Ok(config_home.join("fish").join("conf.d").join("fmtview.fish"))
        }
    }
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("HOME is not set; cannot choose a shell config path"))
}

fn command_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_command_file(candidate))
}

fn is_command_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_snippet_uses_alias() {
        assert_eq!(
            alias_snippet(AliasShell::Bash, "fv"),
            "alias fv='fmtview'\n"
        );
    }

    #[test]
    fn fish_snippet_uses_function() {
        assert_eq!(
            alias_snippet(AliasShell::Fish, "fv"),
            "function fv\n    fmtview $argv\nend\n"
        );
    }

    #[test]
    fn managed_block_replaces_existing_block() {
        let current = "before\n# >>> fmtview alias >>>\nold\n# <<< fmtview alias <<<\nafter\n";
        let next = upsert_managed_block(current, &managed_block("alias fv='fmtview'\n"));
        assert_eq!(
            next,
            "before\n# >>> fmtview alias >>>\nalias fv='fmtview'\n# <<< fmtview alias <<<\nafter\n"
        );
    }
}
