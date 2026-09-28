use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_v1_commands() {
    Command::cargo_bin("wt")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("switch"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("merge"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("prune"))
        .stdout(predicate::str::contains("config"));
}

#[test]
fn prune_help_describes_force_alias_and_locked_worktrees() {
    Command::cargo_bin("wt")
        .unwrap()
        .args(["prune", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--all"))
        .stdout(predicate::str::contains("locked"));
}

#[test]
fn version_matches_package_version() {
    Command::cargo_bin("wt")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}
