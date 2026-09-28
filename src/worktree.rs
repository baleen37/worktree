use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use console::{Style, measure_text_width, truncate_str};

use crate::git::{RepoContext, git_text};
use crate::integrations::{herdr::Herdr, nix};
use crate::shell::write_path;

pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_primary: bool,
    pub is_current: bool,
}

pub(crate) fn display_path(repo: &RepoContext, path: &Path) -> String {
    match path.strip_prefix(&repo.primary_root) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

pub(crate) struct WorktreeLabels {
    pub header: String,
    pub rows: Vec<String>,
}

pub(crate) fn worktree_labels(
    repo: &RepoContext,
    entries: &[WorktreeInfo],
    terminal_width: usize,
    selector_prefix: usize,
    colorize: bool,
) -> WorktreeLabels {
    let content_width = terminal_width.saturating_sub(selector_prefix);
    let current_marker = if content_width < 56 { "*" } else { "[current]" };
    let rows: Vec<_> = entries
        .iter()
        .map(|entry| {
            (
                if entry.is_current { current_marker } else { "" },
                entry.branch.as_deref().unwrap_or("(detached HEAD)"),
                entry.status(),
                display_path(repo, &entry.path),
            )
        })
        .collect();
    let here_width = measure_text_width(current_marker).max(measure_text_width("HERE"));
    let branch_limit = rows
        .iter()
        .map(|(_, branch, _, _)| measure_text_width(branch))
        .max()
        .unwrap_or(0)
        .max(measure_text_width("BRANCH"));
    let status_width = rows
        .iter()
        .map(|(_, _, status, _)| measure_text_width(status))
        .max()
        .unwrap_or(0)
        .max(measure_text_width("STATUS"));
    let branch_and_path_width = content_width.saturating_sub(here_width + 6 + status_width);
    let path_reserve = (branch_and_path_width / 2).min(16);
    let branch_width = branch_limit
        .min(32)
        .min(branch_and_path_width.saturating_sub(path_reserve).max(1));
    let path_width = branch_and_path_width.saturating_sub(branch_width);

    let header = format!(
        "{}  {}  {}  PATH",
        pad_to_width("HERE", here_width),
        pad_to_width(&truncate_middle("BRANCH", branch_width), branch_width),
        pad_to_width(&truncate_str("STATUS", status_width, "…"), status_width),
    );
    let header = truncate_str(&header, content_width, "…").into_owned();
    let header = style_cell(&header, colorize, Style::new().bold());
    let rows = rows
        .into_iter()
        .map(|(here, branch, status, path)| {
            let branch = truncate_middle(branch, branch_width);
            let path = truncate_path(&path, path_width);
            let row = format!(
                "{}  {}  {}  {}",
                style_cell(
                    &pad_to_width(here, here_width),
                    colorize,
                    Style::new().cyan().bold(),
                ),
                pad_to_width(&branch, branch_width),
                style_cell(
                    &pad_to_width(status, status_width),
                    colorize,
                    status_style(status),
                ),
                style_cell(&path, colorize, Style::new().dim()),
            );
            truncate_str(&row, content_width, "…").into_owned()
        })
        .collect();

    WorktreeLabels { header, rows }
}

fn pad_to_width(text: &str, width: usize) -> String {
    let mut padded = text.to_owned();
    padded.push_str(&" ".repeat(width.saturating_sub(measure_text_width(text))));
    padded
}

fn style_cell(text: &str, colorize: bool, style: Style) -> String {
    if colorize {
        style.apply_to(text).to_string()
    } else {
        text.to_owned()
    }
}

fn status_style(status: &str) -> Style {
    match status {
        "clean" => Style::new().green(),
        "dirty" => Style::new().yellow().bold(),
        _ => Style::new().red(),
    }
}

fn truncate_middle(text: &str, width: usize) -> String {
    if measure_text_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }

    let kept_width = width - measure_text_width("…");
    let prefix_width = kept_width.div_ceil(2);
    let prefix = truncate_str(text, prefix_width, "").into_owned();
    let suffix_width = kept_width.saturating_sub(measure_text_width(&prefix));
    let mut suffix_start = text.len();
    for (index, _) in text.char_indices().rev() {
        if measure_text_width(&text[index..]) > suffix_width {
            break;
        }
        suffix_start = index;
    }
    format!("{prefix}…{}", &text[suffix_start..])
}

