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
    base: PathBuf,
    feature: PathBuf,
    wt: TestWt,
}

impl Repo {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let primary = root.join("primary");
        let base = root.join("base");
        let feature = root.join("feature");
        git(&root, &["init", "-b", "parking", primary.to_str().unwrap()]);
        std::fs::write(primary.join("README.md"), "initial\n").unwrap();
        git(&primary, &["add", "."]);
        commit(&primary, "initial");
        git(&primary, &["branch", "main"]);
        git(
            &primary,
            &["worktree", "add", base.to_str().unwrap(), "main"],
        );
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feature/remove",
                feature.to_str().unwrap(),
                "main",
            ],
        );
        Self {
            _temp: temp,
            primary,
            base,
            feature,
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
fn current_worktree_removal_returns_base_path_and_deletes_merged_branch() {
    let repo = Repo::new();
    std::fs::write(repo.feature.join("change"), "merged\n").unwrap();
    git(&repo.feature, &["add", "."]);
    commit(&repo.feature, "feature change");
    git(
        &repo.base,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "merge",
            "--no-ff",
            "-m",
            "merge feature",
            "feature/remove",
        ],
    );
    let path_file = repo.primary.parent().unwrap().join("shell-path");
    repo.wt
        .command()
        .current_dir(&repo.feature)
        .arg("remove")
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .success()
        .stdout("");
    assert!(!repo.feature.exists());
    assert!(!repo.branch_exists("feature/remove"));
    assert_eq!(
        std::fs::read_to_string(path_file).unwrap(),
        format!("{}\n", repo.base.display())
    );
    assert!(repo.primary.exists());
}

#[test]
fn explicit_other_worktree_removal_preserves_shell_path_file() {
    let repo = Repo::new();
    let path_file = repo.primary.parent().unwrap().join("shell-path");
    std::fs::write(&path_file, "unchanged\n").unwrap();
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", "feature/remove"])
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .success()
        .stdout("");
    assert!(!repo.feature.exists());
    assert_eq!(std::fs::read_to_string(path_file).unwrap(), "unchanged\n");
}

#[test]
fn explicit_path_removes_only_that_worktree() {
    let repo = Repo::new();
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", repo.feature.to_str().unwrap()])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout("");
    assert!(!repo.feature.exists());
    assert!(repo.base.exists());
    assert!(repo.primary.exists());
}

#[test]
fn branch_name_takes_precedence_over_a_same_named_path() {
    let repo = Repo::new();
    let misleading_path = repo.base.join("feature/remove");
    std::fs::create_dir_all(&misleading_path).unwrap();
    std::fs::write(misleading_path.join("keep"), "untouched\n").unwrap();
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", "feature/remove"])
        .assert()
        .success();
    assert!(!repo.feature.exists());
    assert_eq!(
        std::fs::read_to_string(misleading_path.join("keep")).unwrap(),
        "untouched\n"
    );
}

#[test]
fn dirty_target_is_preserved() {
    let repo = Repo::new();
    std::fs::write(repo.feature.join("untracked"), "keep\n").unwrap();
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", "feature/remove"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("dirty"));
    assert_eq!(
        std::fs::read_to_string(repo.feature.join("untracked")).unwrap(),
        "keep\n"
    );
    assert!(repo.branch_exists("feature/remove"));
}

#[test]
fn primary_and_base_worktrees_are_preserved() {
    let repo = Repo::new();
    for target in [repo.primary.as_path(), repo.base.as_path()] {
        repo.wt
            .command()
            .current_dir(&repo.feature)
            .args(["remove", target.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                if target == repo.primary.as_path() {
                    "primary"
                } else {
                    "base"
                },
            ));
        assert!(target.exists());
    }
    assert!(repo.branch_exists("parking"));
    assert!(repo.branch_exists("main"));
}

#[test]
fn unmerged_branch_survives_successful_removal() {
    let repo = Repo::new();
    std::fs::write(repo.feature.join("change"), "unmerged\n").unwrap();
    git(&repo.feature, &["add", "."]);
    commit(&repo.feature, "feature change");
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", "feature/remove"])
        .assert()
        .success();
    assert!(!repo.feature.exists());
    assert!(repo.branch_exists("feature/remove"));
}

#[test]
fn merged_branch_is_deleted_even_with_an_unmerged_upstream() {
    let repo = Repo::new();
    git(&repo.primary, &["branch", "upstream"]);
    std::fs::write(repo.feature.join("change"), "merged\n").unwrap();
    git(&repo.feature, &["add", "."]);
    commit(&repo.feature, "feature change");
    git(
        &repo.primary,
        &["branch", "--set-upstream-to", "upstream", "feature/remove"],
    );
    git(
        &repo.base,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "merge",
            "--no-ff",
            "-m",
            "merge feature",
            "feature/remove",
        ],
    );
    repo.wt
        .command()
        .current_dir(&repo.base)
        .args(["remove", "feature/remove"])
        .assert()
        .success();
    assert!(!repo.feature.exists());
    assert!(!repo.branch_exists("feature/remove"));
}
