#[path = "support/git_repo.rs"]
mod git_repo;
#[path = "support/wt_command.rs"]
mod wt_command;

use git_repo::{GitRepo, git};
use predicates::prelude::*;
use wt_command::TestWt;

fn create_fetched_remote_branch(repo: &GitRepo, branch: &str) {
    git(&repo.primary, &["branch", branch]);
    git(&repo.primary, &["push", "origin", branch]);
    git(&repo.primary, &["branch", "-D", branch]);
    git(&repo.primary, &["fetch", "origin"]);
}

fn git_text(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

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
fn creates_tracking_worktree_from_cached_origin_branch_without_fetching() {
    let repo = GitRepo::with_origin();
    let wt = TestWt::new();
    create_fetched_remote_branch(&repo, "feature/remote");
    let unavailable_origin = repo.primary.parent().unwrap().join("missing-origin.git");
    git(
        &repo.primary,
        &[
            "remote",
            "set-url",
            "origin",
            unavailable_origin.to_str().unwrap(),
        ],
    );
    let target = repo.primary.join(".worktrees/feature-remote");

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/remote"])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    assert_eq!(
        git_text(&target, &["branch", "--show-current"]),
        "feature/remote"
    );
    assert_eq!(
        git_text(
            &target,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}"
            ],
        ),
        "origin/feature/remote"
    );
    assert_eq!(
        git_text(&target, &["rev-parse", "HEAD"]),
        git_text(
            &repo.primary,
            &["rev-parse", "refs/remotes/origin/feature/remote"],
        )
    );
}

#[test]
fn full_remote_ref_avoids_collision_with_local_origin_named_branch() {
    let repo = GitRepo::with_origin();
    let wt = TestWt::new();
    create_fetched_remote_branch(&repo, "feature/remote");
    git(&repo.primary, &["branch", "origin/feature/remote"]);
    let target = repo.primary.join(".worktrees/feature-remote");

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/remote"])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    assert_eq!(
        git_text(&target, &["branch", "--show-current"]),
        "feature/remote"
    );
    assert_eq!(
        git_text(
            &target,
            &["rev-parse", "--symbolic-full-name", "@{upstream}"],
        ),
        "refs/remotes/origin/feature/remote"
    );
    assert_eq!(
        git_text(
            &target,
            &["config", "--get", "branch.feature/remote.remote"]
        ),
        "origin"
    );
    assert_eq!(
        git_text(&target, &["config", "--get", "branch.feature/remote.merge"]),
        "refs/heads/feature/remote"
    );
}

#[test]
fn existing_local_branch_wins_over_a_newer_origin_branch() {
    let repo = GitRepo::with_origin();
    let wt = TestWt::new();
    git(&repo.primary, &["branch", "feature/preferred"]);
    let local_head = git_text(
        &repo.primary,
        &["rev-parse", "refs/heads/feature/preferred"],
    );
    std::fs::write(repo.primary.join("remote-only.txt"), "origin version\n").unwrap();
    git(&repo.primary, &["add", "remote-only.txt"]);
    git(
        &repo.primary,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "newer origin branch",
        ],
    );
    git(
        &repo.primary,
        &["push", "origin", "main:refs/heads/feature/preferred"],
    );
    git(&repo.primary, &["fetch", "origin"]);
    let target = repo.primary.join(".worktrees/feature-preferred");

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/preferred"])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    assert_eq!(git_text(&target, &["rev-parse", "HEAD"]), local_head);
    assert_ne!(
        git_text(
            &repo.primary,
            &["rev-parse", "refs/remotes/origin/feature/preferred"]
        ),
        local_head
    );
}

#[test]
fn remote_branch_path_conflict_is_preserved_without_creating_local_branch() {
    let repo = GitRepo::with_origin();
    let wt = TestWt::new();
    create_fetched_remote_branch(&repo, "feature/remote");
    let target = repo.primary.join(".worktrees/feature-remote");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep"), "untouched").unwrap();

    wt.command()
        .current_dir(&repo.primary)
        .args(["switch", "feature/remote"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    let branch = std::process::Command::new("git")
        .current_dir(&repo.primary)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/feature/remote",
        ])
        .status()
        .unwrap();
    assert!(!branch.success());
    assert_eq!(
        std::fs::read_to_string(target.join("keep")).unwrap(),
        "untouched"
    );
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
