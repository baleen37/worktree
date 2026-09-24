use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

pub struct GitRepo {
    _temp: TempDir,
    pub primary: PathBuf,
    pub linked: PathBuf,
}

impl GitRepo {
    pub fn new() -> Self {
        Self::with_base("main")
    }

    pub fn with_base(base: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let primary = root.join("primary repo");
        let linked = root.join("linked feature list");

        git(
            temp.path(),
            &["init", "-b", base, primary.to_str().unwrap()],
        );
        std::fs::write(primary.join("README.md"), "test repository\n").unwrap();
        std::fs::write(primary.join(".gitignore"), "/.worktrees/\n").unwrap();
        git(&primary, &["add", "README.md", ".gitignore"]);
        git(
            &primary,
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
        }
    }

    #[allow(dead_code)] // This shared helper is used only by integration adapter tests.
    pub fn with_origin() -> Self {
        let repo = Self::new();
        let origin = repo.primary.parent().unwrap().join("origin.git");
        git(
            repo.primary.parent().unwrap(),
            &["init", "--bare", origin.to_str().unwrap()],
        );
        git(
            &repo.primary,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repo.primary, &["push", "-u", "origin", "main"]);
        repo
    }
}

pub fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
