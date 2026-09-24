mod cli;
pub mod git;
pub mod worktree;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
