use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use dialoguer::{FuzzySelect, theme::ColorfulTheme};

use crate::git::RepoContext;
use crate::shell::write_path;
use crate::worktree::{WorktreeInfo, worktree_labels};

pub(crate) fn select(
    repo: &RepoContext,
    shell_path_file: Option<&Path>,
) -> Result<Option<PathBuf>> {
    if !console::Term::stderr().is_term() {
        bail!("worktree picker requires a TTY");
    }
    let entries = WorktreeInfo::list(repo)?;
    let terminal_width = console::Term::stderr().size().1 as usize;
    let labels = worktree_labels(repo, &entries, terminal_width, 2, false);
    let prompt = console::truncate_str(
        "Switch worktree · type to filter",
        terminal_width.saturating_sub(4),
        "…",
    )
    .into_owned();
    let theme = ColorfulTheme::default();
    let choice = FuzzySelect::with_theme(&theme)
        .with_prompt(prompt)
        .items(&labels.rows)
        .interact_opt()?;
    let selected = choice.map(|index| entries[index].path.clone());
    if let Some(path) = &selected {
        write_path(shell_path_file, path)?;
    }
    Ok(selected)
}
