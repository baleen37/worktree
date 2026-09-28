#[path = "support/git_repo.rs"]
mod git_repo;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use git_repo::{GitRepo, git};

struct FakeTools {
    _dir: tempfile::TempDir,
    path: PathBuf,
    log: PathBuf,
    session_log: PathBuf,
    list_started: PathBuf,
    real_git: PathBuf,
    real_sleep: PathBuf,
}

impl FakeTools {
    fn new(nix: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin");
        fs::create_dir(&path).unwrap();
        let real_git = PathBuf::from(
            String::from_utf8(
                ProcessCommand::new("sh")
                    .args(["-c", "command -v git"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .trim(),
        );
        let real_sleep = PathBuf::from(
            String::from_utf8(
                ProcessCommand::new("sh")
                    .args(["-c", "command -v sleep"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .trim(),
        );
        symlink(&real_git, path.join("git")).unwrap();
        let herdr = path.join("herdr");
        fs::write(
            &herdr,
            r#"#!/bin/sh
printf 'herdr' >> "$WT_LOG"
for arg in "$@"; do printf ' <%s>' "$arg" >> "$WT_LOG"; done
printf '\n' >> "$WT_LOG"
if [ "$WT_HERDR_FAIL" = "$2" ]; then exit 31; fi
if [ "$1 $2" = 'worktree list' ]; then
  if [ -n "$WT_SESSION_LOG" ]; then printf '%s\n' "$PPID" >> "$WT_SESSION_LOG"; fi
  if [ "$WT_HERDR_LIST_DELAY" = 1 ]; then : > "$WT_LIST_STARTED"; "$WT_SLEEP" 30; fi
  printf '%s\n' "$WT_HERDR_LIST"
  exit 0
fi
if [ "$1 $2" = 'worktree remove' ]; then
  if [ -n "$WT_SESSION_LOG" ]; then printf '%s\n' "$PPID" >> "$WT_SESSION_LOG"; fi
  force=0
  for arg in "$@"; do
    if [ "$arg" = "--force" ]; then force=1; fi
  done
  if [ "$force" = 1 ]; then
    exec "$WT_REAL_GIT" -C "$WT_PRIMARY" worktree remove --force -- "$WT_HERDR_REMOVE_PATH"
  fi
  exec "$WT_REAL_GIT" -C "$WT_PRIMARY" worktree remove -- "$WT_HERDR_REMOVE_PATH"
fi
if [ "$1 $2" = 'worktree create' ]; then
  shift 2
  while [ $# -gt 0 ]; do
    case "$1" in --branch) branch=$2; shift 2;; --base) base=$2; shift 2;; --path) target=$2; shift 2;; *) shift;; esac
  done
  if "$WT_REAL_GIT" -C "$WT_PRIMARY" show-ref --verify --quiet "refs/heads/$branch"; then
    exec "$WT_REAL_GIT" -C "$WT_PRIMARY" worktree add -- "$target" "$branch"
  fi
  exec "$WT_REAL_GIT" -C "$WT_PRIMARY" worktree add -b "$branch" -- "$target" "$base"
fi
"#,
        )
        .unwrap();
        fs::set_permissions(&herdr, fs::Permissions::from_mode(0o755)).unwrap();
        if nix {
            let real_sleep = String::from_utf8(
                ProcessCommand::new("sh")
                    .args(["-c", "command -v sleep"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap();
            symlink(real_sleep.trim(), path.join("sleep")).unwrap();
            let exe = path.join("nix");
            fs::write(
                &exe,
                r#"#!/bin/sh
printf 'nix' >> "$WT_LOG"
for arg in "$@"; do
  printf ' <%s>' "$arg" >> "$WT_LOG"
  if [ "$WT_NIX_DELAY" = 1 ] && [ "$arg" = store ]; then sleep 0.1; fi
done
printf '\n' >> "$WT_LOG"
if [ "$WT_NIX_FAIL" = 1 ]; then exit 42; fi
"#,
            )
            .unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self {
            log: dir.path().join("calls"),
            session_log: dir.path().join("session"),
            list_started: dir.path().join("list-started"),
            _dir: dir,
            path,
            real_git,
            real_sleep,
        }
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::cargo_bin("wt").unwrap();
        command
            .current_dir(cwd)
            .env("PATH", &self.path)
            .env("WT_LOG", &self.log)
            .env("WT_REAL_GIT", &self.real_git)
            .env("WT_SESSION_LOG", &self.session_log)
            .env("WT_LIST_STARTED", &self.list_started)
            .env("WT_SLEEP", &self.real_sleep)
            .env("WT_PRIMARY", cwd)
            .env("WT_HERDR_LIST", "[]")
            .env_remove("WT_SHELL_PATH_FILE")
            .env_remove("WT_HERDR_LIST_DELAY")
            .env_remove("WT_NIX_DELAY")
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_WORKSPACE_ID");
        command
    }

    fn lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn process_group_id(&self, pid: u32) -> libc::pid_t {
        // SAFETY: getpgid only queries the process identified by pid.
        let process_group = unsafe { libc::getpgid(pid as libc::pid_t) };
        assert_ne!(process_group, -1, "could not read process group for {pid}");
        process_group
    }

    fn await_nix(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self
            .lines()
            .iter()
            .filter(|line| {
                *line == "nix <--extra-experimental-features> <nix-command> <store> <gc>"
            })
            .count()
            < count
        {
            assert!(
                Instant::now() < deadline,
                "missing nix call: {:?}",
                self.lines()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn herdr_list(root: &Path, worktrees: &[(&Path, Option<&str>)]) -> String {
    let worktrees = worktrees
        .iter()
        .map(|(path, workspace_id)| match workspace_id {
            Some(id) => serde_json::json!({
                "path": path.to_str().unwrap(),
                "open_workspace_id": id,
            }),
            None => serde_json::json!({ "path": path.to_str().unwrap() }),
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "id": "request-id",
        "result": {
            "source": {
                "source_workspace_id": "source-id",
                "repo_root": root.to_str().unwrap(),
            },
            "worktrees": worktrees,
        },
    })
    .to_string()
}

fn branch_exists(repo_root: &Path, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    ProcessCommand::new("git")
        .current_dir(repo_root)
        .args(["show-ref", "--verify", "--quiet", &reference])
        .status()
        .unwrap()
        .success()
}

#[test]
fn active_herdr_reads_current_envelope_and_uses_source_workspace_id() {
    let repo = GitRepo::with_origin();
    let tools = FakeTools::new(false);
    let response = herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]);

    tools
        .command(&repo.linked)
        .args(["switch", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_HERDR_LIST", response)
        .assert()
        .success();
    assert!(tools.lines().iter().any(|line| {
        line.contains("<open> <--workspace> <source-id>")
            && line.contains(repo.linked.to_str().unwrap())
    }));
}

#[test]
fn active_herdr_rejects_invalid_or_foreign_repository_response() {
    let repo = GitRepo::new();
    for response in ["not-json".to_owned(), herdr_list(&repo.linked, &[])] {
        let tools = FakeTools::new(false);
        let output = tools
            .command(&repo.primary)
            .args(["switch", "feature/list"])
            .env("HERDR_ENV", "1")
            .env("HERDR_WORKSPACE_ID", "source-id")
            .env("WT_HERDR_LIST", response)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(tools.lines().len(), 1);
        assert!(tools.lines()[0].contains("<list> <--workspace> <source-id>"));
    }
}

#[test]
fn active_herdr_remove_uses_child_id_and_restores_caller_focus() {
    let repo = GitRepo::new();
    fs::write(repo.linked.join("unmerged.txt"), "keep branch\n").unwrap();
    git(&repo.linked, &["add", "."]);
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "unmerged feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(
        tools
            .lines()
            .iter()
            .any(|line| line == "herdr <worktree> <remove> <--workspace> <child-id>")
    );
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
    assert!(branch_exists(&repo.primary, "feature/list"));
}

#[test]
fn active_herdr_remove_failure_keeps_worktree_and_branch_without_git_fallback() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .env("WT_HERDR_FAIL", "remove")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(branch_exists(&repo.primary, "feature/list"));
    assert_eq!(
        tools
            .lines()
            .iter()
            .filter(|line| line.contains("<remove>"))
            .count(),
        1
    );
    assert!(!tools.lines().iter().any(|line| line.contains("<open>")));
}

#[test]
fn active_herdr_remove_rejects_foreign_repository_before_deleting() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "foreign-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.linked, &[(&repo.linked, Some("child-id"))]),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(branch_exists(&repo.primary, "feature/list"));
    assert_eq!(tools.lines().len(), 1);
    assert!(tools.lines()[0].contains("<list> <--workspace> <foreign-id>"));
}

#[test]
fn active_herdr_remove_dirty_worktree_does_not_call_herdr() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    fs::write(repo.linked.join("uncommitted.txt"), "keep\n").unwrap();
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(tools.lines().is_empty());
}

#[test]
fn active_herdr_remove_protected_and_dirty_paths_never_reach_herdr() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "main"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.primary.exists());
    assert!(tools.lines().is_empty());

    let repo = GitRepo::new();
    fs::write(repo.linked.join("dirty.txt"), "keep\n").unwrap();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(tools.lines().is_empty());
}

#[test]
fn active_herdr_remove_without_child_id_uses_git() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, None)]),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(!tools.lines().iter().any(|line| line.contains("<remove>")));
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
}

#[test]
fn active_herdr_remove_current_worktree_focuses_base() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["remove"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
}

#[test]
fn active_herdr_remove_current_writes_safe_path_when_focus_fails() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let shell_path_file = repo.primary.parent().unwrap().join("shell-path");
    let output = tools
        .command(&repo.linked)
        .args(["remove"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .env("WT_HERDR_FAIL", "open")
        .env("WT_SHELL_PATH_FILE", &shell_path_file)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!repo.linked.exists());
    assert_eq!(
        fs::read_to_string(shell_path_file).unwrap(),
        format!("{}\n", repo.primary.display())
    );
}

#[test]
fn active_herdr_remove_current_worktree_completes_after_herdr_closes_pane() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["remove"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    let worker_pid: libc::pid_t = fs::read_to_string(&tools.session_log)
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(worker_pid > 0);
}

#[test]
fn active_herdr_remove_forwards_sigint_to_detached_worker() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_wt"));
    command
        .current_dir(&repo.primary)
        .args(["remove", "feature/list"])
        .env("PATH", &tools.path)
        .env("WT_LOG", &tools.log)
        .env("WT_REAL_GIT", &tools.real_git)
        .env("WT_SESSION_LOG", &tools.session_log)
        .env("WT_LIST_STARTED", &tools.list_started)
        .env("WT_SLEEP", &tools.real_sleep)
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env("WT_HERDR_LIST_DELAY", "1");
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn().unwrap();

