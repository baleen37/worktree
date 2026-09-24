use clap::{Parser, Subcommand};

use crate::git::RepoContext;
use crate::picker;
use crate::shell::{self, Shell};
use crate::worktree::{PruneOptions, WorktreeInfo, create_branch, prune, remove, switch_existing};

#[derive(Debug, Parser)]
#[command(name = "wt", version, about = "Git worktree manager written in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Switch to a worktree, or create one for a branch.
    Switch {
        #[arg(short = 'c', num_args = 0..=1)]
        create: Option<Option<String>>,
        branch: Option<String>,
    },
    /// List worktrees.
    List,
    /// Remove a worktree.
    Remove { target: Option<String> },
    /// Preview and remove eligible worktrees.
    Prune {
        /// Include clean unmerged worktrees older than 30 days.
        #[arg(long)]
        stale: bool,
        /// Remove without confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Manage shell integration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Manage shell integration.
    Shell {
        #[command(subcommand)]
        command: ShellCommands,
    },
}

#[derive(Debug, Subcommand)]
enum ShellCommands {
    /// Print a shell wrapper.
    Init { shell: Shell },
    /// Install shell startup blocks.
    Install,
    /// Remove shell startup blocks.
    Uninstall,
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Switch {
        create: None,
        branch: None,
    }) {
        Commands::Switch { create, branch } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            let shell_path_file =
                std::env::var_os("WT_SHELL_PATH_FILE").map(std::path::PathBuf::from);
            let path = if let Some(name) = create {
                if branch.is_some() {
                    anyhow::bail!("branch cannot be provided separately from -c");
                }
                Some(create_branch(
                    &repo,
                    name.as_deref(),
                    shell_path_file.as_deref(),
                )?)
            } else if let Some(branch) = branch {
                Some(switch_existing(&repo, &branch, shell_path_file.as_deref())?)
            } else {
                picker::select(&repo, shell_path_file.as_deref())?
            };
            if let Some(path) = path.filter(|_| shell_path_file.is_none()) {
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
        Commands::Remove { target } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            let shell_path_file =
                std::env::var_os("WT_SHELL_PATH_FILE").map(std::path::PathBuf::from);
            remove(&repo, target.as_deref(), shell_path_file.as_deref())?;
        }
        Commands::Prune { stale, yes } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            prune(&repo, PruneOptions { stale, yes })?;
        }
        Commands::Config { command } => match command {
            ConfigCommands::Shell { command } => match command {
                ShellCommands::Init { shell } => print!("{}", shell::init(shell)),
                ShellCommands::Install => shell::install()?,
                ShellCommands::Uninstall => shell::uninstall()?,
            },
        },
    }
    Ok(())
}
