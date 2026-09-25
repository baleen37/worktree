#[path = "support/git_repo.rs"]
mod git_repo;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use git_repo::{GitRepo, git};

struct FakeTools {
    _dir: tempfile::TempDir,
    path: PathBuf,
    log: PathBuf,
    real_git: PathBuf,
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
        symlink(&real_git, path.join("git")).unwrap();
        let herdr = path.join("herdr");
        fs::write(&herdr, "#!/bin/sh\nprintf 'herdr' >> \"$WT_LOG\"\nfor arg in \"$@\"; do printf ' <%s>' \"$arg\" >> \"$WT_LOG\"; done\nprintf '\\n' >> \"$WT_LOG\"\nif [ \"$WT_HERDR_FAIL\" = \"$2\" ]; then exit 31; fi\nif [ \"$1 $2\" = 'worktree list' ]; then printf '%s\\n' \"$WT_HERDR_LIST\"; exit 0; fi\nif [ \"$1 $2\" = 'worktree create' ]; then\n  shift 2\n  while [ $# -gt 0 ]; do\n    case \"$1\" in --branch) branch=$2; shift 2;; --base) base=$2; shift 2;; --path) target=$2; shift 2;; *) shift;; esac\n  done\n  if \"$WT_REAL_GIT\" -C \"$WT_PRIMARY\" show-ref --verify --quiet \"refs/heads/$branch\"; then\n    exec \"$WT_REAL_GIT\" -C \"$WT_PRIMARY\" worktree add -- \"$target\" \"$branch\"\n  fi\n  exec \"$WT_REAL_GIT\" -C \"$WT_PRIMARY\" worktree add -b \"$branch\" -- \"$target\" \"$base\"\nfi\n").unwrap();
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
            _dir: dir,
            path,
            real_git,
        }
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::cargo_bin("wt").unwrap();
        command
            .current_dir(cwd)
            .env("PATH", &self.path)
            .env("WT_LOG", &self.log)
            .env("WT_REAL_GIT", &self.real_git)
            .env("WT_PRIMARY", cwd)
            .env("WT_HERDR_LIST", "[]")
            .env_remove("WT_SHELL_PATH_FILE")
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

    fn await_nix(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self
            .lines()
            .iter()
            .filter(|line| *line == "nix <store> <gc>")
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
        .env("WT_HERDR_LIST", r#"[{"source_workspace_id":"source-id"}]"#)
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
            "nix <store> <gc>",
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
    let a = repo.primary.parent().unwrap().join("a");
    let b = repo.primary.parent().unwrap().join("b");
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
            .filter(|line| *line == "nix <store> <gc>")
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
            .filter(|line| *line == "nix <store> <gc>")
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
