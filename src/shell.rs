use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const START: &str = "# >>> wt shell integration >>>";
const END: &str = "# <<< wt shell integration <<<";
const ADDED_SEPARATOR: &str = "# wt shell integration: added separator newline";

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Shell {
    Zsh,
    Bash,
    Fish,
}

pub(crate) fn init(shell: Shell) -> String {
    match shell {
        Shell::Zsh => include_str!("shell/zsh.sh"),
        Shell::Bash => include_str!("shell/bash.sh"),
        Shell::Fish => include_str!("shell/fish.fish"),
    }
    .to_owned()
}

fn rc_files() -> Result<[(PathBuf, &'static str); 3]> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let fish_config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("fish/config.fish");
    Ok([
        (home.join(".zshrc"), "eval \"$(wt config shell init zsh)\""),
        (
            home.join(".bashrc"),
            "eval \"$(wt config shell init bash)\"",
        ),
        (fish_config, "wt config shell init fish | source"),
    ])
}

fn without_managed_block(text: &str) -> Result<String> {
    let mut output = text.to_owned();
    while let Some(start) = output.find(START) {
        let has_added_separator =
            output[start + START.len()..].starts_with(&format!("\n{ADDED_SEPARATOR}\n"));
        let block_start =
            if has_added_separator && start > 0 && output.as_bytes()[start - 1] == b'\n' {
                start - 1
            } else {
                start
            };
        let end = output[start..]
            .find(END)
            .context("incomplete wt shell integration block")?
            + start
            + END.len();
        let end = if output[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        output.replace_range(block_start..end, "");
    }
    if output.contains(END) {
        bail!("incomplete wt shell integration block");
    }
    Ok(output)
}

fn update(install: bool) -> Result<()> {
    for (path, command) in rc_files()? {
        if !install && !path.exists() {
            continue;
        }
        let existing = if path.exists() {
            std::fs::read_to_string(&path)
                .with_context(|| format!("could not read {}", path.display()))?
        } else {
            String::new()
        };
        let mut output = without_managed_block(&existing)?;
        if install {
            let needs_separator = !output.is_empty() && !output.ends_with('\n');
            if needs_separator {
                output.push('\n');
            }
            output.push_str(START);
            output.push('\n');
            if needs_separator {
                output.push_str(ADDED_SEPARATOR);
                output.push('\n');
            }
            output.push_str(&format!("{command}\n{END}\n"));
        }
        if output != existing {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, output)
                .with_context(|| format!("could not write {}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn install() -> Result<()> {
    update(true)
}

pub(crate) fn uninstall() -> Result<()> {
    update(false)
}

pub(crate) fn write_path(path_file: Option<&Path>, worktree_path: &Path) -> Result<()> {
    if let Some(path_file) = path_file {
        std::fs::write(path_file, format!("{}\n", worktree_path.display()))
            .with_context(|| format!("could not write shell path file {}", path_file.display()))?;
    }
    Ok(())
}
