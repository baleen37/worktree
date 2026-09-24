#[path = "support/git_repo.rs"]
mod git_repo;

use std::path::Path;
use std::process::Command;

use assert_cmd::Command as WtCommand;
use expectrl::{Eof, Expect, Session};
use git_repo::GitRepo;

const START: &str = "# >>> wt shell integration >>>";
const END: &str = "# <<< wt shell integration <<<";

fn wt(args: &[&str], home: &Path) -> std::process::Output {
    WtCommand::cargo_bin("wt")
        .unwrap()
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap()
}

#[test]
fn init_outputs_safe_wrappers_for_all_shells() {
    let home = tempfile::tempdir().unwrap();
    for shell in ["zsh", "bash", "fish"] {
        let output = wt(&["config", "shell", "init", shell], home.path());
        assert!(
            output.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("function wt"), "{shell}: {text}");
        assert!(text.contains("command wt"), "{shell}: {text}");
        assert!(!text.contains("eval"), "{shell}: {text}");
        assert!(!text.contains("source"), "{shell}: {text}");
    }
}

#[test]
fn repeated_install_and_uninstall_preserve_user_text() {
    let home = tempfile::tempdir().unwrap();
    let zsh = home.path().join(".zshrc");
    let bash = home.path().join(".bashrc");
    let fish = home.path().join(".config/fish/config.fish");
    std::fs::create_dir_all(fish.parent().unwrap()).unwrap();
    std::fs::write(&zsh, "# keep my settings").unwrap();
    for path in [&bash, &fish] {
        std::fs::write(path, "# keep my settings\n").unwrap();
    }
    for _ in 0..2 {
        let output = wt(&["config", "shell", "install"], home.path());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for path in [&zsh, &bash, &fish] {
        let text = std::fs::read_to_string(path).unwrap();
        assert_eq!(text.matches(START).count(), 1, "{}", path.display());
        assert_eq!(text.matches(END).count(), 1, "{}", path.display());
    }
    let output = wt(&["config", "shell", "uninstall"], home.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(zsh).unwrap(), "# keep my settings");
    for path in [&bash, &fish] {
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "# keep my settings\n"
        );
    }
}

#[test]
fn uninstall_preserves_lines_around_a_middle_block() {
    let home = tempfile::tempdir().unwrap();
    let zsh = home.path().join(".zshrc");
    std::fs::write(
        &zsh,
        format!("before\n{START}\neval \"$(wt config shell init zsh)\"\n{END}\nafter\n"),
    )
    .unwrap();

    let output = wt(&["config", "shell", "uninstall"], home.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(zsh).unwrap(), "before\nafter\n");
}

#[test]
fn picker_requires_tty_and_leaves_path_file_untouched() {
    let repo = GitRepo::new();
    let path_file = repo.primary.parent().unwrap().join("result");
    std::fs::write(&path_file, "sentinel\n").unwrap();
    for args in [vec![], vec!["switch"]] {
        let output = WtCommand::cargo_bin("wt")
            .unwrap()
            .current_dir(&repo.primary)
            .args(args)
            .env("WT_SHELL_PATH_FILE", &path_file)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires a TTY"));
        assert_eq!(std::fs::read_to_string(&path_file).unwrap(), "sentinel\n");
    }
}

#[test]
fn picker_selects_linked_worktree_from_tty() {
    let repo = GitRepo::new();
    let path_file = repo.primary.parent().unwrap().join("result");
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("wt"));
    command
        .current_dir(&repo.primary)
        .env("WT_SHELL_PATH_FILE", &path_file);
    let mut session = Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(5)));
    session.expect("feature/list").unwrap();
    session.send("feature/list").unwrap();
    session.send("\r").unwrap();
    session.expect(Eof).unwrap();
    assert_eq!(
        std::fs::read_to_string(path_file).unwrap(),
        format!("{}\n", repo.linked.display())
    );
}

#[test]
fn picker_escape_keeps_shell_path_unchanged() {
    let repo = GitRepo::new();
    let path_file = repo.primary.parent().unwrap().join("result");
    std::fs::write(&path_file, "sentinel\n").unwrap();
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("wt"));
    command
        .current_dir(&repo.primary)
        .arg("switch")
        .env("WT_SHELL_PATH_FILE", &path_file);
    let mut session = Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(5)));
    session.expect("feature/list").unwrap();
    session.send("\x1b").unwrap();
    session.expect(Eof).unwrap();
    assert_eq!(std::fs::read_to_string(path_file).unwrap(), "sentinel\n");
}

#[test]
fn shell_wrappers_change_to_path_with_spaces() {
    let repo = GitRepo::new();
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(assert_cmd::cargo::cargo_bin("wt"), bin.join("wt")).unwrap();
    }
    for shell in ["zsh", "bash", "fish"] {
        let binary = std::env::var(format!("WT_TEST_{}", shell.to_uppercase()))
            .unwrap_or_else(|_| shell.into());
        let script = home.path().join(format!("test.{shell}"));
        let init = if shell == "fish" {
            "wt config shell init fish | source".to_owned()
        } else {
            format!("eval \"$(wt config shell init {shell})\"")
        };
        let failed = if shell == "fish" {
            "wt switch missing; or true"
        } else {
            "wt switch missing || true"
        };
        let body = format!(
            "{init}\ncd \"{}\"\n{failed}\npwd\nwt switch feature/list\npwd\n",
            repo.primary.display()
        );
        std::fs::write(&script, body).unwrap();
        let output = Command::new(&binary)
            .arg(&script)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", home.path())
            .output()
            .unwrap_or_else(|err| panic!("{shell} unavailable at {binary}: {err}"));
        assert!(
            output.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            format!("{}\n{}", repo.primary.display(), repo.linked.display()),
            "{shell}"
        );
    }
}
