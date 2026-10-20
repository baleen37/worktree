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

`wt` with no arguments opens a picker for existing worktrees. `wt switch -c` fetches `origin` and creates the branch from `origin/main` or `origin/master` under `.worktrees/` (slashes in the branch name become dashes in the directory name), leaving the local `main` or `master` worktree untouched. To remove the clean feature worktree later, run `wt remove` inside it, or pass a branch name or path from anywhere in the repository; an unmerged branch is kept, while a merged branch is deleted. The primary and `main`/`master` worktrees cannot be removed.

Shell integration lets `wt switch` and `wt remove` change the calling shell's directory. Without it, `wt switch` prints the selected path. `wt config shell install` adds startup blocks for zsh, bash, and fish.

In Herdr, `wt switch` opens and focuses the target workspace while leaving the calling shell in its current worktree.

## Commands

| Command | What it does | Example |
| --- | --- | --- |
| `wt` | Open a picker for existing worktrees | `wt` |
| `wt switch <branch>` | Switch to a worktree, creating one from a local or fetched `origin` branch | `wt switch feature/search` |
| `wt switch -c [<branch>]` | Fetch `origin` and create a new branch and worktree; without a name, `wt` picks a random one | `wt switch -c fix/typo` |
| `wt list` | List worktrees with their branch and status | `wt list` |
| `wt merge [<target>]` | Merge the current branch into the target worktree, then remove the source | `wt merge` |
| `wt remove [<branch or path>]` | Remove a clean worktree, the current one by default | `wt remove feature/search` |
| `wt prune` | Preview and remove unused worktrees under the current directory | `wt prune --yes` |
| `wt config shell install` | Add shell integration to zsh, bash, and fish startup files | `wt config shell install` |

Run `wt <command> --help` for every option.

## Shell integration

`wt config shell install` adds a managed block to `~/.zshrc`, `~/.bashrc`, and `$XDG_CONFIG_HOME/fish/config.fish` (default `~/.config/fish/config.fish`). `wt config shell uninstall` removes those blocks.

To manage your startup files yourself, add the wrapper for your shell instead:

```sh
# zsh (~/.zshrc)
eval "$(wt config shell init zsh)"
# bash (~/.bashrc)
eval "$(wt config shell init bash)"
# fish (~/.config/fish/config.fish)
wt config shell init fish | source
```

## Install

### Cargo

```sh
cargo install worktree-cli --version 0.1.10 --locked
```

### macOS and Linux

Install the pinned `v0.1.10` GitHub Release with the cargo-dist shell installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/baleen37/worktree/releases/download/v0.1.10/worktree-cli-installer.sh | sh
```

Linux binaries require glibc 2.35 or newer. Use the Nix flake on older glibc systems or NixOS.

The installer places `wt` in `CARGO_HOME/bin` (usually `~/.cargo/bin`) and attempts to add that directory to `PATH`. Follow its prompt or restart your shell to refresh `PATH`.

### NixOS and Nix users

The shell installer targets macOS and Linux and does not support NixOS. Install from the Nix flake instead:

```bash
nix profile install github:baleen37/worktree/v0.1.10
```

Home Manager configurations can install the flake package declaratively. To use it as `pkgs.worktree`, add the flake's overlay to nixpkgs:

```nix
nixpkgs.overlays = [ inputs.worktree.overlays.default ];
home.packages = [ pkgs.worktree ];
```

### Manual archive installation

This example installs the macOS ARM64 archive. On Linux, set `archive` to `worktree-cli-x86_64-unknown-linux-gnu.tar.xz` or `worktree-cli-aarch64-unknown-linux-gnu.tar.xz`. The commands require the GitHub CLI (`gh`).

```sh
tag="v0.1.10"
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

Replace `v0.1.10` with the release tag you want to install. `gh release download` fetches the unified checksum file and all platform archives so `sha256.sum` can verify every archive.

## Switching and listing worktrees

`wt list` prints aligned `HERE`, `BRANCH`, `STATUS`, and `PATH` columns sized to the terminal. `[current]` marks the worktree where the command runs; narrow terminals use `*`. Status is `clean`, `dirty`, or `status unavailable`, colored green, yellow, and red when terminal colors are enabled. Paths inside the primary worktree are shown relative to it, while paths outside remain absolute. Long branch and path labels are shortened at the middle to keep both ends visible. The switch picker colors the active selection.

`wt switch <branch>` reuses a registered worktree or creates one from a local branch. If the local branch does not exist, `wt` can create a local tracking branch from an already fetched `origin/<branch>` reference:

```sh
git fetch origin
wt switch feature/example
```

`wt switch` does not fetch automatically.

Run `wt merge` from a feature worktree to merge it into the local `main` or `master` worktree. Pass a branch name to choose another target that is already checked out, such as `wt merge release`. Both worktrees must be clean. `wt merge` uses a regular Git merge, without fetching, squashing, or rebasing. After confirming the merge, it removes the source worktree and branch. If a conflict or merge error occurs, both worktrees remain and the conflict stays in the target. Shell integration switches to the target; without it, `wt merge` prints the target path.

## Pruning worktrees

`wt prune` scans the current directory and its subdirectories, including hidden directories, without following symbolic links. It can run outside a Git repository and finds linked worktrees registered by any repository under that folder. The primary checkout, the worktree at the scan root, and paths outside the folder are kept. By default it removes only unused clean linked worktrees: the branch is merged into local `main`/`master` after gaining its own commits, or its upstream is gone, or the worktree has had no commit or Git activity for three days. Worktrees that a running process (a shell or editor) has open are kept. Pass `--all` to include every clean linked worktree regardless of merge status, age, or open processes. Dirty worktrees and worktrees whose status cannot be read are kept.

A terminal run asks before removal; a non-interactive run previews only. Pass `--yes` to remove without prompting. Use `-f` or `--force` to include dirty worktrees among the selected ones (`--all --force` removes every linked worktree). Repeat the force option (`-ff` or `--force --force`) to remove Git-locked worktrees. Branch references are kept, and nested worktrees are removed from child to parent. When run from an active Herdr workspace, Herdr synchronization applies only to that repository.

`--stale` remains as a deprecated alias for the default cleanup and prints a warning.

## License

Licensed under either the MIT License or the Apache License, Version 2.0, at your option.
