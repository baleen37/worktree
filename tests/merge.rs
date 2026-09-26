#[path = "support/wt_command.rs"]
mod wt_command;

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use predicates::prelude::*;
use tempfile::TempDir;
use wt_command::TestWt;

struct Repo {
    _temp: TempDir,
    primary: PathBuf,
    source: PathBuf,
    wt: TestWt,
}

impl Repo {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let primary = root.join("primary");
        let source = root.join("source");
        git(&root, &["init", "-b", "main", primary.to_str().unwrap()]);
        std::fs::write(primary.join("README.md"), "initial\n").unwrap();
        git(&primary, &["add", "."]);
        commit(&primary, "initial");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feature/merge",
                source.to_str().unwrap(),
            ],
        );
        Self {
            _temp: temp,
            primary,
            source,
            wt: TestWt::new(),
        }
    }

    fn branch_exists(&self, branch: &str) -> bool {
        let reference = format!("refs/heads/{branch}");
        ProcessCommand::new("git")
            .current_dir(&self.primary)
            .args(["show-ref", "--verify", "--quiet", &reference])
            .status()
            .unwrap()
            .success()
    }

    fn add_target(&self, branch: &str) -> PathBuf {
        let target = self.primary.parent().unwrap().join("target");
        git(
            &self.primary,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                target.to_str().unwrap(),
                "main",
            ],
        );
        target
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit(cwd: &Path, message: &str) {
    git(
        cwd,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            message,
        ],
    );
}

#[test]
fn merge_fast_forwards_to_default_branch_and_cleans_up_source() {
    let repo = Repo::new();
    std::fs::write(repo.source.join("feature.txt"), "merged\n").unwrap();
    git(&repo.source, &["add", "."]);
    commit(&repo.source, "feature change");

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge"])
        .assert()
        .success()
        .stdout(format!("{}\n", repo.primary.display()));

    assert!(!repo.source.exists());
    assert!(!repo.branch_exists("feature/merge"));
    assert_eq!(
        std::fs::read_to_string(repo.primary.join("feature.txt")).unwrap(),
        "merged\n"
    );
}

#[test]
fn merge_creates_a_regular_merge_commit_in_the_specified_target() {
    let repo = Repo::new();
    let target = repo.add_target("release");
    std::fs::write(repo.source.join("feature.txt"), "feature\n").unwrap();
    git(&repo.source, &["add", "."]);
    commit(&repo.source, "feature change");
    std::fs::write(target.join("release.txt"), "release\n").unwrap();
    git(&target, &["add", "."]);
    commit(&target, "release change");

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge", "release"])
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    let output = ProcessCommand::new("git")
        .current_dir(&target)
        .args(["rev-list", "--parents", "-n", "1", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .count(),
        3
    );
    assert!(!repo.source.exists());
    assert!(!repo.branch_exists("feature/merge"));
    assert_eq!(
        std::fs::read_to_string(target.join("feature.txt")).unwrap(),
        "feature\n"
    );
}

#[test]
fn merge_writes_the_target_path_for_shell_integration() {
    let repo = Repo::new();
    let path_file = repo.primary.parent().unwrap().join("shell-path");

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge"])
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .success()
        .stdout("");

    assert_eq!(
        std::fs::read_to_string(path_file).unwrap(),
        format!("{}\n", repo.primary.display())
    );
}

#[test]
fn merge_rejects_a_dirty_source_or_target_without_removing_worktrees() {
    for dirty_target in [false, true] {
        let repo = Repo::new();
        let dirty_worktree = if dirty_target {
            &repo.primary
        } else {
            &repo.source
        };
        std::fs::write(dirty_worktree.join("uncommitted.txt"), "keep\n").unwrap();

        repo.wt
            .command()
            .current_dir(&repo.source)
            .args(["merge"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("dirty"));

        assert!(repo.source.exists());
        assert!(repo.branch_exists("feature/merge"));
        assert_eq!(
            std::fs::read_to_string(dirty_worktree.join("uncommitted.txt")).unwrap(),
            "keep\n"
        );
    }
}

#[test]
fn merge_rejects_a_target_branch_that_is_not_checked_out() {
    let repo = Repo::new();
    git(&repo.primary, &["branch", "release"]);

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge", "release"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not checked out in a worktree"));

    assert!(repo.source.exists());
    assert!(repo.branch_exists("feature/merge"));
    assert!(repo.branch_exists("release"));
}

#[test]
fn merge_rejects_the_current_branch_as_target() {
    let repo = Repo::new();

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge", "feature/merge"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already the merge target"));

    assert!(repo.source.exists());
    assert!(repo.branch_exists("feature/merge"));
}

#[test]
fn merge_conflict_keeps_both_worktrees_and_source_branch() {
    let repo = Repo::new();
    std::fs::write(repo.source.join("README.md"), "source\n").unwrap();
    git(&repo.source, &["add", "README.md"]);
    commit(&repo.source, "source change");
    std::fs::write(repo.primary.join("README.md"), "target\n").unwrap();
    git(&repo.primary, &["add", "README.md"]);
    commit(&repo.primary, "target change");
    let path_file = repo.primary.parent().unwrap().join("shell-path");
    std::fs::write(&path_file, "unchanged\n").unwrap();

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge"])
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("CONFLICT"));

    assert!(repo.source.exists());
    assert!(repo.branch_exists("feature/merge"));
    assert!(
        std::fs::read_to_string(repo.primary.join("README.md"))
            .unwrap()
            .contains("<<<<<<<")
    );
    assert_eq!(std::fs::read_to_string(path_file).unwrap(), "unchanged\n");
}

#[test]
fn merge_deletes_source_branch_even_when_its_upstream_lacks_the_merged_commit() {
    let repo = Repo::new();
    let target = repo.add_target("release");
    git(&repo.primary, &["branch", "upstream"]);
    git(
        &repo.primary,
        &["branch", "--set-upstream-to", "upstream", "feature/merge"],
    );
    std::fs::write(repo.source.join("feature.txt"), "feature\n").unwrap();
    git(&repo.source, &["add", "."]);
    commit(&repo.source, "feature change");
    std::fs::write(target.join("release.txt"), "release\n").unwrap();
    git(&target, &["add", "."]);
    commit(&target, "release change");

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge", "release"])
        .assert()
        .success();

    assert!(!repo.source.exists());
    assert!(!repo.branch_exists("feature/merge"));
}

#[test]
fn merge_refuses_to_remove_a_source_worktree_that_contains_another_worktree() {
    let repo = Repo::new();
    std::fs::write(repo.source.join("feature.txt"), "feature\n").unwrap();
    git(&repo.source, &["add", "."]);
    commit(&repo.source, "feature change");
    let target = repo.source.join("nested-target");
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "release",
            target.to_str().unwrap(),
            "main",
        ],
    );

    repo.wt
        .command()
        .current_dir(&repo.source)
        .args(["merge", "release"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "contains another registered worktree",
        ));

    assert!(repo.source.exists());
    assert!(target.exists());
    assert!(repo.branch_exists("feature/merge"));
    assert!(repo.branch_exists("release"));
    let output = ProcessCommand::new("git")
        .current_dir(&repo.primary)
        .args([
            "merge-base",
            "--is-ancestor",
            "refs/heads/feature/merge",
            "refs/heads/release",
        ])
        .status()
        .unwrap();
    assert!(!output.success());
}
