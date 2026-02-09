// Krill CLI - Unified command-line interface

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io;
use tracing_subscriber::{fmt, EnvFilter};

mod commands;
mod config_discovery;
mod daemon_manager;

#[derive(Parser, Debug)]
#[command(name = "krill")]
#[command(about = "Krill process orchestrator", long_about = None)]
#[command(version)]
struct Cli {
    /// Verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start daemon and attach TUI
    Up(commands::UpArgs),

    /// Stop all services and the daemon
    Down(commands::DownArgs),

    /// Attach TUI to running daemon
    Ps(commands::PsArgs),

    /// View logs
    Logs(commands::LogsArgs),

    /// Run daemon directly (internal use)
    #[command(hide = true)]
    Daemon(commands::DaemonArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing for CLI commands only (daemon initializes its own)
    let is_daemon_command = matches!(cli.command, Some(Commands::Daemon(_)));

    if !is_daemon_command {
        let filter = if cli.verbose {
            EnvFilter::new("debug")
        } else {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
        };

        fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(io::stderr)
            .init();
    }

    // If no subcommand provided, try to attach to daemon or show help
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            // Check if daemon is running
            let socket = std::path::PathBuf::from("/tmp/krill.sock");
            if crate::daemon_manager::is_daemon_running(&socket).await {
                // Attach to running daemon
                Commands::Ps(commands::PsArgs { socket })
            } else {
                // No daemon running, show help
                use clap::CommandFactory;
                Cli::command().print_help()?;
                println!();
                return Ok(());
            }
        }
    };

    match command {
        Commands::Up(args) => commands::up(args).await,
        Commands::Down(args) => commands::down(args).await,
        Commands::Ps(args) => commands::ps(args).await,
        Commands::Logs(args) => commands::logs(args).await,
        Commands::Daemon(args) => commands::daemon(args).await,
    }
}