fn truncate_path(path: &str, width: usize) -> String {
    if measure_text_width(path) <= width {
        return path.to_owned();
    }
    if width == 0 {
        return String::new();
    }

    let basename = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if width <= measure_text_width("…/") + 1 {
        return truncate_middle(path, width);
    }
    let basename_width = width - measure_text_width("…/");
    format!("…/{}", truncate_middle(basename, basename_width))
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

    pub(crate) fn status(&self) -> &'static str {
        match git_text(
            &self.path,
            &["status", "--porcelain", "--untracked-files=all"],
        ) {
            Ok(status) if status.is_empty() => "clean",
            Ok(_) => "dirty",
            Err(_) => "status unavailable",
        }
    }
}

pub struct PruneOptions {
    pub force: u8,
    pub yes: bool,
}

pub struct PruneOutcome {
    pub keep: usize,
    pub candidates: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

struct RegisteredWorktree {
    path: PathBuf,
    is_primary: bool,
    is_locked: bool,
}

struct PruneRepository {
    common_dir: PathBuf,
    primary_root: PathBuf,
    worktrees: Vec<RegisteredWorktree>,
}

struct PruneCandidate {
    path: PathBuf,
    common_dir: PathBuf,
    primary_root: PathBuf,
}

struct PrunePlan {
    keep: usize,
    candidates: Vec<PruneCandidate>,
    repositories: Vec<PruneRepository>,
    current_repository: Option<(PathBuf, PathBuf)>,
}

fn collect_git_roots(directory: &Path, roots: &mut Vec<PathBuf>) -> Result<()> {
    let marker = directory.join(".git");
    match std::fs::symlink_metadata(&marker) {
        Ok(metadata) if !metadata.file_type().is_symlink() => roots.push(directory.to_owned()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", marker.display()));
        }
    }

    let mut children = std::fs::read_dir(directory)
        .with_context(|| format!("could not scan {}", directory.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("could not scan {}", directory.display()))?;
    children.sort_by_key(std::fs::DirEntry::path);
    for child in children {
        let path = child.path();
        let file_type = child
            .file_type()
            .with_context(|| format!("could not inspect {}", path.display()))?;
        if !file_type.is_dir() || file_type.is_symlink() || child.file_name() == ".git" {
            continue;
        }
        collect_git_roots(&path, roots)?;
    }
    Ok(())
}

fn parse_worktree_list(text: &str, command_root: &Path) -> Result<Vec<RegisteredWorktree>> {
    let mut worktrees = Vec::new();
    let mut fields = Vec::new();
    for field in text.split('\0') {
        if field.is_empty() {
            if !fields.is_empty() {
                worktrees.push(parse_worktree_fields(
                    &fields,
                    command_root,
                    worktrees.is_empty(),
                )?);
                fields.clear();
            }
        } else {
            fields.push(field);
        }
    }
    if !fields.is_empty() {
        worktrees.push(parse_worktree_fields(
            &fields,
            command_root,
            worktrees.is_empty(),
        )?);
    }
    if worktrees.is_empty() {
        bail!("Git reported no worktrees");
    }
    Ok(worktrees)
}

fn parse_worktree_fields(
    fields: &[&str],
    command_root: &Path,
    is_primary: bool,
) -> Result<RegisteredWorktree> {
    let path = fields
        .iter()
        .find_map(|field| field.strip_prefix("worktree "))
        .map(PathBuf::from)
        .context("Git worktree entry has no path")?;
    let path = if path.is_absolute() {
        path
    } else {
        command_root.join(path)
    };
    Ok(RegisteredWorktree {
        path,
        is_primary,
        is_locked: fields
            .iter()
            .any(|field| *field == "locked" || field.starts_with("locked ")),
    })
}

fn current_repository(directory: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    for ancestor in directory.ancestors() {
        let marker = ancestor.join(".git");
        match std::fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.file_type().is_symlink() => continue,
            Ok(_) => {
                let root = PathBuf::from(git_text(ancestor, &["rev-parse", "--show-toplevel"])?);
                let common_dir = PathBuf::from(git_text(
                    ancestor,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                )?)
                .canonicalize()
                .with_context(|| {
                    format!("could not resolve Git metadata for {}", ancestor.display())
                })?;
                return Ok(Some((common_dir, root)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", marker.display()));
            }
        }
    }
    Ok(None)
}

fn scan_repositories(root: &Path) -> Result<Vec<PruneRepository>> {
    let mut roots = Vec::new();
    collect_git_roots(root, &mut roots)?;
    roots.sort();

    let mut seen = BTreeSet::new();
    let mut repositories = Vec::new();
    for command_root in roots {
        let common_dir = PathBuf::from(git_text(
            &command_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?)
        .canonicalize()
        .with_context(|| {
            format!(
                "could not resolve Git metadata for {}",
                command_root.display()
            )
        })?;
        if !seen.insert(common_dir.clone()) {
            continue;
        }
        let listing = git_text(&command_root, &["worktree", "list", "--porcelain", "-z"])
            .with_context(|| format!("could not list worktrees for {}", command_root.display()))?;
        let worktrees = parse_worktree_list(&listing, &command_root).with_context(|| {
            format!(
                "could not read worktree registrations for {}",
                command_root.display()
            )
        })?;
        let primary_root = worktrees
            .first()
            .context("Git reported no worktrees")?
            .path
            .clone();
        repositories.push(PruneRepository {
            common_dir,
            primary_root,
            worktrees,
        });
    }
    Ok(repositories)
}

fn worktree_in_scope(path: &Path, root: &Path) -> Result<Option<PathBuf>> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    if !path.starts_with(root) {
        return Ok(None);
    }
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", path.display()));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(None);
    }
    let resolved = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not resolve {}", path.display()));
        }
    };
    Ok(resolved.starts_with(root).then_some(resolved))
}

