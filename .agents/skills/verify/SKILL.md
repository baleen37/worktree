---
name: verify
description: Verify wt CLI behavior against disposable Git repositories.
---

Build with `cargo build --locked`. On macOS with a stale Nix SDK, use
`DEVELOPER_DIR=/Library/Developer/CommandLineTools SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk cargo build --locked`.

Use the absolute path to `target/debug/wt` in a fresh temporary directory.
Initialize a primary checkout with an initial commit and add linked worktrees
under the temporary scan root. Never run deletion probes on real user worktrees.

For prune changes, capture CLI output and inspect paths and branch references:
1. Create recent, unmerged, dirty, and Git-locked linked worktrees.
2. Run `wt prune -f` with stdout piped: it must preview and keep all paths.
3. Run `wt prune -fy`: eligible paths disappear, primary and branches remain.
4. Probe `--force --yes`, `-ffy`, and a repeated prune on the empty scope.
5. Check `wt prune --help` for the documented flags.

Also exercise confirmation through a PTY when changing prompt behavior, and
verify scan-root/out-of-scope protection and child-first nested removal when
changing selection or ordering. Preserve stdout/stderr as review evidence.
