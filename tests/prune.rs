#[path = "support/wt_command.rs"]
mod wt_command;

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use expectrl::{Eof, Expect, Session};
use tempfile::TempDir;
use wt_command::TestWt;

struct Repo {
    _temp: Option<TempDir>,
    root: PathBuf,
    primary: PathBuf,
    wt: TestWt,
}

impl Repo {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        Self::in_folder(root, Some(temp))
    }

    fn in_folder(root: PathBuf, temp: Option<TempDir>) -> Self {
        std::fs::create_dir_all(&root).unwrap();
        let primary = root.join("main checkout");
        git(&root, &["init", "-b", "main", primary.to_str().unwrap()]);
        std::fs::write(primary.join(".git/info/exclude"), "/.worktrees/\n").unwrap();
        git(
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
        );
        Self {
            _temp: temp,
            root,
            primary,
            wt: TestWt::new(),
        }
    }

    fn candidate_path(&self, name: &str) -> PathBuf {
        self.root.join(".worktrees").join(name.replace('/', "-"))
    }

    fn worktree(&self, branch: &str, path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        git(
            &self.primary,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                path.to_str().unwrap(),
                "main",
            ],
        );
    }

    fn add_candidate(&self, branch: &str) -> PathBuf {
        let path = self.candidate_path(branch);
        self.worktree(branch, &path);
        path
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_from(&self.root, args)
    }

    fn run_from(&self, folder: &Path, args: &[&str]) -> std::process::Output {
        self.wt
            .command()
            .current_dir(folder)
            .args(args)
            .output()
            .unwrap()
    }

    fn pty(&self, folder: &Path, args: &[&str]) -> expectrl::session::OsSession {
        let mut command = self.wt.process_command();
        command.current_dir(folder).args(args);
        let mut session = Session::spawn(command).unwrap();
        session.set_expect_timeout(Some(std::time::Duration::from_secs(5)));
        session
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

fn unmerged_change(path: &Path, name: &str) {
    std::fs::write(path.join(name), "unmerged\n").unwrap();
    git(path, &["add", name]);
    commit(path, name);
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    ProcessCommand::new("git")
        .current_dir(repo)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .unwrap()
        .success()
}

/// The line `wt prune` prints for a removal candidate, relative to the scan root.
fn listed(root: &Path, path: &Path) -> String {
    format!("- {}\n", path.strip_prefix(root).unwrap().display())
}

/// The line for a candidate the default prune removes, with the reason it gives.
fn listed_as(root: &Path, path: &Path, reason: &str) -> String {
    format!(
        "- {}  {reason}\n",
        path.strip_prefix(root).unwrap().display()
    )
}

/// A linked worktree whose branch gained a commit that is now merged into main.
fn merged_candidate(repo: &Repo, branch: &str) -> PathBuf {
    let path = repo.add_candidate(branch);
    unmerged_change(&path, &branch.replace('/', "-"));
    git(&repo.primary, &["merge", "--ff-only", branch]);
    path
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prune_scans_multiple_repositories_from_a_non_git_directory_and_keeps_outside_worktrees() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let scope = root.join("scope");
    let first = Repo::in_folder(scope.join("first"), None);
    let second = Repo::in_folder(scope.join("second"), None);
    let first_linked = first.add_candidate("feature/first");
    let second_linked = second.add_candidate("feature/second");
    let outside = root.join("outside worktree");
    first.worktree("feature/outside", &outside);

    let output = first.run_from(&scope, &["prune", "--all", "--yes"]);

    assert_success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains(&listed(&scope, &first_linked)), "{text}");
    assert!(text.contains(&listed(&scope, &second_linked)), "{text}");
    assert!(!first_linked.exists());
    assert!(!second_linked.exists());
    assert!(first.primary.exists());
    assert!(second.primary.exists());
    assert!(outside.exists());
    assert!(branch_exists(&first.primary, "feature/first"));
    assert!(branch_exists(&second.primary, "feature/second"));
}

