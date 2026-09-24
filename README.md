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

For manual installation, download the archive for your platform from [GitHub Releases](https://github.com/baleen37/worktree/releases). Verify the checksums and artifact attestation before extracting an archive:

```sh
shasum -a 256 -c sha256.sum
gh attestation verify <archive> --repo baleen37/worktree
```

## License

Licensed under either the MIT License or the Apache License, Version 2.0, at your option.