fn worktree_is_clean(path: &Path) -> Option<bool> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout.is_empty())
}

fn plan_prune(root: &Path, options: &PruneOptions) -> Result<PrunePlan> {
    let root = root
        .canonicalize()
        .with_context(|| format!("could not resolve scan directory {}", root.display()))?;
    let repositories = scan_repositories(&root)?;
    let current_repository = current_repository(&root)?;
    let mut entries = Vec::new();
    for repository in &repositories {
        for worktree in &repository.worktrees {
            let Some(path) = worktree_in_scope(&worktree.path, &root)? else {
                continue;
            };
            let is_scope_root = path == root;
            let can_remove = !worktree.is_primary
                && !is_scope_root
                && (!worktree.is_locked || options.force >= 2)
                && (options.force > 0 || worktree_is_clean(&path) == Some(true));
            entries.push((
                PruneCandidate {
                    path,
                    common_dir: repository.common_dir.clone(),
                    primary_root: repository.primary_root.clone(),
                },
                can_remove,
            ));
        }
    }

    for index in 0..entries.len() {
        if !entries[index].1 {
            continue;
        }
        let parent = &entries[index].0.path;
        if entries.iter().any(|(child, can_remove)| {
            child.path != *parent && child.path.starts_with(parent) && !can_remove
        }) {
            entries[index].1 = false;
        }
    }

    let mut candidates = Vec::new();
    let mut keep = 0;
    for (candidate, can_remove) in entries {
        if can_remove {
            candidates.push(candidate);
        } else {
            keep += 1;
        }
    }
    candidates.sort_by(|left, right| {
        right
            .path
            .components()
            .count()
            .cmp(&left.path.components().count())
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(PrunePlan {
        keep,
        candidates,
        repositories,
        current_repository,
    })
}

fn remove_worktree(candidate: &PruneCandidate, force: u8) -> Result<()> {
    let mut command = Command::new("git");
    command
        .arg("--git-dir")
        .arg(&candidate.common_dir)
        .args(["worktree", "remove"]);
    command.args(std::iter::repeat_n("--force", force.min(2) as usize));
    let output = command
        .arg("--")
        .arg(&candidate.path)
        .output()
        .with_context(|| format!("could not remove worktree {}", candidate.path.display()))?;
    if !output.status.success() {
        bail!(
            "could not remove worktree {}: {}",
            candidate.path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn prune_plan_outcome(plan: &PrunePlan) -> PruneOutcome {
    PruneOutcome {
        keep: plan.keep,
        candidates: plan
            .candidates
            .iter()
            .map(|candidate| candidate.path.clone())
            .collect(),
        removed: Vec::new(),
    }
}

pub fn prune(folder: &Path, options: PruneOptions) -> Result<PruneOutcome> {
    let mut plan = plan_prune(folder, &options)?;
    let mut outcome = prune_plan_outcome(&plan);
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

    // Re-scan after confirmation so failed discovery still happens before the first removal.
    let planned_paths = outcome.candidates.iter().cloned().collect::<BTreeSet<_>>();
    plan = plan_prune(folder, &options)?;
    let new_paths = plan
        .candidates
        .iter()
        .filter(|candidate| !planned_paths.contains(&candidate.path))
        .map(|candidate| candidate.path.clone())
        .collect::<Vec<_>>();
    let mut newly_kept = new_paths.len();
    plan.candidates.retain(|candidate| {
        let was_planned = planned_paths.contains(&candidate.path);
        let contains_new_worktree = new_paths
            .iter()
            .any(|path| path != &candidate.path && path.starts_with(&candidate.path));
        if was_planned && contains_new_worktree {
            newly_kept += 1;
        }
        was_planned && !contains_new_worktree
    });
    plan.keep += newly_kept;
    outcome.keep = plan.keep;
    let caller_path = plan
        .current_repository
        .as_ref()
        .map(|(_, path)| path.clone());
    let herdr_repo = plan
        .current_repository
        .as_ref()
        .and_then(|(common_dir, _)| {
            plan.repositories
                .iter()
                .find(|repository| repository.common_dir == *common_dir)
        });
    let herdr = match herdr_repo {
        Some(repository)
            if plan
                .candidates
                .iter()
                .any(|candidate| candidate.common_dir == repository.common_dir) =>
        {
            Herdr::active(&repository.primary_root)?
        }
        _ => None,
    };
    let caller_common_dir = plan
        .current_repository
        .as_ref()
        .map(|(common_dir, _)| common_dir);
    let mut herdr_repo_removed = false;
    for candidate in &plan.candidates {
        let belongs_to_caller = caller_common_dir == Some(&candidate.common_dir);
        let workspace_id = if belongs_to_caller {
            herdr
                .as_ref()
                .and_then(|active_herdr| active_herdr.open_workspace_id(&candidate.path))
        } else {
            None
        };
        if let (Some(active_herdr), Some(workspace_id)) = (herdr.as_ref(), workspace_id) {
            active_herdr.remove(&candidate.primary_root, workspace_id, options.force)?;
        } else {
            remove_worktree(candidate, options.force)?;
        }
        herdr_repo_removed |= belongs_to_caller;
        outcome.removed.push(candidate.path.clone());
    }
    if herdr_repo_removed && let Some(active_herdr) = herdr.as_ref() {
        let repository = herdr_repo.context("Herdr repository disappeared during prune")?;
        let caller_path = caller_path.context("Herdr caller worktree is unavailable")?;
        active_herdr.open(&repository.primary_root, &caller_path)?;
    }
    if !outcome.removed.is_empty() {
        nix::start();
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
    let caller_path = repo.current_root.clone();
    let focus_path = if entry.is_current {
        base_path.clone()
    } else {
        caller_path
    };
    let herdr = Herdr::active(&repo.primary_root)?;
    if entry.is_current {
        std::env::set_current_dir(base_path)
            .with_context(|| format!("could not change directory to {}", base_path.display()))?;
    }
    let target_path = entry.path.to_str().context("worktree path is not UTF-8")?;
    if let Some(herdr) = herdr.as_ref() {
        if let Some(workspace_id) = herdr.open_workspace_id(&entry.path) {
            herdr.remove(&repo.primary_root, workspace_id, 0)?;
        } else {
            git_text(
                &repo.primary_root,
                &["worktree", "remove", "--", target_path],
            )?;
        }
    } else {
        git_text(
            &repo.primary_root,
            &["worktree", "remove", "--", target_path],
        )?;
    }

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
    if let Some(herdr) = herdr.as_ref() {
        herdr.open(&repo.primary_root, &focus_path)?;
    }
    nix::start();
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
    let herdr = Herdr::active(&repo.primary_root)?;

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
    if let Some(herdr) = herdr.as_ref() {
        if let Some(workspace_id) = herdr.open_workspace_id(&source.path) {
            herdr.remove(&repo.primary_root, workspace_id, 0)?;
        } else {
            git_text(
                &repo.primary_root,
                &["worktree", "remove", "--", source_path],
            )?;
        }
    } else {
        git_text(
            &repo.primary_root,
            &["worktree", "remove", "--", source_path],
        )?;
    }
    let _ = git_text(&target_path, &["branch", "--unset-upstream", source_branch]);
    git_text(&target_path, &["branch", "-d", "--", source_branch])?;
    write_path(shell_path_file, &target_path)?;
    if let Some(herdr) = herdr.as_ref() {
        herdr.open(&repo.primary_root, &target_path)?;
    }
    nix::start();
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
        let herdr = Herdr::active(&repo.primary_root)?;
        if let Some(herdr) = &herdr {
            herdr.open(&repo.primary_root, &entry.path)?;
        } else {
            write_path(shell_path_file, &entry.path)?;
        }
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
    let herdr = Herdr::active(&repo.primary_root)?;
    if let Some(herdr) = &herdr {
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
    nix::start();
    if herdr.is_none() {
        write_path(shell_path_file, &target)?;
    }
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
    let herdr = Herdr::active(&repo.primary_root)?;
    if let Some(herdr) = &herdr {
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
    nix::start();
    if herdr.is_none() {
        write_path(shell_path_file, &target)?;
    }
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
    fn worktree_labels_fit_terminal_width_and_shorten_long_values() {
        let temp = tempfile::tempdir().unwrap();
        let primary_root = temp.path().join("primary");
        std::fs::create_dir(&primary_root).unwrap();
        let repo = RepoContext {
            primary_root: primary_root.clone(),
            common_dir: primary_root.join(".git"),
            base_branch: "main".to_owned(),
            current_root: primary_root.clone(),
        };
        let entries = [
            WorktreeInfo {
                path: primary_root.clone(),
                branch: Some("feature/with/a/very/long/branch/name".to_owned()),
                is_primary: true,
                is_current: true,
            },
            WorktreeInfo {
                path: temp
                    .path()
                    .join("another directory")
                    .join("linked worktree with a long name"),
                branch: Some("feature/another/long/branch/name".to_owned()),
                is_primary: false,
                is_current: false,
            },
        ];
        let labels = worktree_labels(&repo, &entries, 72, 0, false);

        assert!(
            measure_text_width(&labels.header) <= 72,
            "{}",
            labels.header
        );
        assert!(
            labels.rows.iter().all(|row| measure_text_width(row) <= 72),
            "{:#?}",
            labels.rows
        );
        assert!(labels.rows[0].contains("[current]"), "{}", labels.rows[0]);
        assert!(labels.rows[1].contains('…'), "{}", labels.rows[1]);

        let narrow = worktree_labels(&repo, &entries, 40, 0, false);
        assert!(narrow.rows[0].starts_with('*'), "{}", narrow.rows[0]);
        assert!(
            narrow.rows.iter().all(|row| measure_text_width(row) <= 40),
            "{:#?}",
            narrow.rows
        );
    }

    #[test]
    fn middle_truncation_keeps_branch_and_path_ends_visible() {
        assert_eq!(
            truncate_middle("feature/long/branch/name/for/screen-check", 28),
            "feature/long/b…/screen-check"
        );
        assert_eq!(
            truncate_path("/root/linked worktree with a very long directory name", 16),
            "…/linked …y name"
        );
    }

    #[test]
    fn worktree_list_parser_handles_spaces_and_locked_metadata() {
        let root = PathBuf::from("/tmp/repo with spaces");
        let entries = parse_worktree_list(
            "worktree /tmp/repo with spaces\0HEAD abc\0branch refs/heads/main\0\0worktree /tmp/locked worktree\0HEAD def\0detached\0locked backup drive\0\0",
            &root,
        )
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_primary);
        assert!(!entries[0].is_locked);
        assert_eq!(entries[1].path, PathBuf::from("/tmp/locked worktree"));
        assert!(!entries[1].is_primary);
        assert!(entries[1].is_locked);
    }

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