#[test]
fn prune_skips_missing_gitdir_marker_and_removes_valid_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let scope = root.join("scope");
    let repo = Repo::in_folder(scope.join("repo"), None);
    let candidate = repo.add_candidate("feature/candidate");
    let orphan = scope.join("orphan-worktree");
    std::fs::create_dir_all(&orphan).unwrap();
    let missing_gitdir = root.join("missing-repo/.git/worktrees/orphan-worktree");
    std::fs::write(
        orphan.join(".git"),
        format!("gitdir: {}\n", missing_gitdir.display()),
    )
    .unwrap();

    let output = repo.run_from(&scope, &["prune", "--all", "--yes"]);

    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("orphan-worktree"));
    assert!(!candidate.exists());
    assert!(orphan.exists());
}

#[test]
fn default_keeps_worktrees_in_use_and_all_removes_clean_ones() {
    let repo = Repo::new();
    let young = repo.add_candidate("feature/young");
    let unmerged = repo.add_candidate("feature/unmerged");
    let detached = repo.candidate_path("detached");
    std::fs::create_dir_all(detached.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "--detach",
            detached.to_str().unwrap(),
            "main",
        ],
    );
    unmerged_change(&unmerged, "committed-change");
    let dirty = repo.add_candidate("feature/dirty");
    std::fs::write(dirty.join("untracked"), "keep\n").unwrap();

    let default = repo.run(&["prune", "--yes"]);

    assert_success(&default);
    let text = String::from_utf8(default.stdout).unwrap();
    assert!(text.contains("nothing to remove"), "{text}");
    assert!(young.exists() && unmerged.exists() && detached.exists());

    let all = repo.run(&["prune", "--all", "--yes"]);

    assert_success(&all);
    let text = String::from_utf8(all.stdout).unwrap();
    assert!(text.contains(&listed(&repo.root, &young)), "{text}");
    assert!(text.contains(&listed(&repo.root, &unmerged)), "{text}");
    assert!(text.contains(&listed(&repo.root, &detached)), "{text}");
    assert!(!young.exists());
    assert!(!unmerged.exists());
    assert!(!detached.exists());
    assert!(dirty.exists());
    assert!(repo.primary.exists());
    assert!(branch_exists(&repo.primary, "feature/unmerged"));
}

