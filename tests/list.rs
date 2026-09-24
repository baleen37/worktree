#[path = "support/git_repo.rs"]
mod git_repo;

use assert_cmd::Command;
use git_repo::{GitRepo, git};
use predicates::prelude::*;
use worktree::git::RepoContext;

#[test]
fn list_shows_primary_and_linked_worktrees_from_either_directory() {
    let repo = GitRepo::new();
    let other_repo = GitRepo::new();

    for cwd in [&repo.primary, &repo.linked] {
        let output = Command::cargo_bin("wt")
            .unwrap()
            .current_dir(cwd)
            .arg("list")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(repo.primary.to_str().unwrap()), "{stdout}");
        assert!(stdout.contains(repo.linked.to_str().unwrap()), "{stdout}");
        assert!(stdout.contains("main"), "{stdout}");
        assert!(stdout.contains("feature/list"), "{stdout}");
        assert!(
            !stdout.contains(other_repo.primary.to_str().unwrap()),
            "{stdout}"
        );
    }
}

#[test]
fn discover_uses_local_master_when_main_is_absent() {
    let repo = GitRepo::with_base("master");
    let context = RepoContext::discover(&repo.linked).unwrap();
    assert_eq!(context.base_branch, "master");
    assert_eq!(context.primary_root, repo.primary);
}

#[test]
fn list_labels_detached_head() {
    let repo = GitRepo::new();
    git(&repo.linked, &["checkout", "--detach"]);
    Command::cargo_bin("wt")
        .unwrap()
        .current_dir(&repo.linked)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("(detached HEAD)"));
}

#[test]
fn list_requires_a_git_repository() {
    let outside = tempfile::tempdir().unwrap();
    Command::cargo_bin("wt")
        .unwrap()
        .current_dir(outside.path())
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Git repository"));
}