    let startup_deadline = Instant::now() + Duration::from_secs(3);
    while !tools.list_started.exists() && child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < startup_deadline,
            "Herdr list did not start"
        );
        thread::sleep(Duration::from_millis(10));
    }
    if !tools.list_started.exists() {
        let status = child.wait().unwrap();
        panic!("wt exited before the delayed Herdr list started: {status}");
    }

    let worker_pid: libc::pid_t = fs::read_to_string(&tools.session_log)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(worker_pid > 0);
    // Herdr is still in its delayed list call, so the detached worker is alive.
    let worker_group = tools.process_group_id(worker_pid as u32);
    assert_eq!(worker_group, worker_pid, "worker must lead its own session");
    assert_ne!(
        worker_group,
        tools.process_group_id(std::process::id()),
        "worker must be outside the caller's pane session"
    );
    // SAFETY: child.id() is the live wt process started above.
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );

    let stop_deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= stop_deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    if status.is_none() {
        // SAFETY: worker_group identifies the isolated session created by wt.
        unsafe { libc::kill(-worker_group, libc::SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
        panic!("wt did not finish after forwarding Ctrl+C to its worker");
    }

    let group_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < group_deadline {
        // SAFETY: signal zero only checks whether the isolated worker group exists.
        if unsafe { libc::kill(-worker_group, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: clean up the isolated group if a shell child ignored SIGINT.
    let group_exists = unsafe { libc::kill(-worker_group, 0) } == 0;
    if group_exists {
        // SAFETY: worker_group belongs to this test's detached command.
        unsafe { libc::kill(-worker_group, libc::SIGKILL) };
    }
    assert!(!group_exists, "detached Herdr worker remained after Ctrl+C");
    assert!(!status.unwrap().success());
    assert!(repo.linked.exists());
    assert!(branch_exists(&repo.primary, "feature/list"));
    assert!(!tools.lines().iter().any(|line| line.contains("<remove>")));
}

#[test]
fn active_herdr_merge_removes_source_child_and_focuses_target() {
    let repo = GitRepo::with_origin();
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["merge"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(!branch_exists(&repo.primary, "feature/list"));
    assert!(
        tools
            .lines()
            .iter()
            .any(|line| line == "herdr <worktree> <remove> <--workspace> <child-id>")
    );
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
}

#[test]
fn active_herdr_merge_remove_failure_keeps_source_and_branch() {
    let repo = GitRepo::with_origin();
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["merge"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .env("WT_HERDR_FAIL", "remove")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(branch_exists(&repo.primary, "feature/list"));
    assert!(!tools.lines().iter().any(|line| line.contains("<open>")));
    assert!(
        ProcessCommand::new("git")
            .current_dir(&repo.primary)
            .args(["merge-base", "--is-ancestor", "feature/list", "main"])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn active_herdr_merge_without_source_id_uses_git_remove() {
    let repo = GitRepo::with_origin();
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["merge"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, None)]),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(!branch_exists(&repo.primary, "feature/list"));
    assert!(!tools.lines().iter().any(|line| line.contains("<remove>")));
}

#[test]
fn active_herdr_merge_focuses_selected_target_worktree() {
    let repo = GitRepo::with_origin();
    let release = repo.primary.parent().unwrap().join("release");
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "release",
            release.to_str().unwrap(),
            "main",
        ],
    );
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["merge", "release"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.linked.exists());
    assert!(branch_exists(&repo.primary, "release"));
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            release.display()
        )));
}

#[test]
fn active_herdr_merge_writes_target_path_when_focus_fails() {
    let repo = GitRepo::with_origin();
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let shell_path_file = repo.primary.parent().unwrap().join("shell-path");
    let output = tools
        .command(&repo.linked)
        .args(["merge"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "child-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &repo.linked)
        .env("WT_HERDR_FAIL", "open")
        .env("WT_SHELL_PATH_FILE", &shell_path_file)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!repo.linked.exists());
    assert!(!branch_exists(&repo.primary, "feature/list"));
    assert_eq!(
        fs::read_to_string(shell_path_file).unwrap(),
        format!("{}\n", repo.primary.display())
    );
}

#[test]
fn active_herdr_merge_rejects_foreign_repository_before_merging() {
    let repo = GitRepo::with_origin();
    git(
        &repo.linked,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "feature",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.linked)
        .args(["merge"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "foreign-id")
        .env("WT_PRIMARY", &repo.primary)
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.linked, &[(&repo.linked, Some("child-id"))]),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(repo.linked.exists());
    assert!(branch_exists(&repo.primary, "feature/list"));
    assert_eq!(tools.lines().len(), 1);
    assert!(tools.lines()[0].contains("<list> <--workspace> <foreign-id>"));
    assert!(
        !ProcessCommand::new("git")
            .current_dir(&repo.primary)
            .args(["merge-base", "--is-ancestor", "feature/list", "main"])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn active_herdr_prune_force_removes_dirty_open_child_preserves_branch_and_focus() {
    let repo = GitRepo::with_origin();
    let child = repo.primary.join(".worktrees/herdr-child");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "feature/herdr-child",
            child.to_str().unwrap(),
            "main",
        ],
    );
    fs::write(child.join("uncommitted.txt"), "discard this child\n").unwrap();
    let tools = FakeTools::new(true);
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--force", "--yes"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&child, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &child)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    tools.await_nix(1);
    assert!(!child.exists());
    assert!(branch_exists(&repo.primary, "feature/herdr-child"));
    assert!(!child.join("uncommitted.txt").exists());
    assert!(
        tools
            .lines()
            .iter()
            .any(|line| line == "herdr <worktree> <remove> <--workspace> <child-id> <--force>")
    );
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
    assert_eq!(
        tools
            .lines()
            .iter()
            .filter(|line| {
                *line == "nix <--extra-experimental-features> <nix-command> <store> <gc>"
            })
            .count(),
        1
    );
}

#[test]
fn active_herdr_prune_uses_herdr_only_for_the_callers_repository() {
    let repo = GitRepo::with_origin();
    let herdr_candidate = repo.primary.join(".worktrees/herdr-candidate");
    fs::create_dir_all(herdr_candidate.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "feature/herdr-candidate",
            herdr_candidate.to_str().unwrap(),
            "main",
        ],
    );

    let other_root = repo.primary.join(".worktrees/other-repository");
    fs::create_dir_all(&other_root).unwrap();
    let other_primary = other_root.join("main checkout");
    git(
        &other_root,
        &["init", "-b", "main", other_primary.to_str().unwrap()],
    );
    fs::write(other_primary.join("README.md"), "other repository\n").unwrap();
    git(&other_primary, &["add", "."]);
    git(
        &other_primary,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "initial",
        ],
    );
    let git_candidate = other_root.join(".worktrees/git-candidate");
    git(
        &other_primary,
        &[
            "worktree",
            "add",
            "-b",
            "feature/git-candidate",
            git_candidate.to_str().unwrap(),
            "main",
        ],
    );

    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--yes"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&herdr_candidate, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &herdr_candidate)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!herdr_candidate.exists());
    assert!(!git_candidate.exists());
    assert!(repo.primary.exists());
    assert!(branch_exists(&repo.primary, "feature/herdr-candidate"));
    assert!(branch_exists(&other_primary, "feature/git-candidate"));
    let herdr_removals: Vec<_> = tools
        .lines()
        .into_iter()
        .filter(|line| line.contains("<remove>"))
        .collect();
    assert_eq!(
        herdr_removals,
        ["herdr <worktree> <remove> <--workspace> <child-id>"]
    );
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
}