#[test]
fn default_removes_merged_and_gone_upstream_worktrees() {
    let repo = Repo::new();
    let merged = merged_candidate(&repo, "feature/merged");
    let remote = repo.root.join("remote.git");
    git(
        &repo.root,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &repo.primary,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let gone = repo.add_candidate("feature/gone");
    unmerged_change(&gone, "pushed-change");
    git(&gone, &["push", "-u", "origin", "feature/gone"]);
    git(
        &repo.primary,
        &["push", "origin", "--delete", "feature/gone"],
    );
    git(&repo.primary, &["fetch", "--prune", "origin"]);

    let output = repo.run(&["prune", "--yes"]);

    assert_success(&output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(&listed_as(&repo.root, &merged, "merged")),
        "{text}"
    );
    assert!(
        text.contains(&listed_as(&repo.root, &gone, "merged")),
        "{text}"
    );
    assert!(!merged.exists());
    assert!(!gone.exists());
}

#[test]
fn default_keeps_merged_worktree_open_in_a_process() {
    let repo = Repo::new();
    let merged = merged_candidate(&repo, "feature/open");
    let mut shell = ProcessCommand::new("sleep")
        .arg("30")
        .current_dir(&merged)
        .spawn()
        .unwrap();

    let output = repo.run(&["prune", "--yes"]);

    shell.kill().unwrap();
    shell.wait().unwrap();
    assert_success(&output);
    assert!(merged.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("nothing to remove"));
}

#[test]
fn non_tty_previews_without_removing_and_stale_alias_runs_default_cleanup() {
    let repo = Repo::new();
    let candidate = merged_candidate(&repo, "feature/candidate");

    let preview = repo.run(&["prune"]);
    assert_success(&preview);
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(
        text.contains(&listed_as(&repo.root, &candidate, "merged")),
        "{text}"
    );
    assert!(text.contains("dry run"), "{text}");
    assert!(candidate.exists());

    let stale = repo.run(&["prune", "--stale", "--yes"]);
    assert_success(&stale);
    assert!(String::from_utf8_lossy(&stale.stderr).contains("deprecated"));
    assert!(!candidate.exists());
}

#[test]
fn dirty_worktrees_need_all_and_force_and_keep_the_branch() {
    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/dirty");
    std::fs::write(candidate.join("untracked"), "discard\n").unwrap();

    for options in [&["--force"][..], &["--all"][..]] {
        let output = repo.run(&[&["prune", "--yes"][..], options].concat());
        assert_success(&output);
        assert!(candidate.exists(), "{options:?}");
    }

    let output = repo.run(&["prune", "--all", "--force", "--yes"]);

    assert_success(&output);
    assert!(!candidate.exists());
    assert!(branch_exists(&repo.primary, "feature/dirty"));
}

#[test]
fn locked_worktree_requires_force_twice() {
    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/locked");
    git(
        &repo.primary,
        &["worktree", "lock", candidate.to_str().unwrap()],
    );

    let force_once = repo.run(&["prune", "--all", "-f", "--yes"]);
    assert_success(&force_once);
    assert!(candidate.exists());
    assert!(String::from_utf8_lossy(&force_once.stdout).contains("keep 2 "));

    let force_twice = repo.run(&["prune", "--all", "-ff", "--yes"]);
    assert_success(&force_twice);
    assert!(!candidate.exists());
    assert!(branch_exists(&repo.primary, "feature/locked"));
}

#[test]
fn nested_worktrees_are_removed_child_first_and_dirty_children_keep_the_parent() {
    let repo = Repo::new();
    let outer = repo.add_candidate("feature/outer");
    let nested = outer.join(".worktrees/nested");
    repo.worktree("feature/nested", &nested);
    std::fs::write(nested.join("untracked"), "keep\n").unwrap();

    let normal = repo.run(&["prune", "--all", "--yes"]);
    assert_success(&normal);
    assert!(outer.exists());
    assert!(nested.exists());
    assert!(String::from_utf8_lossy(&normal.stdout).contains("nothing to remove"));

    let forced = repo.run(&["prune", "--all", "--force", "--yes"]);
    assert_success(&forced);
    let text = String::from_utf8(forced.stdout).unwrap();
    let nested_removal = text.find(&listed(&repo.root, &nested)).unwrap();
    let outer_removal = text.find(&listed(&repo.root, &outer)).unwrap();
    assert!(nested_removal < outer_removal, "{text}");
    assert!(!nested.exists());
    assert!(!outer.exists());
    assert!(branch_exists(&repo.primary, "feature/outer"));
    assert!(branch_exists(&repo.primary, "feature/nested"));
}

#[test]
fn parent_containing_a_nested_primary_checkout_is_kept() {
    let repo = Repo::new();
    let outer = repo.add_candidate("feature/outer");
    let nested_repo = Repo::in_folder(outer.join(".worktrees/nested-repo"), None);

    let output = repo.run(&["prune", "--all", "--yes"]);

    assert_success(&output);
    assert!(outer.exists());
    assert!(nested_repo.primary.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("nothing to remove"));
}

#[test]
fn scan_does_not_follow_symbolic_links() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let repo = Repo::in_folder(root.join("scope"), None);
    let invalid_repo = root.join("invalid-repo");
    std::fs::create_dir(&invalid_repo).unwrap();
    std::fs::write(invalid_repo.join(".git"), "not a git directory\n").unwrap();
    symlink(&invalid_repo, repo.root.join(".hidden-link")).unwrap();

    let output = repo.run(&["prune", "--all", "--yes"]);

    assert_success(&output);
    assert!(invalid_repo.exists());
}

