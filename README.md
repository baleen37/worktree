# worktree

`worktree` is a Git worktree manager written in Rust. Its command line executable is `wt`.

```sh
wt --help
```

## Install

Install with Homebrew:

```sh
brew install baleen37/tap/worktree
```

For manual installation, download all release assets so the unified `sha256.sum` can check every platform archive. Replace `vX.Y.Z` with the release tag, then verify the checksums and the attestation for your platform archive:

```sh
tag="vX.Y.Z"
gh release download "$tag" --repo baleen37/worktree
shasum -a 256 -c sha256.sum
archive="worktree-aarch64-apple-darwin.tar.xz" # choose the archive for your platform
gh attestation verify "$archive" --repo baleen37/worktree
```

## License

Licensed under either the MIT License or the Apache License, Version 2.0, at your option.