#[test]
fn active_herdr_prune_failure_does_not_git_remove_same_candidate() {
    let repo = GitRepo::with_origin();
    let child = repo.primary.join(".worktrees/herdr-child");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "feature/herdr-child",
            child.to_str().unwrap(),
            "main",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--yes"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&child, Some("child-id"))]),
        )
        .env("WT_HERDR_REMOVE_PATH", &child)
        .env("WT_HERDR_FAIL", "remove")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(child.exists());
    assert!(branch_exists(&repo.primary, "feature/herdr-child"));
    assert_eq!(
        tools
            .lines()
            .iter()
            .filter(|line| line.contains("<remove>"))
            .count(),
        1
    );
}

#[test]
fn active_herdr_prune_without_child_id_uses_git_and_keeps_branch() {
    let repo = GitRepo::with_origin();
    let child = repo.primary.join(".worktrees/herdr-child");
    fs::create_dir_all(child.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &[
            "worktree",
            "add",
            "-b",
            "feature/herdr-child",
            child.to_str().unwrap(),
            "main",
        ],
    );
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--yes"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&child, None)]),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!child.exists());
    assert!(branch_exists(&repo.primary, "feature/herdr-child"));
    assert!(!tools.lines().iter().any(|line| line.contains("<remove>")));
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.primary.display()
        )));
}