#[test]
fn unreadable_directory_outside_candidates_is_skipped_with_warning() {
    use std::os::unix::fs::PermissionsExt;

    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/candidate");
    let unreadable = repo.root.join("a-unreadable");
    std::fs::create_dir(&unreadable).unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&unreadable).is_ok() {
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }

    let output = repo.run(&["prune", "--all", "--yes"]);

    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("warning: skipped 1 unreadable directory: a-unreadable"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!candidate.exists());
}

#[test]
fn candidate_containing_unreadable_directory_is_kept() {
    use std::os::unix::fs::PermissionsExt;

    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/candidate");
    let unreadable = candidate.join("a-unreadable");
    std::fs::create_dir(&unreadable).unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&unreadable).is_ok() {
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }

    let output = repo.run(&["prune", "--all", "--yes", "--force"]);

    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_success(&output);
    assert!(candidate.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("nothing to remove"));
}

#[test]
fn worktree_registration_error_prevents_all_removals() {
    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/candidate");
    let real_git = ProcessCommand::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let fake_dir = repo.root.join("fake-bin");
    std::fs::create_dir(&fake_dir).unwrap();
    let fake_git = fake_dir.join("git");
    std::fs::write(
        &fake_git,
        "#!/bin/sh\nif [ \"$1\" = worktree ] && [ \"$2\" = list ]; then echo injected-failure >&2; exit 128; fi\nexec \"$WT_REAL_GIT\" \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = repo
        .wt
        .command()
        .current_dir(&repo.root)
        .args(["prune", "--all", "--yes"])
        .env("PATH", repo.wt.path_with(&fake_dir))
        .env(
            "WT_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        )
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("injected-failure"));
    assert!(candidate.exists());
}

#[test]
fn status_failure_keeps_worktree_without_force() {
    let repo = Repo::new();
    let candidate = repo.add_candidate("feature/candidate");
    let real_git = ProcessCommand::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let fake_dir = repo.root.join("fake-bin");
    std::fs::create_dir(&fake_dir).unwrap();
    let fake_git = fake_dir.join("git");
    std::fs::write(
        &fake_git,
        "#!/bin/sh\nif [ \"$1\" = status ]; then echo injected-failure >&2; exit 128; fi\nexec \"$WT_REAL_GIT\" \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = repo
        .wt
        .command()
        .current_dir(&repo.root)
        .args(["prune", "--all", "--yes"])
        .env("PATH", repo.wt.path_with(&fake_dir))
        .env(
            "WT_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        )
        .output()
        .unwrap();

    assert_success(&output);
    assert!(candidate.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("keep 2 "));
}

#[test]
fn invocation_worktree_is_kept() {
    let repo = Repo::new();
    let current = repo.add_candidate("feature/current");

    let output = repo.run_from(&current, &["prune", "--all", "--force", "--yes"]);

    assert_success(&output);
    assert!(current.exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("keep 1 "));
}

#[test]
fn confirmation_accepts_y_and_cancel_preserves_candidates() {
    for (key, should_remove) in [("y", true), ("n", false)] {
        let repo = Repo::new();
        let candidate = repo.add_candidate("feature/candidate");
        let mut session = repo.pty(&repo.root, &["prune", "--all"]);
        session.expect("[y/N]").unwrap();
        session.send(key).unwrap();
        session.expect(Eof).unwrap();
        assert_eq!(candidate.exists(), !should_remove, "key {key}");
    }
}

#[test]
fn new_nested_worktree_during_confirmation_preserves_parent_and_child() {
    let repo = Repo::new();
    let outer = repo.add_candidate("feature/outer");
    let nested = outer.join(".worktrees/added-during-confirmation");
    let mut session = repo.pty(&repo.root, &["prune", "--all"]);
    session.expect("[y/N]").unwrap();
    repo.worktree("feature/new-child", &nested);
    session.send("y").unwrap();
    let result = session.expect(Eof).unwrap();

    let output = String::from_utf8_lossy(result.before());
    assert!(!output.contains("error:"), "{output}");
    assert!(outer.exists());
    assert!(nested.exists());
    assert!(branch_exists(&repo.primary, "feature/new-child"));
}
