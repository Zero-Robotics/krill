// krill up - Start daemon and optionally attach TUI

use crate::{config_discovery, daemon_manager};
use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

#[derive(clap::Args, Debug)]
pub struct UpArgs {
    /// Configuration file (defaults to ./krill.yaml)
    pub config: Option<PathBuf>,

    /// Run daemon without attaching TUI
    #[arg(short, long)]
    pub detached: bool,

    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    pub socket: PathBuf,
}

pub async fn execute(args: UpArgs) -> Result<()> {
    // Discover config file
    let config_path = config_discovery::discover_config(args.config)?;
    info!("Using configuration: {:?}", config_path);

    // Check if daemon is already running
    let daemon_running = daemon_manager::is_daemon_running(&args.socket).await;

    if !daemon_running {
        info!("Starting daemon...");

        // Start daemon in background
        daemon_manager::start_daemon_background(&config_path, &args.socket, None).await?;

        // Wait for daemon to be ready
        daemon_manager::wait_for_socket(&args.socket, Duration::from_secs(10)).await?;

        println!("Daemon started successfully");
    } else {
        println!("Daemon already running");
    }

    // Launch TUI unless detached mode
    if !args.detached {
        info!("Launching TUI...");
        let tui_config = krill_tui::TuiConfig {
            socket: args.socket,
        };

        krill_tui::run(tui_config).await?;
    } else {
        println!("Running in detached mode. Use 'krill ps' to attach TUI.");
    }

    Ok(())
}