#[test]
fn active_herdr_resolves_linked_workspace_and_creates_with_exact_arguments() {
    let repo = GitRepo::with_origin();
    let tools = FakeTools::new(true);
    let target = repo.primary.join(".worktrees/feature-new");
    let output = tools
        .command(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "linked-id")
        .env("WT_NIX_DELAY", "1")
        .env("WT_HERDR_LIST", herdr_list(&repo.primary, &[]))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target.exists());
    tools.await_nix(1);
    assert_eq!(
        tools.lines(),
        [
            "herdr <worktree> <list> <--workspace> <linked-id>",
            &format!(
                "herdr <worktree> <create> <--workspace> <source-id> <--branch> <feature/new> <--base> <main> <--path> <{}> <--focus>",
                target.display()
            ),
            "nix <--extra-experimental-features> <nix-command> <store> <gc>",
        ]
    );
}

#[test]
fn active_herdr_opens_registered_worktree_and_creates_for_unattached_branch() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("linked-id"))]),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        tools.lines(),
        [
            "herdr <worktree> <list> <--workspace> <source-id>",
            &format!(
                "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
                repo.linked.display()
            ),
        ]
    );

    git(&repo.primary, &["branch", "feature/new"]);
    let target = repo.primary.join(".worktrees/feature-new");
    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/new"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env("WT_HERDR_LIST", herdr_list(&repo.primary, &[]))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(tools.lines().contains(&format!("herdr <worktree> <create> <--workspace> <source-id> <--branch> <feature/new> <--base> <main> <--path> <{}> <--focus>", target.display())));
}

