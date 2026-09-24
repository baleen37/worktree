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
    linked: PathBuf,
    writer: PathBuf,
    wt: TestWt,
}

impl Repo {
    fn new(base: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let primary = root.join("primary repo");
        let origin = root.join("origin.git");
        let writer = root.join("writer");
        let linked = root.join("linked feature");
        git(&root, &["init", "-b", base, primary.to_str().unwrap()]);
        std::fs::write(primary.join("README.md"), "initial\n").unwrap();
        std::fs::write(primary.join(".gitignore"), "/.worktrees/\n").unwrap();
        git(&primary, &["add", "."]);
        commit(&primary, "initial");
        git(&root, &["init", "--bare", origin.to_str().unwrap()]);
        git(
            &primary,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&primary, &["push", "-u", "origin", base]);
        git(
            &root,
            &[
                "clone",
                "-b",
                base,
                origin.to_str().unwrap(),
                writer.to_str().unwrap(),
            ],
        );
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "feature/list",
                linked.to_str().unwrap(),
            ],
        );
        Self {
            _temp: temp,
            primary,
            linked,
            writer,
            wt: TestWt::new(),
        }
    }

    fn remote_commit(&self) -> String {
        std::fs::write(self.writer.join("remote.txt"), "from origin\n").unwrap();
        git(&self.writer, &["add", "."]);
        commit(&self.writer, "remote advance");
        git(&self.writer, &["push", "origin", "HEAD"]);
        git_text(&self.writer, &["rev-parse", "HEAD"])
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

fn git_text(cwd: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
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
fn creates_explicit_branch_from_fast_forwarded_main() {
    let repo = Repo::new("main");
    let remote_head = repo.remote_commit();
    let target = repo.primary.join(".worktrees/feature-new");

    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .env_remove("WT_SHELL_PATH_FILE")
        .assert()
        .success()
        .stdout(format!("{}\n", target.display()));

    assert_eq!(git_text(&repo.primary, &["rev-parse", "HEAD"]), remote_head);
    assert_eq!(git_text(&target, &["rev-parse", "HEAD"]), remote_head);
    assert_eq!(
        git_text(&target, &["branch", "--show-current"]),
        "feature/new"
    );
}

#[test]
fn dirty_base_prevents_creation() {
    let repo = Repo::new("main");
    std::fs::write(repo.primary.join("README.md"), "dirty\n").unwrap();
    let target = repo.primary.join(".worktrees/feature-new");

    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("dirty"));

    assert!(!target.exists());
    assert!(!git_text(&repo.primary, &["branch", "--list", "feature/new"]).contains("feature/new"));
}

#[test]
fn divergent_base_prevents_creation() {
    let repo = Repo::new("main");
    let old_head = git_text(&repo.primary, &["rev-parse", "HEAD"]);
    std::fs::write(repo.primary.join("local.txt"), "local\n").unwrap();
    git(&repo.primary, &["add", "."]);
    commit(&repo.primary, "local advance");
    repo.remote_commit();

    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("pull --ff-only"));

    assert_ne!(git_text(&repo.primary, &["rev-parse", "HEAD"]), old_head);
    assert!(!repo.primary.join(".worktrees/feature-new").exists());
    assert!(git_text(&repo.primary, &["branch", "--list", "feature/new"]).is_empty());
}

#[test]
fn creates_from_master_and_writes_shell_path() {
    let repo = Repo::new("master");
    let remote_head = repo.remote_commit();
    let target = repo.primary.join(".worktrees/feature-new");
    let path_file = repo.primary.parent().unwrap().join("shell-path");

    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .env("WT_SHELL_PATH_FILE", &path_file)
        .assert()
        .success()
        .stdout("");

    assert_eq!(
        std::fs::read_to_string(path_file).unwrap(),
        format!("{}\n", target.display())
    );
    assert_eq!(git_text(&target, &["rev-parse", "HEAD"]), remote_head);
}

#[test]
fn existing_local_branch_is_rejected() {
    let repo = Repo::new("main");
    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
    assert_eq!(
        git_text(&repo.linked, &["branch", "--show-current"]),
        "feature/list"
    );
}

#[test]
fn missing_checked_out_base_prevents_creation() {
    let repo = Repo::new("main");
    git(&repo.primary, &["checkout", "--detach"]);
    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not checked out"));
    assert!(!repo.primary.join(".worktrees/feature-new").exists());
}

#[test]
fn normalized_path_collision_preserves_registered_worktree() {
    let repo = Repo::new("main");
    let target = repo.primary.join(".worktrees/feature-ui");
    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/ui"])
        .assert()
        .success();
    std::fs::write(target.join("keep"), "first worktree").unwrap();

    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature-ui"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("worktree path already exists"));

    assert_eq!(
        std::fs::read_to_string(target.join("keep")).unwrap(),
        "first worktree"
    );
    assert_eq!(
        git_text(&target, &["branch", "--show-current"]),
        "feature/ui"
    );
    assert!(git_text(&repo.primary, &["branch", "--list", "feature-ui"]).is_empty());
}

#[test]
fn fetch_failure_prevents_creation() {
    let repo = Repo::new("main");
    git(
        &repo.primary,
        &["remote", "set-url", "origin", "/missing/origin.git"],
    );
    repo.wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fetch origin failed"));
    assert!(!repo.primary.join(".worktrees/feature-new").exists());
    assert!(git_text(&repo.primary, &["branch", "--list", "feature/new"]).is_empty());
}

#[test]
fn omitted_name_creates_legacy_three_word_branch() {
    let repo = Repo::new("main");
    let remote_head = repo.remote_commit();
    let output = repo
        .wt
        .command()
        .current_dir(&repo.linked)
        .args(["switch", "-c"])
        .env_remove("WT_SHELL_PATH_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let target = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let name = git_text(&target, &["branch", "--show-current"]);
    let parts: Vec<_> = name.split('-').collect();
    assert_eq!(parts.len(), 3);
    assert!("snappy brave calm clever eager fuzzy gentle happy jolly keen lively mellow nimble proud quick silly swift witty zesty bold bright chill cosmic cozy crisp daring dapper epic fancy fierce glossy humble lucky mighty peppy plucky quirky royal sunny tidy".split_whitespace().any(|word| word == parts[0]));
    assert!("greeting falcon otter panda harbor meadow canyon comet lantern beacon cipher nebula pebble prairie quartz ripple summit thicket tundra voyage willow anchor badger cactus dahlia ember fjord glacier horizon iris juniper kettle lagoon mango".split_whitespace().any(|word| word == parts[1]));
    assert!("bachman turing lovelace hopper knuth ritchie torvalds dijkstra kernighan stallman carmack abramov hickey armstrong rossum wall matz gosling stroustrup liskov hamilton feynman curie tesla darwin newton galileo kepler hubble sagan".split_whitespace().any(|word| word == parts[2]));
    assert_eq!(target, repo.primary.join(".worktrees").join(&name));
    assert_eq!(git_text(&target, &["rev-parse", "HEAD"]), remote_head);
}
