use std::process::{Command, Stdio};

pub fn start() {
    let _ = Command::new("nix")
        .args([
            "--extra-experimental-features",
            "nix-command",
            "store",
            "gc",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
