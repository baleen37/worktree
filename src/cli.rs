use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "wt", version, about = "Git worktree manager written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Switch to a worktree, or create one for a branch.
    Switch,
    /// List worktrees.
    List,
    /// Remove a worktree.
    Remove,
    /// Prune stale worktree metadata.
    Prune,
    /// Manage shell integration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Print shell integration instructions.
    Shell,
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let _command = cli.command.unwrap_or(Commands::Switch);
    Ok(())
}
