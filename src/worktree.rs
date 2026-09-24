use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::git::{RepoContext, git_text};
use crate::shell::write_path;

pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_primary: bool,
    pub is_current: bool,
}

impl WorktreeInfo {
    pub fn list(repo: &RepoContext) -> Result<Vec<Self>> {
        let mut entries = parse_porcelain(&git_text(
            &repo.primary_root,
            &["worktree", "list", "--porcelain"],
        )?)?;
        for entry in &mut entries {
            entry.is_primary = entry.path == repo.primary_root;
            entry.is_current = entry.path == repo.current_root;
        }
        Ok(entries)
    }
}

pub fn remove(
    repo: &RepoContext,
    target: Option<&str>,
    shell_path_file: Option<&Path>,
) -> Result<()> {
    let entries = WorktreeInfo::list(repo)?;
    let entry = if let Some(target) = target {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.branch.as_deref() == Some(target))
        {
            entry
        } else {
            let path = std::fs::canonicalize(target)
                .with_context(|| format!("unknown worktree: {target}"))?;
            entries
                .iter()
                .find(|entry| entry.path == path)
                .with_context(|| format!("unknown worktree: {target}"))?
        }
    } else {
        entries
            .iter()
            .find(|entry| entry.is_current)
            .context("current worktree is not registered")?
    };

    if entry.is_primary {
        bail!("cannot remove primary worktree: {}", entry.path.display());
    }
    if entry.branch.as_deref() == Some(&repo.base_branch) {
        bail!("cannot remove base worktree: {}", entry.path.display());
    }
    if !git_text(
        &entry.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty()
    {
        bail!("worktree is dirty: {}", entry.path.display());
    }

    let base_path = entries
        .iter()
        .find(|entry| entry.branch.as_deref() == Some(&repo.base_branch))
        .map(|entry| &entry.path)
        .context("base branch is not checked out in a worktree")?;
    if entry.is_current {
        std::env::set_current_dir(base_path)
            .with_context(|| format!("could not change directory to {}", base_path.display()))?;
    }
    let target_path = entry.path.to_str().context("worktree path is not UTF-8")?;
    git_text(
        &repo.primary_root,
        &["worktree", "remove", "--", target_path],
    )?;

    if entry.is_current {
        write_path(shell_path_file, base_path)?;
    }

    if let Some(branch) = &entry.branch {
        let branch_ref = format!("refs/heads/{branch}");
        let base_ref = format!("refs/heads/{}", repo.base_branch);
        if git_text(
            &repo.primary_root,
            &["merge-base", "--is-ancestor", &branch_ref, &base_ref],
        )
        .is_ok()
        {
            let _ = git_text(base_path, &["branch", "--unset-upstream", branch]);
            git_text(base_path, &["branch", "-d", branch])?;
        }
    }
    Ok(())
}

pub fn switch_existing(
    repo: &RepoContext,
    branch: &str,
    shell_path_file: Option<&Path>,
) -> Result<PathBuf> {
    let branch_ref = format!("refs/heads/{branch}");
    git_text(
        &repo.primary_root,
        &["show-ref", "--verify", "--quiet", &branch_ref],
    )
    .map_err(|_| anyhow::anyhow!("unknown local branch: {branch}"))?;

    if let Some(entry) = WorktreeInfo::list(repo)?
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(branch))
    {
        write_path(shell_path_file, &entry.path)?;
        return Ok(entry.path);
    }

    let target = repo
        .primary_root
        .join(".worktrees")
        .join(branch.replace('/', "-"));
    if target.exists() {
        bail!("worktree path already exists: {}", target.display());
    }

    let target_text = target.to_str().context("worktree path is not UTF-8")?;
    git_text(
        &repo.primary_root,
        &["worktree", "add", "--", target_text, branch],
    )?;
    write_path(shell_path_file, &target)?;
    Ok(target)
}

pub fn create_branch(
    repo: &RepoContext,
    name: Option<&str>,
    shell_path_file: Option<&Path>,
) -> Result<PathBuf> {
    let base_path = WorktreeInfo::list(repo)?
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(&repo.base_branch))
        .map(|entry| entry.path)
        .context("base branch is not checked out in a worktree")?;
    if !git_text(&base_path, &["status", "--porcelain"])?.is_empty() {
        bail!("base worktree is dirty: {}", base_path.display());
    }
    git_text(&repo.primary_root, &["fetch", "origin"])?;
    git_text(
        &base_path,
        &["pull", "--ff-only", "origin", &repo.base_branch],
    )?;

    let generated;
    let name = if let Some(name) = name {
        name
    } else {
        generated = choose_random_name(repo, &mut random_name)?;
        &generated
    };
    let branch_ref = format!("refs/heads/{name}");
    if git_text(
        &repo.primary_root,
        &["show-ref", "--verify", "--quiet", &branch_ref],
    )
    .is_ok()
    {
        bail!("local branch already exists: {name}");
    }
    let target = repo
        .primary_root
        .join(".worktrees")
        .join(name.replace('/', "-"));
    if target.exists() {
        bail!("worktree path already exists: {}", target.display());
    }
    let target_text = target.to_str().context("worktree path is not UTF-8")?;
    git_text(
        &repo.primary_root,
        &[
            "worktree",
            "add",
            "-b",
            name,
            "--",
            target_text,
            &repo.base_branch,
        ],
    )?;
    write_path(shell_path_file, &target)?;
    Ok(target)
}

