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
        let primary = stdout
            .lines()
            .find(|line| line.contains("main"))
            .unwrap_or_else(|| panic!("missing primary worktree in {stdout}"));
        assert!(primary.ends_with("  ."), "{primary}");
        let linked = stdout
            .lines()
            .find(|line| line.contains("feature/list"))
            .unwrap_or_else(|| panic!("missing linked worktree in {stdout}"));
        assert!(linked.ends_with("linked feature list"), "{linked}");
        assert!(
            !stdout.contains(other_repo.primary.to_str().unwrap()),
            "{stdout}"
        );
    }
}

#[test]
fn list_aligns_current_branch_status_and_path_columns() {
    let repo = GitRepo::new();
    let output = Command::cargo_bin("wt")
        .unwrap()
        .current_dir(&repo.linked)
        .args(["list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut lines = stdout.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("HERE"), "{header}");
    let here_column = header.find("HERE").unwrap();
    let status_column = header.find("STATUS").unwrap();
    let path_column = header.find("PATH").unwrap();
    let current = lines
        .find(|line| line.contains("feature/list"))
        .unwrap_or_else(|| panic!("missing feature/list in {stdout}"));
    assert_eq!(current.find("[current]"), Some(here_column), "{current}");
    assert_eq!(current.find("clean"), Some(status_column), "{current}");
    assert!(
        current[path_column..].ends_with("linked feature list"),
        "{current}"
    );
}

#[test]
fn list_marks_only_current_and_dirty_worktrees() {
    let repo = GitRepo::new();
    let worktrees_root = repo.primary.join(".worktrees");
    std::fs::create_dir_all(&worktrees_root).unwrap();
    let unstaged = worktrees_root.join("unstaged");
    let untracked = worktrees_root.join("untracked");
    git(&repo.primary, &["branch", "feature/unstaged"]);
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            unstaged.to_str().unwrap(),
            "feature/unstaged",
        ],
    );
    git(&repo.primary, &["branch", "feature/untracked"]);
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            untracked.to_str().unwrap(),
            "feature/untracked",
        ],
    );
    std::fs::write(repo.primary.join("README.md"), "staged change\n").unwrap();
    git(&repo.primary, &["add", "README.md"]);
    std::fs::write(unstaged.join("README.md"), "unstaged change\n").unwrap();
    std::fs::write(untracked.join("untracked.txt"), "untracked change\n").unwrap();

    let entries = [
        (&repo.primary, "main", ".", true),
        (&repo.linked, "feature/list", "linked feature list", false),
        (&unstaged, "feature/unstaged", ".worktrees/unstaged", true),
        (
            &untracked,
            "feature/untracked",
            ".worktrees/untracked",
            true,
        ),
    ];
    for (cwd, _, _, _) in entries {
        let output = Command::cargo_bin("wt")
            .unwrap()
            .current_dir(cwd)
            .args(["list"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        for (path, branch, shown_path, is_dirty) in entries {
            let line = stdout
                .lines()
                .find(|line| line.contains(branch))
                .unwrap_or_else(|| panic!("missing worktree {} in {stdout}", path.display()));
            assert!(line.ends_with(shown_path), "{line}");
            assert_eq!(line.contains("[current]"), path == cwd, "{line}");
            assert_eq!(line.contains("dirty"), is_dirty, "{line}");
        }
    }
}

#[test]
fn list_preserves_registered_worktrees_without_an_accessible_directory() {
    let repo = GitRepo::new();
    std::fs::remove_dir_all(&repo.linked).unwrap();

    let output = Command::cargo_bin("wt")
        .unwrap()
        .current_dir(&repo.primary)
        .args(["list"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout
        .lines()
        .find(|line| line.contains("feature/list"))
        .unwrap_or_else(|| panic!("missing registered worktree in {stdout}"));
    assert!(line.ends_with("linked feature list"), "{line}");
    assert!(line.contains("feature/list"), "{line}");
    assert!(line.contains("status unavailable"), "{line}");
    assert!(!line.contains("dirty"), "{line}");
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
