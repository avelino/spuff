pub mod commands;

use clap::{Parser, Subcommand};

use crate::config::AppConfig;
use crate::error::Result;

#[derive(Parser)]
#[command(name = "spuff")]
#[command(version)]
#[command(about = "Ephemeral dev environments in the cloud")]
#[command(long_about = "Spin up cloud VMs with your dev environment, auto-destroy when idle.\n\nNo more forgotten instances. No surprise bills.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize spuff configuration
    Init,

    /// Create and connect to a new dev environment
    Up {
        /// Instance size (e.g., s-2vcpu-4gb)
        #[arg(short, long)]
        size: Option<String>,

        /// Use a specific snapshot
        #[arg(long)]
        snapshot: Option<String>,

        /// Region for the instance
        #[arg(short, long)]
        region: Option<String>,

        /// Don't connect after creation
        #[arg(long)]
        no_connect: bool,

        /// Development mode: upload local spuff-agent binary instead of downloading from GitHub
        #[arg(long)]
        dev: bool,
    },

    /// Destroy the current environment
    Down {
        /// Create snapshot before destroying
        #[arg(long)]
        snapshot: bool,

        /// Force destroy without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// SSH into existing environment
    Ssh,

    /// Show environment status
    Status {
        /// Show detailed information
        #[arg(short, long)]
        detailed: bool,
    },

    /// View project setup logs from the remote environment
    Logs {
        /// Show logs for a specific bundle (e.g., rust, go, python)
        #[arg(long)]
        bundle: Option<String>,

        /// Show package installation logs
        #[arg(long)]
        packages: bool,

        /// Show repository cloning logs
        #[arg(long)]
        repos: bool,

        /// Show docker services logs
        #[arg(long)]
        services: bool,

        /// Show logs for a specific setup script (by index, starting at 1)
        #[arg(long)]
        script: Option<usize>,

        /// Follow mode: continuously watch for new logs (like tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show (default: 100)
        #[arg(short = 'n', long, default_value = "100")]
        lines: usize,
    },

    /// Create SSH tunnels to the remote environment (for ports in spuff.yaml)
    Tunnel {
        /// Specific port to tunnel (default: all ports from spuff.yaml)
        #[arg(short, long)]
        port: Option<u16>,

        /// Stop running tunnels
        #[arg(long)]
        stop: bool,
    },

    /// Manage snapshots
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommands,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// Interact with the remote agent
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },

    /// Execute a command on the remote environment
    Exec {
        /// Command to execute
        command: String,
    },
}

#[derive(Subcommand)]
pub enum SnapshotCommands {
    /// Create a snapshot of the current environment
    Create {
        /// Name for the snapshot
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List available snapshots
    List,

    /// Delete a snapshot
    Delete {
        /// Snapshot ID or name
        id: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },

    /// Open configuration file in editor
    Edit,
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Show agent status and system metrics
    Status,

    /// Show system metrics as JSON
    Metrics,

    /// Show top processes
    Processes,

    /// View system logs from the remote environment
    Logs {
        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "100")]
        lines: usize,

        /// Log file to read (default: /var/log/cloud-init-output.log)
        #[arg(short, long)]
        file: Option<String>,
    },

    /// View agent activity log (what the agent has executed)
    Activity {
        /// Number of entries to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,

        /// Follow mode: continuously watch for new activity (like tail -f)
        #[arg(short, long)]
        follow: bool,
    },

    /// View persistent exec log (survives agent restarts)
    ExecLog {
        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
}

impl Cli {
    pub async fn execute(self) -> Result<()> {
        match self.command {
            Commands::Init => commands::init::execute().await,
            Commands::Up {
                size,
                snapshot,
                region,
                no_connect,
                dev,
            } => {
                let config = AppConfig::load()?;
                commands::up::execute(&config, size, snapshot, region, no_connect, dev).await
            }
            Commands::Down { snapshot, force } => {
                let config = AppConfig::load()?;
                commands::down::execute(&config, snapshot, force).await
            }
            Commands::Ssh => {
                let config = AppConfig::load()?;
                commands::ssh::execute(&config).await
            }
            Commands::Status { detailed } => {
                let config = AppConfig::load()?;
                commands::status::execute(&config, detailed).await
            }
            Commands::Logs {
                bundle,
                packages,
                repos,
                services,
                script,
                follow,
                lines,
            } => {
                let config = AppConfig::load()?;
                commands::logs::execute(&config, bundle, packages, repos, services, script, follow, lines).await
            }
            Commands::Tunnel { port, stop } => {
                let config = AppConfig::load()?;
                commands::ssh::tunnel(&config, port, stop).await
            }
            Commands::Snapshot { command } => {
                let config = AppConfig::load()?;
                match command {
                    SnapshotCommands::Create { name } => {
                        commands::snapshot::create(&config, name).await
                    }
                    SnapshotCommands::List => commands::snapshot::list(&config).await,
                    SnapshotCommands::Delete { id } => commands::snapshot::delete(&config, id).await,
                }
            }
            Commands::Config { command } => match command {
                ConfigCommands::Show => commands::config::show().await,
                ConfigCommands::Set { key, value } => commands::config::set(key, value).await,
                ConfigCommands::Edit => commands::config::edit().await,
            },
            Commands::Agent { command } => {
                let config = AppConfig::load()?;
                match command {
                    AgentCommands::Status => commands::agent::status(&config).await,
                    AgentCommands::Metrics => commands::agent::metrics(&config).await,
                    AgentCommands::Processes => commands::agent::processes(&config).await,
                    AgentCommands::Logs { lines, file } => {
                        commands::agent::logs(&config, lines, file).await
                    }
                    AgentCommands::Activity { limit, follow } => {
                        commands::agent::activity(&config, limit, follow).await
                    }
                    AgentCommands::ExecLog { lines } => {
                        commands::agent::exec_log(&config, lines).await
                    }
                }
            }
            Commands::Exec { command } => {
                let config = AppConfig::load()?;
                commands::agent::exec(&config, command).await
            }
        }
    }
}
