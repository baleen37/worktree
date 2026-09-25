# worktree

`wt` is a Git worktree manager. It creates a separate checkout for each branch, lets you switch between worktrees from your shell, and refuses to remove worktrees with uncommitted changes.

## Quick start

After installing `wt`, enable shell integration once:

```sh
wt config shell install
```

Restart your shell, then run these commands from a Git repository:

```sh
wt switch -c feature/search
wt
wt list
```

`wt` with no arguments opens a picker for existing worktrees. `wt switch -c` fetches `origin`, fast-forwards the clean local `main` or `master` worktree, and creates the branch under `.worktrees/`. To remove the clean feature worktree later, switch to it and run `wt remove`; an unmerged branch is kept, while a merged branch is deleted.

Shell integration lets `wt switch` and `wt remove` change the calling shell's directory. Without it, `wt switch` prints the selected path. `wt config shell install` adds startup blocks for zsh, bash, and fish.

## Install

### Cargo

After `worktree-cli` v0.1.3 is published to crates.io:

```sh
cargo install worktree-cli --version 0.1.3 --locked
```

### macOS and Linux

After the `v0.1.3` GitHub Release is published, install that pinned version with the cargo-dist shell installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/baleen37/worktree/releases/download/v0.1.3/worktree-cli-installer.sh | sh
```

Linux binaries require glibc 2.35 or newer. Use the Nix flake on older glibc systems or NixOS.

The installer places `wt` in `CARGO_HOME/bin` (usually `~/.cargo/bin`) and attempts to add that directory to `PATH`. Follow its prompt or restart your shell to refresh `PATH`.

### NixOS and Nix users

The shell installer targets macOS and Linux and does not support NixOS. Install from the Nix flake instead:

```bash
nix profile install github:baleen37/worktree/v0.1.3
```

Home Manager configurations can install the flake package declaratively.

### Manual archive installation

This example installs the macOS ARM64 archive. On Linux, set `archive` to `worktree-cli-x86_64-unknown-linux-gnu.tar.xz` or `worktree-cli-aarch64-unknown-linux-gnu.tar.xz`. The commands require the GitHub CLI (`gh`).

```sh
tag="v0.1.3"
archive="worktree-cli-aarch64-apple-darwin.tar.xz"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
gh release download "$tag" --repo baleen37/worktree --dir "$tmp_dir"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp_dir" && sha256sum -c sha256.sum)
else
  (cd "$tmp_dir" && shasum -a 256 -c sha256.sum)
fi
gh attestation verify "$tmp_dir/$archive" --repo baleen37/worktree
install_dir="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$tmp_dir/extracted" "$install_dir"
tar -xf "$tmp_dir/$archive" -C "$tmp_dir/extracted"
install -m 755 "$tmp_dir/extracted/${archive%.tar.xz}/wt" "$install_dir/wt"
```

Replace `v0.1.3` with the release tag you want to install. `gh release download` fetches the unified checksum file and all platform archives so `sha256.sum` can verify every archive.

## License

Licensed under either the MIT License or the Apache License, Version 2.0, at your option.