#[test]
fn active_herdr_switch_paths_leave_shell_path_file_empty() {
    let repo = GitRepo::with_origin();
    let tools = FakeTools::new(false);
    let shell_path_file = repo.primary.parent().unwrap().join("shell-path");
    fs::write(&shell_path_file, "").unwrap();

    let output = tools
        .command(&repo.linked)
        .args(["switch", "-c", "feature/new"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "linked-id")
        .env("WT_HERDR_LIST", herdr_list(&repo.primary, &[]))
        .env("WT_SHELL_PATH_FILE", &shell_path_file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&shell_path_file).unwrap(), "");
    assert!(tools.lines().iter().any(|line| line == &format!(
        "herdr <worktree> <create> <--workspace> <source-id> <--branch> <feature/new> <--base> <main> <--path> <{}> <--focus>",
        repo.primary.join(".worktrees/feature-new").display()
    )));

    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/list"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env(
            "WT_HERDR_LIST",
            herdr_list(&repo.primary, &[(&repo.linked, Some("linked-id"))]),
        )
        .env("WT_SHELL_PATH_FILE", &shell_path_file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(shell_path_file).unwrap(), "");
    assert!(tools.lines().iter().any(|line| line
        == &format!(
            "herdr <worktree> <open> <--workspace> <source-id> <--path> <{}> <--focus>",
            repo.linked.display()
        )));
}

