use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

pub struct RepoContext {
    pub primary_root: PathBuf,
    pub common_dir: PathBuf,
    pub base_branch: String,
    pub(crate) current_root: PathBuf,
}

impl RepoContext {
    pub fn discover(cwd: &Path) -> Result<Self> {
        let current_root = git_text(cwd, &["rev-parse", "--show-toplevel"])
            .context("a Git repository is required for wt list")?;
        let common_dir = git_text(cwd, &["rev-parse", "--git-common-dir"])?;
        let common_dir = cwd.join(common_dir).canonicalize()?;
        let entries = crate::worktree::parse_porcelain(&git_text(
            cwd,
            &["worktree", "list", "--porcelain"],
        )?)?;
        let primary_root = entries
            .first()
            .context("Git reported no worktrees")?
            .path
            .clone();
        let base_branch = ["main", "master"]
            .into_iter()
            .find(|branch| {
                git(
                    cwd,
                    &[
                        "show-ref",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{branch}"),
                    ],
                )
                .is_ok_and(|output| output.status.success())
            })
            .context("no local main or master branch found")?
            .to_owned();

        Ok(Self {
            primary_root,
            common_dir,
            base_branch,
            current_root: PathBuf::from(current_root),
        })
    }
}

pub(crate) fn git_text(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = git(cwd, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim_end_matches('\n').to_owned())
        .context("Git output was not UTF-8")
}

fn git(cwd: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("could not run git in {}", cwd.display()))
}
