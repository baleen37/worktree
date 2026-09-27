use clap::{Parser, Subcommand};

use crate::git::RepoContext;
use crate::integrations::herdr::Herdr;
use crate::picker;
use crate::shell::{self, Shell};
use crate::worktree::{
    PruneOptions, WorktreeInfo, create_branch, merge, prune, remove, switch_existing,
    worktree_labels,
};

#[derive(Debug, Parser)]
#[command(name = "wt", version, about = "Git worktree manager written in Rust")]
struct Cli {
    #[arg(long, hide = true)]
    internal_herdr_detached: bool,
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
    /// Merge the current branch into a target worktree and remove the source worktree.
    Merge { target: Option<String> },
    /// Remove a worktree.
    Remove { target: Option<String> },
    /// Preview and remove eligible worktrees.
    Prune {
        /// Deprecated alias for the default three-day cleanup.
        #[arg(long)]
        stale: bool,
        /// Force-remove all registered worktrees except the primary and current worktrees.
        #[arg(long)]
        all: bool,
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
    if matches!(
        cli.command.as_ref(),
        Some(Commands::Merge { .. } | Commands::Remove { .. })
    ) && !cli.internal_herdr_detached
        && Herdr::is_active_context()
    {
        let status = Herdr::run_lifecycle_in_detached_session()?;
        anyhow::ensure!(
            status.success(),
            "detached wt lifecycle command exited with {status}"
        );
        return Ok(());
    }
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
            let entries = WorktreeInfo::list(&repo)?;
            let terminal_width = console::Term::stdout().size().1 as usize;
            let labels = worktree_labels(&repo, &entries, terminal_width, 0, true);
            println!("{}", labels.header);
            for row in labels.rows {
                println!("{row}");
            }
        }
        Commands::Merge { target } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            let shell_path_file =
                std::env::var_os("WT_SHELL_PATH_FILE").map(std::path::PathBuf::from);
            let path = merge(&repo, target.as_deref(), shell_path_file.as_deref())?;
            if shell_path_file.is_none() {
                println!("{}", path.display());
            }
        }
        Commands::Remove { target } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            let shell_path_file =
                std::env::var_os("WT_SHELL_PATH_FILE").map(std::path::PathBuf::from);
            remove(&repo, target.as_deref(), shell_path_file.as_deref())?;
        }
        Commands::Prune { stale, all, yes } => {
            let repo = RepoContext::discover(&std::env::current_dir()?)?;
            if stale {
                eprintln!("warning: --stale is deprecated; it is now the default behavior");
            }
            prune(&repo, PruneOptions { all, yes })?;
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