#[test]
fn active_herdr_creates_remote_branch_from_origin_and_sets_upstream() {
    let repo = GitRepo::with_origin();
    git(&repo.primary, &["branch", "feature/remote"]);
    git(&repo.primary, &["push", "origin", "feature/remote"]);
    git(&repo.primary, &["branch", "-D", "feature/remote"]);
    git(&repo.primary, &["fetch", "origin"]);
    let tools = FakeTools::new(false);
    let target = repo.primary.join(".worktrees/feature-remote");

    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/remote"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "source-id")
        .env("WT_HERDR_LIST", herdr_list(&repo.primary, &[]))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(tools.lines().contains(&format!(
        "herdr <worktree> <create> <--workspace> <source-id> <--branch> <feature/remote> <--base> <refs/remotes/origin/feature/remote> <--path> <{}> <--focus>",
        target.display()
    )));
    let upstream = ProcessCommand::new("git")
        .current_dir(&target)
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ])
        .output()
        .unwrap();
    assert!(upstream.status.success());
    assert_eq!(
        String::from_utf8(upstream.stdout).unwrap(),
        "origin/feature/remote\n"
    );
}

#[test]
fn inactive_herdr_uses_git_and_active_failure_never_falls_back() {
    let repo = GitRepo::new();
    let tools = FakeTools::new(false);
    git(&repo.primary, &["branch", "feature/inactive"]);
    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/inactive"])
        .env("HERDR_ENV", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(repo.primary.join(".worktrees/feature-inactive").exists());
    assert!(tools.lines().is_empty());

    git(&repo.primary, &["branch", "feature/no-env"]);
    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/no-env"])
        .env("HERDR_WORKSPACE_ID", "id")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(repo.primary.join(".worktrees/feature-no-env").exists());
    assert!(tools.lines().is_empty());

    git(&repo.primary, &["branch", "feature/fail"]);
    let output = tools
        .command(&repo.primary)
        .args(["switch", "feature/fail"])
        .env("HERDR_ENV", "1")
        .env("HERDR_WORKSPACE_ID", "id")
        .env("WT_HERDR_LIST", herdr_list(&repo.primary, &[]))
        .env("WT_HERDR_FAIL", "create")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!repo.primary.join(".worktrees/feature-fail").exists());
}

#[test]
fn gc_runs_after_git_create_remove_and_once_per_prune_batch() {
    let repo = GitRepo::with_origin();
    let tools = FakeTools::new(true);
    let output = tools
        .command(&repo.primary)
        .args(["switch", "-c", "feature/new"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    tools.await_nix(1);
    let output = tools
        .command(&repo.primary)
        .args(["remove", "feature/new"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    tools.await_nix(2);
    let a = repo.primary.join(".worktrees/a");
    let b = repo.primary.join(".worktrees/b");
    fs::create_dir_all(a.parent().unwrap()).unwrap();
    git(
        &repo.primary,
        &["worktree", "add", "-b", "feature/a", a.to_str().unwrap()],
    );
    git(
        &repo.primary,
        &["worktree", "add", "-b", "feature/b", b.to_str().unwrap()],
    );
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--yes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    tools.await_nix(3);
    assert!(!a.exists() && !b.exists());
    assert_eq!(
        tools
            .lines()
            .iter()
            .filter(|line| {
                *line == "nix <--extra-experimental-features> <nix-command> <store> <gc>"
            })
            .count(),
        3
    );
    let output = tools
        .command(&repo.primary)
        .args(["prune", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        tools
            .lines()
            .iter()
            .filter(|line| {
                *line == "nix <--extra-experimental-features> <nix-command> <store> <gc>"
            })
            .count(),
        3
    );
}

#[test]
fn missing_or_failing_nix_preserves_success() {
    for nix in [false, true] {
        let repo = GitRepo::with_origin();
        let tools = FakeTools::new(nix);
        let output = tools
            .command(&repo.primary)
            .args(["switch", "-c", "feature/new"])
            .env("WT_NIX_FAIL", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(repo.primary.join(".worktrees/feature-new").exists());
        if nix {
            tools.await_nix(1);
        }
    }
}
