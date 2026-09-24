#[path = "support/wt_command.rs"]
mod wt_command;

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use expectrl::{Eof, Expect, Session};
use tempfile::TempDir;
use wt_command::TestWt;

struct Repo {
    _temp: TempDir,
    primary: PathBuf,
    base: PathBuf,
    wt: TestWt,
}

impl Repo {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let primary = root.join("primary");
        let base = root.join("base");
        git(&root, &["init", "-b", "parking", primary.to_str().unwrap()]);
        std::fs::write(primary.join("README.md"), "initial\n").unwrap();
        git(&primary, &["add", "."]);
        commit(&primary, "initial");
        git(&primary, &["branch", "main"]);
        git(
            &primary,
            &["worktree", "add", base.to_str().unwrap(), "main"],
        );
        Self {
            _temp: temp,
            primary,
            base,
            wt: TestWt::new(),
        }
    }

    fn worktree(&self, name: &str, path: &Path) {
        git(
            &self.primary,
            &[
                "worktree",
                "add",
                "-b",
                name,
                path.to_str().unwrap(),
                "main",
            ],
        );
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.wt
            .command()
            .current_dir(&self.base)
            .args(args)
            .output()
            .unwrap()
    }

    fn pty(&self, args: &[&str]) -> expectrl::session::OsSession {
        let mut command = self.wt.process_command();
        command.current_dir(&self.base).args(args);
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

fn old_directory(path: &Path) {
    let status = ProcessCommand::new("touch")
        .args(["-t", "202001010000", path.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn non_tty_previews_merged_clean_candidate_without_removing_it() {
    let repo = Repo::new();
    let candidate = repo.primary.parent().unwrap().join("merged");
    repo.worktree("feature/merged", &candidate);

    let output = repo.run(&["prune"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("safe: 1"), "{stdout}");
    assert!(stdout.contains("stale: 0"), "{stdout}");
    assert!(stdout.contains("keep: 2"), "{stdout}");
    assert!(stdout.contains(candidate.to_str().unwrap()), "{stdout}");
    assert!(candidate.exists());
}

#[test]
fn stale_is_opt_in_and_yes_removes_only_eligible_worktrees() {
    let repo = Repo::new();
    let root = repo.primary.parent().unwrap();
    let safe = root.join("safe");
    let young = root.join("young");
    let old = root.join("old");
    let dirty = root.join("dirty");
    let detached = root.join("detached");
    repo.worktree("feature/safe", &safe);
    repo.worktree("feature/young", &young);
    repo.worktree("feature/old", &old);
    repo.worktree("feature/dirty", &dirty);
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
    unmerged_change(&young, "young-change");
    unmerged_change(&old, "old-change");
    old_directory(&old);
    std::fs::write(dirty.join("untracked"), "keep\n").unwrap();

    let preview = repo.run(&["prune"]);
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(text.contains("safe: 1"), "{text}");
    assert!(text.contains("stale: 0"), "{text}");
    assert!(text.contains("keep: 6"), "{text}");
    assert!(text.contains(safe.to_str().unwrap()), "{text}");
    assert!(
        !text.contains(&format!("remove: {}", old.display())),
        "{text}"
    );

    let preview = repo.run(&["prune", "--stale"]);
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(text.contains("safe: 1"), "{text}");
    assert!(text.contains("stale: 1"), "{text}");
    assert!(text.contains("keep: 5"), "{text}");
    assert!(
        text.contains(&format!("remove: {}", old.display())),
        "{text}"
    );
    assert!(old.exists());

    let removed = repo.run(&["prune", "--stale", "--yes"]);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!safe.exists());
    assert!(!old.exists());
    for path in [&repo.primary, &repo.base, &young, &dirty, &detached] {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn current_worktree_is_preserved_even_when_merged_and_clean() {
    let repo = Repo::new();
    let current = repo.primary.parent().unwrap().join("current");
    repo.worktree("feature/current", &current);
    let output = repo
        .wt
        .command()
        .current_dir(&current)
        .args(["prune", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("keep: 3")
    );
    assert!(current.exists());
}

#[test]
fn candidate_containing_preserved_worktree_is_kept() {
    let repo = Repo::new();
    std::fs::write(repo.base.join(".gitignore"), "nested/\n").unwrap();
    git(&repo.base, &["add", ".gitignore"]);
    commit(&repo.base, "ignore nested worktrees");
    let outer = repo.primary.parent().unwrap().join("outer");
    repo.worktree("feature/outer", &outer);
    let nested = outer.join("nested");
    repo.worktree("feature/nested", &nested);
    unmerged_change(&nested, "nested-change");
    let output = repo.run(&["prune", "--yes"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("safe: 0"), "{text}");
    assert!(text.contains("keep: 4"), "{text}");
    assert!(outer.exists() && nested.exists());
}

#[test]
fn nested_candidates_are_removed_from_descendant_to_ancestor() {
    let repo = Repo::new();
    std::fs::write(repo.base.join(".gitignore"), "nested/\n").unwrap();
    git(&repo.base, &["add", ".gitignore"]);
    commit(&repo.base, "ignore nested worktrees");
    let outer = repo.primary.parent().unwrap().join("outer");
    repo.worktree("feature/outer", &outer);
    let nested = outer.join("nested");
    repo.worktree("feature/nested", &nested);
    let output = repo.run(&["prune", "--yes"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!nested.exists());
    assert!(!outer.exists());
}

#[test]
fn single_key_confirmation_accepts_lower_and_upper_y() {
    for key in ["y", "Y"] {
        let repo = Repo::new();
        let candidate = repo.primary.parent().unwrap().join("candidate");
        repo.worktree("feature/candidate", &candidate);
        let mut session = repo.pty(&["prune"]);
        session.expect("[y/N]").unwrap();
        session.send(key).unwrap();
        session.expect(Eof).unwrap();
        assert!(!candidate.exists(), "key {key}");
    }
}

#[test]
fn enter_n_and_other_key_cancel() {
    for key in ["\r", "n", "x"] {
        let repo = Repo::new();
        let candidate = repo.primary.parent().unwrap().join("candidate");
        repo.worktree("feature/candidate", &candidate);
        let mut session = repo.pty(&["prune"]);
        session.expect("[y/N]").unwrap();
        session.send(key).unwrap();
        session.expect(Eof).unwrap();
        assert!(candidate.exists(), "key {key:?}");
    }
}

#[test]
fn tty_yes_and_no_candidate_do_not_prompt() {
    let repo = Repo::new();
    let mut session = repo.pty(&["prune"]);
    let result = session.expect(Eof).unwrap();
    assert!(!String::from_utf8_lossy(result.before()).contains("[y/N]"));

    let candidate = repo.primary.parent().unwrap().join("candidate");
    repo.worktree("feature/candidate", &candidate);
    let mut session = repo.pty(&["prune", "--yes"]);
    let result = session.expect(Eof).unwrap();
    assert!(!String::from_utf8_lossy(result.before()).contains("[y/N]"));
    assert!(!candidate.exists());
}

#[test]
fn candidate_becoming_dirty_during_confirmation_is_preserved() {
    let repo = Repo::new();
    let candidate = repo.primary.parent().unwrap().join("candidate");
    repo.worktree("feature/candidate", &candidate);
    let mut session = repo.pty(&["prune"]);
    session.expect("[y/N]").unwrap();
    std::fs::write(candidate.join("new-file"), "keep\n").unwrap();
    session.send("y").unwrap();
    session.expect(Eof).unwrap();
    assert_eq!(
        std::fs::read_to_string(candidate.join("new-file")).unwrap(),
        "keep\n"
    );
}

#[test]
fn merge_base_failure_is_reported_without_removal() {
    let repo = Repo::new();
    let candidate = repo.primary.parent().unwrap().join("candidate");
    repo.worktree("feature/candidate", &candidate);
    let real_git = ProcessCommand::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let fake_dir = repo.primary.parent().unwrap().join("fake-bin");
    std::fs::create_dir(&fake_dir).unwrap();
    let fake_git = fake_dir.join("git");
    std::fs::write(
        &fake_git,
        "#!/bin/sh\nif [ \"$1\" = merge-base ]; then echo injected-failure >&2; exit 128; fi\nexec \"$WT_REAL_GIT\" \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = repo.wt.path_with(&fake_dir);
    let output = repo
        .wt
        .command()
        .current_dir(&repo.base)
        .args(["prune", "--yes"])
        .env("PATH", path)
        .env(
            "WT_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("merge-base --is-ancestor failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(candidate.exists());
}
