use clap::{Parser, Subcommand};

use crate::git::RepoContext;
use crate::worktree::{WorktreeInfo, switch_existing};

#[derive(Debug, Parser)]
#[command(name = "wt", version, about = "Git worktree manager written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Switch to a worktree, or create one for a branch.
    Switch { branch: Option<String> },
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
    match cli.command.unwrap_or(Commands::Switch { branch: None }) {
        Commands::Switch { branch } => {
            let branch = branch.ok_or_else(|| anyhow::anyhow!("branch is required"))?;
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            let shell_path_file =
                std::env::var_os("WT_SHELL_PATH_FILE").map(std::path::PathBuf::from);
            let path = switch_existing(&repo, &branch, shell_path_file.as_deref())?;
            if shell_path_file.is_none() {
                println!("{}", path.display());
            }
        }
        Commands::List => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            for entry in WorktreeInfo::list(&repo)? {
                let branch = entry.branch.as_deref().unwrap_or("(detached HEAD)");
                println!("{}  {branch}", entry.path.display());
            }
        }
        _ => {}
    }
    Ok(())
}
