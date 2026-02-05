// krill ps - Attach TUI to running daemon

use crate::daemon_manager;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use tracing::info;

#[derive(clap::Args, Debug)]
pub struct PsArgs {
    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    pub socket: PathBuf,
}

pub async fn execute(args: PsArgs) -> Result<()> {
    // Check if daemon is running
    if !daemon_manager::is_daemon_running(&args.socket).await {
        return Err(anyhow!("Daemon is not running. Start it with 'krill up'"));
    }

    info!("Attaching TUI to daemon...");
    let tui_config = krill_tui::TuiConfig {
        socket: args.socket,
    };

    krill_tui::run(tui_config).await?;
    Ok(())
}
