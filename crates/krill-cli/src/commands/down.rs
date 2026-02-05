// krill down - Stop all services and the daemon

use crate::daemon_manager;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct DownArgs {
    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    pub socket: PathBuf,
}

pub async fn execute(args: DownArgs) -> Result<()> {
    // Check if daemon is running
    if !daemon_manager::is_daemon_running(&args.socket).await {
        return Err(anyhow!("Daemon is not running"));
    }

    println!("Stopping all services and daemon...");
    daemon_manager::stop_daemon(&args.socket).await?;
    println!("Daemon stopped successfully");
    Ok(())
}
