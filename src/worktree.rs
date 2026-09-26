use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::git::{RepoContext, git_text};
use crate::integrations::{herdr::Herdr, nix_gc};
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

pub struct PruneOptions {
    pub stale: bool,
    pub yes: bool,
}

pub struct PruneOutcome {
    pub safe: usize,
    pub stale: usize,
    pub keep: usize,
    pub candidates: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PruneClass {
    Safe,
    Stale,
    Keep,
}

fn merged_into_base(repo: &RepoContext, branch: &str) -> Result<bool> {
    let branch_ref = format!("refs/heads/{branch}");
    let base_ref = format!("refs/heads/{}", repo.base_branch);
    let output = std::process::Command::new("git")
        .current_dir(&repo.primary_root)
        .args(["merge-base", "--is-ancestor", &branch_ref, &base_ref])
        .output()
        .with_context(|| format!("could not check whether {branch} is merged"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "git merge-base --is-ancestor failed for {branch}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

fn classify(repo: &RepoContext, entry: &WorktreeInfo) -> Result<PruneClass> {
    if entry.is_primary || entry.is_current || entry.branch.as_deref() == Some(&repo.base_branch) {
        return Ok(PruneClass::Keep);
    }
    let Some(branch) = entry.branch.as_deref() else {
        return Ok(PruneClass::Keep);
    };
    if !git_text(
        &entry.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty()
    {
        return Ok(PruneClass::Keep);
    }
    if merged_into_base(repo, branch)? {
        return Ok(PruneClass::Safe);
    }
    let age = SystemTime::now()
        .duration_since(std::fs::metadata(&entry.path)?.modified()?)
        .unwrap_or_default();
    if age >= Duration::from_secs(30 * 24 * 60 * 60) {
        Ok(PruneClass::Stale)
    } else {
        Ok(PruneClass::Keep)
    }
}

fn preview(repo: &RepoContext, options: &PruneOptions) -> Result<PruneOutcome> {
    let entries = WorktreeInfo::list(repo)?;
    let classes = entries
        .iter()
        .map(|entry| classify(repo, entry))
        .collect::<Result<Vec<_>>>()?;
    let mut candidates = Vec::new();
    let mut safe = 0;
    let mut stale = 0;
    let mut keep = 0;
    for (entry, class) in entries.iter().zip(&classes) {
        let eligible = *class == PruneClass::Safe || (*class == PruneClass::Stale && options.stale);
        let contains_preserved = entries.iter().zip(&classes).any(|(other, other_class)| {
            other.path != entry.path
                && other.path.starts_with(&entry.path)
                && (*other_class == PruneClass::Keep
                    || (*other_class == PruneClass::Stale && !options.stale))
        });
        if !eligible || contains_preserved {
            keep += 1;
        } else {
            match class {
                PruneClass::Safe => safe += 1,
                PruneClass::Stale => stale += 1,
                PruneClass::Keep => unreachable!(),
            }
            candidates.push(entry.path.clone());
        }
    }
    candidates.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    Ok(PruneOutcome {
        safe,
        stale,
        keep,
        candidates,
        removed: Vec::new(),
    })
}

pub fn prune(repo: &RepoContext, options: PruneOptions) -> Result<PruneOutcome> {
    let mut outcome = preview(repo, &options)?;
    println!("safe: {}", outcome.safe);
    println!("stale: {}", outcome.stale);
    println!("keep: {}", outcome.keep);
    for path in &outcome.candidates {
        println!("remove: {}", path.display());
    }
    if outcome.candidates.is_empty() {
        return Ok(outcome);
    }
    if !options.yes {
        let term = console::Term::stdout();
        if !term.is_term() {
            println!("dry run");
            return Ok(outcome);
        }
        term.write_str("Apply? [y/N] ")?;
        if !matches!(term.read_char()?, 'y' | 'Y') {
            println!("cancelled");
            return Ok(outcome);
        }
        println!();
    }
    // Re-evaluate the whole repository after confirmation and before every removal.
    for path in outcome.candidates.clone() {
        if !preview(repo, &options)?.candidates.contains(&path) {
            continue;
        }
        let target = path.to_str().context("worktree path is not UTF-8")?;
        git_text(&repo.primary_root, &["worktree", "remove", "--", target])?;
        outcome.removed.push(path);
    }
    if !outcome.removed.is_empty() {
        nix_gc::start();
    }
    Ok(outcome)
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
    nix_gc::start();
    Ok(())
}

pub fn merge(
    repo: &RepoContext,
    target: Option<&str>,
    shell_path_file: Option<&Path>,
) -> Result<PathBuf> {
    let entries = WorktreeInfo::list(repo)?;
    let source = entries
        .iter()
        .find(|entry| entry.is_current)
        .context("current worktree is not registered")?;
    let source_branch = source
        .branch
        .as_deref()
        .context("cannot merge from a detached HEAD")?;
    if source.is_primary {
        bail!("cannot merge from the primary worktree");
    }
    if source_branch == repo.base_branch {
        bail!("cannot merge from the base branch");
    }

    let target_branch = target.unwrap_or(&repo.base_branch);
    if source_branch == target_branch {
        bail!("current branch is already the merge target: {target_branch}");
    }
    let target = entries
        .iter()
        .find(|entry| entry.branch.as_deref() == Some(target_branch))
        .with_context(|| {
            format!("target branch is not checked out in a worktree: {target_branch}")
        })?;
    if entries
        .iter()
        .any(|entry| entry.path != source.path && entry.path.starts_with(&source.path))
    {
        bail!("source worktree contains another registered worktree");
    }

    for entry in [source, target] {
        if !git_text(
            &entry.path,
            &["status", "--porcelain", "--untracked-files=all"],
        )?
        .is_empty()
        {
            bail!("worktree is dirty: {}", entry.path.display());
        }
    }

    let source_ref = format!("refs/heads/{source_branch}");
    let output = Command::new("git")
        .current_dir(&target.path)
        .args(["merge", "--no-edit", &source_ref])
        .output()
        .with_context(|| format!("could not run git merge in {}", target.path.display()))?;
    if !output.status.success() {
        let message = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        bail!("git merge failed: {}", message.trim());
    }

    let target_ref = format!("refs/heads/{target_branch}");
    let output = Command::new("git")
        .current_dir(&target.path)
        .args(["merge-base", "--is-ancestor", &source_ref, &target_ref])
        .output()
        .context("could not confirm that the merge completed")?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => bail!("source branch was not merged into target branch: {target_branch}"),
        _ => bail!(
            "could not confirm that the merge completed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }

    let target_path = target.path.clone();
    let source_path = source.path.to_str().context("worktree path is not UTF-8")?;
    std::env::set_current_dir(&target_path)
        .with_context(|| format!("could not change directory to {}", target_path.display()))?;
    git_text(
        &repo.primary_root,
        &["worktree", "remove", "--", source_path],
    )?;
    let _ = git_text(&target_path, &["branch", "--unset-upstream", source_branch]);
    git_text(&target_path, &["branch", "-d", "--", source_branch])?;
    write_path(shell_path_file, &target_path)?;
    nix_gc::start();
    Ok(target_path)
}

fn set_origin_upstream(worktree: &Path, branch: &str) -> Result<()> {
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let merge_ref = format!("refs/heads/{branch}");
    git_text(
        worktree,
        &["config", "--local", "--replace-all", &remote_key, "origin"],
    )?;
    git_text(
        worktree,
        &["config", "--local", "--replace-all", &merge_key, &merge_ref],
    )?;
    Ok(())
}

pub fn switch_existing(
    repo: &RepoContext,
    branch: &str,
    shell_path_file: Option<&Path>,
) -> Result<PathBuf> {
    let branch_ref = format!("refs/heads/{branch}");
    let (start_point, is_remote_branch) = if git_text(
        &repo.primary_root,
        &["show-ref", "--verify", "--quiet", &branch_ref],
    )
    .is_ok()
    {
        (branch.to_owned(), false)
    } else {
        let remote_ref = format!("refs/remotes/origin/{branch}");
        git_text(
            &repo.primary_root,
            &["show-ref", "--verify", "--quiet", &remote_ref],
        )
        .map_err(|_| anyhow::anyhow!("unknown local branch or cached origin branch: {branch}"))?;
        (remote_ref, true)
    };

    if let Some(entry) = WorktreeInfo::list(repo)?
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(branch))
    {
        if let Some(herdr) = Herdr::active(&repo.primary_root)? {
            herdr.open(&repo.primary_root, &entry.path)?;
        }
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
    if let Some(herdr) = Herdr::active(&repo.primary_root)? {
        let base = if is_remote_branch {
            &start_point
        } else {
            &repo.base_branch
        };
        herdr.create(&repo.primary_root, branch, base, &target)?;
        if is_remote_branch {
            set_origin_upstream(&target, branch)?;
        }
    } else if is_remote_branch {
        git_text(
            &repo.primary_root,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                "--",
                target_text,
                &start_point,
            ],
        )?;
        set_origin_upstream(&target, branch)?;
    } else {
        git_text(
            &repo.primary_root,
            &["worktree", "add", "--", target_text, branch],
        )?;
    }
    nix_gc::start();
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
    if let Some(herdr) = Herdr::active(&repo.primary_root)? {
        herdr.create(&repo.primary_root, name, &repo.base_branch, &target)?;
    } else {
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
    }
    nix_gc::start();
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