const ADJECTIVES: &[&str] = &[
    "snappy", "brave", "calm", "clever", "eager", "fuzzy", "gentle", "happy", "jolly", "keen",
    "lively", "mellow", "nimble", "proud", "quick", "silly", "swift", "witty", "zesty", "bold",
    "bright", "chill", "cosmic", "cozy", "crisp", "daring", "dapper", "epic", "fancy", "fierce",
    "glossy", "humble", "lucky", "mighty", "peppy", "plucky", "quirky", "royal", "sunny", "tidy",
];
const NOUNS: &[&str] = &[
    "greeting", "falcon", "otter", "panda", "harbor", "meadow", "canyon", "comet", "lantern",
    "beacon", "cipher", "nebula", "pebble", "prairie", "quartz", "ripple", "summit", "thicket",
    "tundra", "voyage", "willow", "anchor", "badger", "cactus", "dahlia", "ember", "fjord",
    "glacier", "horizon", "iris", "juniper", "kettle", "lagoon", "mango",
];
const SURNAMES: &[&str] = &[
    "bachman",
    "turing",
    "lovelace",
    "hopper",
    "knuth",
    "ritchie",
    "torvalds",
    "dijkstra",
    "kernighan",
    "stallman",
    "carmack",
    "abramov",
    "hickey",
    "armstrong",
    "rossum",
    "wall",
    "matz",
    "gosling",
    "stroustrup",
    "liskov",
    "hamilton",
    "feynman",
    "curie",
    "tesla",
    "darwin",
    "newton",
    "galileo",
    "kepler",
    "hubble",
    "sagan",
];
static NAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn random_name() -> String {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ NAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut next = || {
        seed = seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    };
    format!(
        "{}-{}-{}",
        ADJECTIVES[next() as usize % ADJECTIVES.len()],
        NOUNS[next() as usize % NOUNS.len()],
        SURNAMES[next() as usize % SURNAMES.len()]
    )
}

fn choose_random_name(repo: &RepoContext, generate: &mut impl FnMut() -> String) -> Result<String> {
    for _ in 0..5 {
        let name = generate();
        let branch_ref = format!("refs/heads/{name}");
        let branch_exists = git_text(
            &repo.primary_root,
            &["show-ref", "--verify", "--quiet", &branch_ref],
        )
        .is_ok();
        let path_exists = repo
            .primary_root
            .join(".worktrees")
            .join(name.replace('/', "-"))
            .exists();
        if !branch_exists && !path_exists {
            return Ok(name);
        }
    }
    bail!("could not find an unused worktree name after 5 attempts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_generated_collisions_stop_without_a_new_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git_text(&root, &["init", "-b", "main", "repo"]).unwrap();
        let primary = root.join("repo");
        git_text(
            &primary,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        )
        .unwrap();
        let origin = root.join("origin.git");
        git_text(&root, &["init", "--bare", origin.to_str().unwrap()]).unwrap();
        git_text(
            &primary,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        )
        .unwrap();
        git_text(&primary, &["push", "-u", "origin", "main"]).unwrap();
        for number in 0..5 {
            git_text(&primary, &["branch", &format!("collision-{number}")]).unwrap();
        }
        let repo = RepoContext {
            primary_root: primary.clone(),
            common_dir: primary.join(".git"),
            base_branch: "main".to_owned(),
            current_root: primary.clone(),
        };
        let mut count = 0;
        let result = choose_random_name(&repo, &mut || {
            let name = format!("collision-{count}");
            count += 1;
            name
        });
        assert!(result.is_err());
        assert_eq!(count, 5);
        assert!(!primary.join(".worktrees").exists());
        assert!(
            git_text(&primary, &["branch", "--list", "collision-5"])
                .unwrap()
                .is_empty()
        );
    }
}

pub(crate) fn parse_porcelain(text: &str) -> Result<Vec<WorktreeInfo>> {
    text.split("\n\n")
        .filter(|block| !block.is_empty())
        .map(|block| {
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(value));
                } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(value.to_owned());
                }
            }
            Ok(WorktreeInfo {
                path: path.context("Git worktree entry has no path")?,
                branch,
                is_primary: false,
                is_current: false,
            })
        })
        .collect()
}
