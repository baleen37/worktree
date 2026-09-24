#![allow(dead_code)] // Each integration test binary uses a different subset of this shared harness.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command as ProcessCommand;

use assert_cmd::Command;
use tempfile::TempDir;

pub struct TestWt {
    _bin: TempDir,
    path: OsString,
}

impl TestWt {
    pub fn new() -> Self {
        let bin = tempfile::tempdir().unwrap();
        let nix = bin.path().join("nix");
        std::fs::write(&nix, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&nix, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        Self { _bin: bin, path }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::cargo_bin("wt").unwrap();
        command
            .env("PATH", &self.path)
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_WORKSPACE_ID");
        command
    }

    pub fn process_command(&self) -> ProcessCommand {
        let mut command = ProcessCommand::new(assert_cmd::cargo::cargo_bin("wt"));
        command
            .env("PATH", &self.path)
            .env_remove("HERDR_ENV")
            .env_remove("HERDR_WORKSPACE_ID");
        command
    }

    pub fn path_with(&self, first: &Path) -> OsString {
        std::env::join_paths(
            std::iter::once(first.to_path_buf()).chain(std::env::split_paths(&self.path)),
        )
        .unwrap()
    }
}
