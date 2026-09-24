use std::path::Path;

use anyhow::{Context, Result};

pub(crate) fn write_path(path_file: Option<&Path>, worktree_path: &Path) -> Result<()> {
    if let Some(path_file) = path_file {
        std::fs::write(path_file, format!("{}\n", worktree_path.display()))
            .with_context(|| format!("could not write shell path file {}", path_file.display()))?;
    }
    Ok(())
}
