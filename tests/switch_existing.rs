#[path = "support/git_repo.rs"]
mod git_repo;
#[path = "support/wt_command.rs"]
mod wt_command;

use git_repo::{GitRepo, git};
use predicates::prelude::*;
use wt_command::TestWt;

#[test]
fn reuses_registered_worktree_and_writes_shell_path() {
    let repo = GitRepo::new();
    let wt = TestWt::new();
    let path_file = repo.primary.parent().unwrap().join("shell-path");

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/list"])
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .success()
        .stdout("");

    assert_eq!(
        std::fs::read_to_string(path_file).unwrap(),
        format!("{}\n", repo.linked.display())
    );
}

#[test]
fn creates_worktree_for_existing_local_branch_and_prints_path() {
    let repo = GitRepo::new();
    let wt = TestWt::new();
    git(&repo.primary, &["branch", "feature/new"]);
    let target = repo.primary.join(".worktrees/feature-new");

    wt.command()
        .current_dir(&repo.linked)
        .args(["switch", "feature/new"])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    assert!(target.is_dir());
    let branch = std::process::Command::new("git")
        .current_dir(&target)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(branch.stdout).unwrap(), "feature/new\n");
}

#[test]
fn unknown_branch_does_not_create_branch_or_directory() {
    let repo = GitRepo::new();
    let wt = TestWt::new();
    let target = repo.primary.join(".worktrees/feature-missing");

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown local branch"));

    assert!(!target.exists());
    assert!(!repo.primary.join(".worktrees").exists());
}

#[test]
fn occupied_normalized_path_is_preserved() {
    let repo = GitRepo::new();
    let wt = TestWt::new();
    git(&repo.primary, &["branch", "feature/new"]);
    let target = repo.primary.join(".worktrees/feature-new");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep"), "untouched").unwrap();

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    assert_eq!(
        std::fs::read_to_string(target.join("keep")).unwrap(),
        "untouched"
    );
}
