# worktree

`worktree` is a Git worktree manager written in Rust. Its command line executable is `wt`.

```sh
wt --help
```

## Install

### Cargo

After `worktree-cli` v0.1.2 is published to crates.io:

```sh
cargo install worktree-cli --locked
```

### macOS and Linux

After the `v0.1.2` GitHub Release is published, install that pinned version with the cargo-dist shell installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/baleen37/worktree/releases/download/v0.1.2/worktree-cli-installer.sh | sh
```

Linux archives require glibc 2.35 or newer. Use the Nix flake on older glibc systems or NixOS.

The installer places `wt` in `CARGO_HOME/bin` (usually `~/.cargo/bin`) and attempts to add that directory to `PATH`. Follow its prompt or restart your shell to refresh `PATH`.

The binary does not change the current shell directory by itself. To enable `wt switch` and `wt remove` to move the calling shell, run:

```bash
wt config shell install
```

### NixOS and Nix users

The shell installer targets macOS and Linux and does not support NixOS. Install from the Nix flake instead:

```bash
nix profile install github:baleen37/worktree/v0.1.2
```

Home Manager configurations can continue to install the flake package declaratively.

### Manual archive installation

Choose the archive matching your platform. The example below uses macOS ARM64; use `worktree-cli-x86_64-unknown-linux-gnu.tar.xz` or `worktree-cli-aarch64-unknown-linux-gnu.tar.xz` on Linux.

```sh
tag="v0.1.2"
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

Replace `v0.1.2` with the release tag you want to install. `gh release download` fetches the unified checksum file and all platform archives so `sha256.sum` can verify every archive.

## License

Licensed under either the MIT License or the Apache License, Version 2.0, at your option.
