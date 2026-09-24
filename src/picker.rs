use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use dialoguer::FuzzySelect;

use crate::git::RepoContext;
use crate::shell::write_path;
use crate::worktree::WorktreeInfo;

pub(crate) fn select(
    repo: &RepoContext,
    shell_path_file: Option<&Path>,
) -> Result<Option<PathBuf>> {
    if !console::Term::stderr().is_term() {
        bail!("worktree picker requires a TTY");
    }
    let entries = WorktreeInfo::list(repo)?;
    let labels: Vec<_> = entries
        .iter()
        .map(|entry| {
            format!(
                "{}  {}",
                entry.path.display(),
                entry.branch.as_deref().unwrap_or("(detached HEAD)")
            )
        })
        .collect();
    let choice = FuzzySelect::new().items(&labels).interact_opt()?;
    let selected = choice.map(|index| entries[index].path.clone());
    if let Some(path) = &selected {
        write_path(shell_path_file, path)?;
    }
    Ok(selected)
}
