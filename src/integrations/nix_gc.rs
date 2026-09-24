use std::process::{Command, Stdio};

pub fn start() {
    let _ = Command::new("nix")
        .args(["store", "gc"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
